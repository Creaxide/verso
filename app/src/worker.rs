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
    convert_report, image_info, images_to_pdf, is_supported, pair_info, paths, read_document,
    DocumentSummary, Format, ImageInput, Options, OutputFile,
};

use crate::state;
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
    PageProgress {
        generation: u64,
        done: usize,
        total: usize,
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
                let mut progress = |done: usize, total: usize| {
                    emit(
                        &tx,
                        &ctx,
                        Event::PageProgress {
                            generation,
                            done,
                            total,
                        },
                    );
                };
                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    convert_one(&job, &options, &mut progress)
                }));

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

    /// Convert several images into one PDF, one image per page.
    pub fn combine(&self, generation: u64, files: Vec<BatchInput>, options: Options) {
        self.cancel.store(false, Ordering::SeqCst);
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();

        let spawned = std::thread::Builder::new()
            .name("verso-combine".to_string())
            .spawn(move || {
                let started = Instant::now();
                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    combine_images(&files, &options)
                }));
                match outcome {
                    Ok(Ok(completed)) => emit(
                        &tx,
                        &ctx,
                        Event::Converted {
                            generation,
                            completed: Completed {
                                elapsed_ms: started.elapsed().as_millis(),
                                ..completed
                            },
                        },
                    ),
                    Ok(Err(message)) => emit(
                        &tx,
                        &ctx,
                        Event::Failed {
                            generation,
                            message,
                        },
                    ),
                    Err(_) => emit(
                        &tx,
                        &ctx,
                        Event::Failed {
                            generation,
                            message: "Verso hit an internal problem combining these images. No file was changed."
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
                    message: format!("Verso could not start the combiner: {error}"),
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
                            Ok(Ok((output, outputs, written, carried, elapsed_ms))) => {
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
                                    outputs,
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

fn convert_one(
    job: &ConvertJob,
    options: &Options,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Completed, String> {
    if !is_supported(job.source, job.target) {
        return Err(unsupported_message(job.source, job.target));
    }

    let options = with_stem(options, &job.input);
    let report = verso_core::convert_report_with_progress(
        job.source,
        job.target,
        &job.bytes,
        &options,
        progress,
    )
    .map_err(|error| error.to_string())?;

    let (outputs, primary) = write_outputs(&job.input, job.target, &report.files, &options)?;

    let proof = preview_of(job.target, report.bytes(), &options).ok();
    let (snippet, truncated) = proof
        .map(|preview| (preview.snippet, preview.truncated))
        .unwrap_or_default();

    Ok(Completed {
        output: primary,
        outputs,
        source_format: job.source,
        output_format: job.target,
        bytes: report.total_bytes(),
        pages: report.files.len(),
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
) -> Result<(PathBuf, Vec<PathBuf>, usize, Vec<String>, u128), String> {
    let started = Instant::now();
    let bytes = std::fs::read(&file.path).map_err(|error| read_message(&file.path, &error))?;
    let options = with_stem(options, &file.path);
    let report = convert_report(file.format, target, &bytes, &options)
        .map_err(|error| error.to_string())?;
    let (outputs, primary) = write_outputs(&file.path, target, &report.files, &options)?;
    Ok((
        primary,
        outputs,
        report.total_bytes(),
        report.warnings,
        started.elapsed().as_millis(),
    ))
}

fn combine_images(files: &[BatchInput], options: &Options) -> Result<Completed, String> {
    let Some(first) = files.first() else {
        return Err("No images to combine.".to_string());
    };
    let mut images = Vec::with_capacity(files.len());
    for file in files {
        if !file.format.is_image() {
            return Err(format!(
                "{} is not an image, so it cannot join the PDF.",
                state::file_name(&file.path)
            ));
        }
        let bytes = std::fs::read(&file.path).map_err(|error| read_message(&file.path, &error))?;
        images.push(ImageInput {
            format: file.format,
            bytes,
        });
    }

    let options = with_stem(options, &first.path);
    let (pdf, warnings) = images_to_pdf(&images, &options).map_err(|error| error.to_string())?;
    let output =
        paths::write_output(&first.path, Format::Pdf, &pdf).map_err(|error| error.to_string())?;

    Ok(Completed {
        output: output.clone(),
        outputs: vec![output],
        source_format: first.format,
        output_format: Format::Pdf,
        bytes: pdf.len(),
        pages: files.len(),
        warnings,
        snippet: format!(
            "{} image{} combined into one PDF, one per page.",
            files.len(),
            if files.len() == 1 { "" } else { "s" }
        ),
        truncated: false,
        elapsed_ms: 0,
    })
}

/// Extract the page images from a ZIP written by this app. Used by swap so it
/// works on the exact files from the last run.
pub fn extract_pages(zip_path: &Path) -> Result<Vec<PathBuf>, String> {
    let file = std::fs::File::open(zip_path)
        .map_err(|error| format!("Could not open {}: {error}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| format!("Could not read the ZIP: {error}"))?;
    let folder = unique_folder(zip_path, "images")?;
    let mut extracted = Vec::new();

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("Could not read a ZIP entry: {error}"))?;
        let Some(name) = entry
            .enclosed_name()
            .and_then(|path| path.file_name().map(|name| name.to_owned()))
        else {
            continue;
        };
        let path = folder.join(&name);
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| format!("Could not create {}: {error}", path.display()))?;
        std::io::copy(&mut entry, &mut output)
            .map_err(|error| format!("Could not extract {}: {error}", path.display()))?;
        extracted.push(path);
    }

    if extracted.is_empty() {
        return Err("The ZIP contained no page images.".to_string());
    }
    Ok(extracted)
}

/// A short, human description of the first stretch of a document.
fn preview_of(format: Format, bytes: &[u8], options: &Options) -> Result<Preview, String> {
    if format.is_image() {
        let info = image_info(bytes).map_err(|error| error.to_string())?;
        return Ok(Preview {
            summary: DocumentSummary {
                blocks: 1,
                images: 1,
                ..DocumentSummary::default()
            },
            snippet: format!("{} × {} {}", info.width, info.height, format.code()),
            truncated: false,
            image: Some(info),
        });
    }

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
        image: None,
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

/// Give the converter the source stem so generated names match the input.
fn with_stem(options: &Options, input: &Path) -> Options {
    let mut options = options.clone();
    options.output_stem = input
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_string);
    options
}

/// Write every produced file. One file keeps the existing `_1` naming; several
/// files land in a fresh `<stem>_pages` folder (or a ZIP) so nothing is overwritten.
fn write_outputs(
    input: &Path,
    target: Format,
    files: &[OutputFile],
    options: &Options,
) -> Result<(Vec<PathBuf>, PathBuf), String> {
    if files.is_empty() {
        return Err("The converter produced no files.".to_string());
    }
    if files.len() == 1 {
        let path = paths::write_output(input, target, &files[0].bytes)
            .map_err(|error| error.to_string())?;
        return Ok((vec![path.clone()], path));
    }

    if options.image.bundle_zip {
        let bytes = zip_files(files)?;
        let path = write_zip_sibling(input, &bytes)?;
        return Ok((vec![path.clone()], path));
    }

    let folder = unique_folder(input, "pages")?;
    let mut written = Vec::with_capacity(files.len());
    for file in files {
        let name = sanitize_name(&file.name);
        let path = folder.join(&name);
        std::fs::write(&path, &file.bytes)
            .map_err(|error| format!("Could not write {}: {error}", path.display()))?;
        written.push(path);
    }
    Ok((written, folder))
}

fn zip_files(files: &[OutputFile]) -> Result<Vec<u8>, String> {
    use std::io::Write as _;

    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let entry = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for file in files {
        writer
            .start_file(sanitize_name(&file.name), entry)
            .map_err(|error| format!("Could not add a page to the ZIP: {error}"))?;
        writer
            .write_all(&file.bytes)
            .map_err(|error| format!("Could not write the ZIP: {error}"))?;
    }
    let bytes = writer
        .finish()
        .map_err(|error| format!("Could not finish the ZIP: {error}"))?
        .into_inner();
    Ok(bytes)
}

fn write_zip_sibling(input: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
    use std::io::Write as _;

    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    let stem = input
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("output");
    for attempt in 0..10_000u32 {
        let name = if attempt == 0 {
            format!("{stem}_pages.zip")
        } else {
            format!("{stem}_pages_{attempt}.zip")
        };
        let path = parent.join(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                if let Err(error) = file.write_all(bytes).and_then(|()| file.flush()) {
                    drop(file);
                    let _ = std::fs::remove_file(&path);
                    return Err(format!("Could not write {}: {error}", path.display()));
                }
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!("Could not create {}: {error}", path.display()));
            }
        }
    }
    Err("Could not find a free ZIP name next to the source.".to_string())
}

fn unique_folder(input: &Path, suffix: &str) -> Result<PathBuf, String> {
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    let stem = input
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("output");
    for attempt in 0..10_000u32 {
        let name = if attempt == 0 {
            format!("{stem}_{suffix}")
        } else {
            format!("{stem}_{suffix}_{attempt}")
        };
        let path = parent.join(name);
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!("Could not create {}: {error}", path.display()));
            }
        }
    }
    Err("Could not find a free output folder name.".to_string())
}

fn sanitize_name(name: &str) -> String {
    let candidate = Path::new(name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("page");
    if candidate.trim().is_empty() {
        "page".to_string()
    } else {
        candidate.to_string()
    }
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
