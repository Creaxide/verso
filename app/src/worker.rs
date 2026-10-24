//! Background work.
//!
//! Parsing and conversion happen away from the paint thread. The worker sends
//! plain data back over a channel; the UI decides what to do with it. One job
//! runs at a time, and every job checks the cancel flag between files.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

use eframe::egui;
use rayon::prelude::*;
use verso_core::{
    convert_report, is_supported, pair_info, paths, read_document, Format, Options,
};

use crate::state::{BatchStatus, BatchSummary, Completed, Preview};

const SNIPPET_CHARS: usize = 1600;

/// What the worker reports back to the app.
pub enum Event {
    Loaded {
        generation: u64,
        path: PathBuf,
        bytes: Vec<u8>,
        format: Format,
        preview: Result<Preview, String>,
    },
    Converted {
        generation: u64,
        completed: Completed,
    },
    Failed {
        generation: u64,
        message: String,
    },
    BatchFile {
        generation: u64,
        index: usize,
        status: BatchStatus,
    },
    BatchDone {
        generation: u64,
        summary: BatchSummary,
        cancelled: bool,
    },
}

/// A conversion request. The bytes are already in memory for a single file;
/// batch files are read on the worker threads.
pub struct ConvertJob {
    pub input: PathBuf,
    pub bytes: Vec<u8>,
    pub source: Format,
    pub target: Format,
}

/// One file of a batch.
pub struct BatchInput {
    pub path: PathBuf,
    pub format: Format,
}

pub struct Worker {
    tx: Sender<Event>,
    rx: Receiver<Event>,
    ctx: egui::Context,
    cancel: Arc<AtomicBool>,
}

impl Worker {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            ctx,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Everything that arrived since the last frame.
    pub fn drain(&self) -> Vec<Event> {
        self.rx.try_iter().collect()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Read a file and describe it. `format` overrides extension detection.
    pub fn load(&self, generation: u64, path: PathBuf, format: Option<Format>, options: Options) {
        self.cancel.store(false, Ordering::SeqCst);
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();

        let spawned = std::thread::Builder::new()
            .name("verso-load".to_string())
            .spawn(move || {
                let result = load_file(&path, format, &options);
                match result {
                    Ok((bytes, format, preview)) => emit(
                        &tx,
                        &ctx,
                        Event::Loaded {
                            generation,
                            path,
                            bytes,
                            format,
                            preview,
                        },
                    ),
                    Err(message) => emit(
                        &tx,
                        &ctx,
                        Event::Failed {
                            generation,
                            message,
                        },
                    ),
                }
            });

        if let Err(error) = spawned {
            emit(
                &self.tx,
                &self.ctx,
                Event::Failed {
                    generation,
                    message: format!("Verso could not start its reader: {error}"),
                },
            );
        }
    }

    /// Convert one file, then write it next to its source.
    pub fn convert(&self, generation: u64, job: ConvertJob, options: Options) {
        self.cancel.store(false, Ordering::SeqCst);
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let cancel = Arc::clone(&self.cancel);

        let spawned = std::thread::Builder::new()
            .name("verso-convert".to_string())
            .spawn(move || {
                let started = Instant::now();
                let outcome =
                    catch_unwind(AssertUnwindSafe(|| convert_one(&job, &options)));

                match outcome {
                    Ok(Ok(mut completed)) => {
                        completed.elapsed_ms = started.elapsed().as_millis();
                        emit(&tx, &ctx, Event::Converted {
                            generation,
                            completed,
                        });
                    }
                    Ok(Err(message)) => emit(
                        &tx,
                        &ctx,
                        Event::Failed {
                            generation,
                            message,
                        },
                    ),
                    Err(_) if cancel.load(Ordering::SeqCst) => emit(
                        &tx,
                        &ctx,
                        Event::Failed {
                            generation,
                            message: "The conversion was stopped before it finished.".to_string(),
                        },
                    ),
                    Err(_) => emit(
                        &tx,
                        &ctx,
                        Event::Failed {
                            generation,
                            message: "Verso hit an internal problem converting this file. The source file was not changed."
                                .to_string(),
                        },
                    ),
                }
            });

        if let Err(error) = spawned {
            emit(
                &self.tx,
                &self.ctx,
                Event::Failed {
                    generation,
                    message: format!("Verso could not start its converter: {error}"),
                },
            );
        }
    }

    /// Convert every file that is not already done, in parallel.
    pub fn batch(
        &self,
        generation: u64,
        files: Vec<BatchInput>,
        target: Format,
        options: Options,
    ) {
        self.cancel.store(false, Ordering::SeqCst);
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let cancel = Arc::clone(&self.cancel);

        let spawned = std::thread::Builder::new()
            .name("verso-batch".to_string())
            .spawn(move || {
                let started = Instant::now();
                let total = files.len();
                let finished = AtomicUsize::new(0);
                let done = AtomicUsize::new(0);
                let failed = AtomicUsize::new(0);
                let skipped = AtomicUsize::new(0);
                let bytes = AtomicUsize::new(0);
                let warnings = AtomicUsize::new(0);
                let folder = std::sync::Mutex::new(None::<PathBuf>);

                files.par_iter().enumerate().for_each(|(index, file)| {
                    if cancel.load(Ordering::SeqCst) {
                        return;
                    }

                    emit(
                        &tx,
                        &ctx,
                        Event::BatchFile {
                            generation,
                            index,
                            status: BatchStatus::Running,
                        },
                    );

                    let status = if !is_supported(file.format, target) {
                        skipped.fetch_add(1, Ordering::Relaxed);
                        BatchStatus::Skipped(unsupported_message(file.format, target))
                    } else {
                        match catch_unwind(AssertUnwindSafe(|| {
                            convert_batch_file(file, target, &options)
                        })) {
                            Ok(Ok((output, written, carried, elapsed_ms))) => {
                                done.fetch_add(1, Ordering::Relaxed);
                                bytes.fetch_add(written, Ordering::Relaxed);
                                warnings.fetch_add(carried.len(), Ordering::Relaxed);
                                if let Ok(mut slot) = folder.lock() {
                                    if slot.is_none() {
                                        *slot = Some(
                                            output
                                                .parent()
                                                .map(Path::to_path_buf)
                                                .unwrap_or_else(|| PathBuf::from(".")),
                                        );
                                    }
                                }
                                BatchStatus::Done {
                                    output,
                                    bytes: written,
                                    warnings: carried,
                                    elapsed_ms,
                                }
                            }
                            Ok(Err(message)) => {
                                failed.fetch_add(1, Ordering::Relaxed);
                                BatchStatus::Failed(message)
                            }
                            Err(_) => {
                                failed.fetch_add(1, Ordering::Relaxed);
                                BatchStatus::Failed(
                                    "Verso hit an internal problem with this file. It was not changed."
                                        .to_string(),
                                )
                            }
                        }
                    };

                    emit(
                        &tx,
                        &ctx,
                        Event::BatchFile {
                            generation,
                            index,
                            status,
                        },
                    );

                    finished.fetch_add(1, Ordering::Relaxed);
                    let _ = total;
                });

                let cancelled = cancel.load(Ordering::SeqCst);
                let folder = folder.lock().ok().and_then(|slot| slot.clone());
                emit(
                    &tx,
                    &ctx,
                    Event::BatchDone {
                        generation,
                        summary: BatchSummary {
                            done: done.load(Ordering::Relaxed),
                            failed: failed.load(Ordering::Relaxed),
                            skipped: skipped.load(Ordering::Relaxed),
                            bytes: bytes.load(Ordering::Relaxed) as u64,
                            warnings: warnings.load(Ordering::Relaxed),
                            elapsed_ms: started.elapsed().as_millis(),
                            folder,
                        },
                        cancelled,
                    },
                );
            });

        if let Err(error) = spawned {
            emit(
                &self.tx,
                &self.ctx,
                Event::Failed {
                    generation,
                    message: format!("Verso could not start the batch: {error}"),
                },
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Work
// ---------------------------------------------------------------------------

fn load_file(
    path: &Path,
    format: Option<Format>,
    options: &Options,
) -> Result<(Vec<u8>, Format, Result<Preview, String>), String> {
    let bytes = std::fs::read(path).map_err(|error| read_message(path, &error))?;
    if let Err(error) = options.check_size(&bytes) {
        return Err(error.to_string());
    }
    let format = format.unwrap_or_else(|| Format::detect(Some(path), &bytes));
    let preview = preview_of(format, &bytes, options);
    Ok((bytes, format, preview))
}

fn convert_one(job: &ConvertJob, options: &Options) -> Result<Completed, String> {
    if !is_supported(job.source, job.target) {
        return Err(unsupported_message(job.source, job.target));
    }

    let report = convert_report(job.source, job.target, &job.bytes, options)
        .map_err(|error| error.to_string())?;

    let output = paths::write_output(&job.input, job.target, &report.bytes)
        .map_err(|error| error.to_string())?;

    let proof = preview_of(job.target, &report.bytes, options).ok();
    let (snippet, truncated) = proof
        .map(|preview| (preview.snippet, preview.truncated))
        .unwrap_or_default();

    Ok(Completed {
        output,
        source_format: job.source,
        output_format: job.target,
        bytes: report.bytes.len(),
        warnings: report.warnings,
        snippet,
        truncated,
        elapsed_ms: 0,
    })
}

fn convert_batch_file(
    file: &BatchInput,
    target: Format,
    options: &Options,
) -> Result<(PathBuf, usize, Vec<String>, u128), String> {
    let started = Instant::now();
    let bytes = std::fs::read(&file.path).map_err(|error| read_message(&file.path, &error))?;
    let report = convert_report(file.format, target, &bytes, options)
        .map_err(|error| error.to_string())?;
    let output = paths::write_output(&file.path, target, &report.bytes)
        .map_err(|error| error.to_string())?;
    Ok((
        output,
        report.bytes.len(),
        report.warnings,
        started.elapsed().as_millis(),
    ))
}

/// A short, human description of the first stretch of a document.
fn preview_of(format: Format, bytes: &[u8], options: &Options) -> Result<Preview, String> {
    let document = read_document(format, bytes, options).map_err(|error| error.to_string())?;
    if document.blocks.is_empty() {
        return Err("This document has no readable content.".to_string());
    }
    let text = clean_preview(&document.text_only());
    let (snippet, truncated) = truncate(&text, SNIPPET_CHARS);
    Ok(Preview {
        summary: document.summary(),
        snippet,
        truncated,
    })
}

fn clean_preview(text: &str) -> String {
    let mut out = String::with_capacity(text.len().min(SNIPPET_CHARS * 2));
    for ch in text.trim().chars() {
        match ch {
            '\n' | '\t' => out.push(ch),
            '\r' => {}
            ch if ch.is_control() => out.push(' '),
            ch => out.push(ch),
        }
        if out.len() >= SNIPPET_CHARS * 4 {
            break;
        }
    }
    out.trim_end().to_string()
}

fn truncate(text: &str, limit: usize) -> (String, bool) {
    if text.chars().count() <= limit {
        return (text.to_string(), false);
    }
    let mut cut: String = text.chars().take(limit).collect();
    if let Some(position) = cut.rfind('\n') {
        if position > limit / 2 {
            cut.truncate(position);
        }
    }
    cut.push('…');
    (cut, true)
}

fn unsupported_message(from: Format, to: Format) -> String {
    let reason = pair_info(from, to)
        .notes
        .first()
        .copied()
        .unwrap_or("No converter is registered for this pair.");
    format!("{} → {} is not supported. {reason}", from.name(), to.name())
}

fn read_message(path: &Path, error: &std::io::Error) -> String {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("The file");
    match error.kind() {
        std::io::ErrorKind::NotFound => format!("{name} is no longer at that location."),
        std::io::ErrorKind::PermissionDenied => {
            format!("{name} cannot be read. Another program may have it open.")
        }
        _ => format!("{name} could not be read: {error}"),
    }
}

fn emit(tx: &Sender<Event>, ctx: &egui::Context, event: Event) {
    // A closed receiver means the app is shutting down; there is nothing to do.
    let _ = tx.send(event);
    ctx.request_repaint();
}

/// Used by the app to explain a blocked converter pair the same way the worker
/// would. Kept here so the wording cannot drift.
pub fn pair_refusal(from: Format, to: Format) -> String {
    unsupported_message(from, to)
}
