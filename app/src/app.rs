//! The application: state, actions and the frame loop.
//!
//! Panels are drawn from immutable state and collect [`Action`]s; only here do
//! those actions touch the worker, the file system or the clipboard of state.
//! That keeps every panel a pure function of what the window knows.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{self, Context, Key, KeyboardShortcut, Modifiers};
use verso_core::{is_supported, pair_info, Format, ImagePageSize, Options};

use crate::components;
use crate::{state, theme, ui, worker};
use crate::state::{
    fallback_target, format_size, Batch, BatchFile, BatchStatus, BatchSummary, Completed,
    Inspection, Notice, SourceFile, SourceState,
};
use crate::theme::{Palette, ThemeChoice};
use crate::worker::{BatchInput, ConvertJob, Event, Worker};

const OPEN_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const CONVERT_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter);
const SWAP_SHORTCUT: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S);
const MATRIX_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::M);

pub const SUPPORTED_EXTENSIONS: [&str; 16] = [
    "txt", "md", "markdown", "html", "htm", "docx", "pdf", "rtf", "json", "csv", "png", "jpg",
    "jpeg", "webp", "bmp", "gif",
];

/// Things a panel can ask the app to do.
#[derive(Debug)]
pub enum Action {
    OpenPicker,
    Clear,
    SetSourceFormat(Format),
    SetTarget(Format),
    Convert,
    Cancel,
    Swap,
    OpenOutput,
    OpenFolder,
    ToggleMatrix,
    CycleTheme,
    RemoveBatchFile(usize),
    SetImageQuality(u8),
    SetMaxDimension(Option<u32>),
    SetDpi(u16),
    SetBundleZip(bool),
    SetImagePageSize(ImagePageSize),
    SetCombineImages(bool),
    ConfirmLargeImage,
    DismissLargeImage,
    ConfirmSwapCombine,
    DismissSwapCombine,
    DismissPdfium,
}

pub(crate) enum Job {
    Idle,
    Loading {
        path: PathBuf,
        started: Instant,
    },
    Converting {
        started: Instant,
        batch: bool,
        total: usize,
        done: usize,
    },
}

impl Job {
    fn is_busy(&self) -> bool {
        !matches!(self, Job::Idle)
    }
}

pub struct VersoApp {
    pub(crate) source: SourceState,
    pub(crate) target: Format,
    pub(crate) completed: Option<Completed>,
    pub(crate) batch_summary: Option<BatchSummary>,
    pub(crate) notice: Notice,
    pub(crate) theme: ThemeChoice,
    pub(crate) dark: bool,
    pub(crate) palette: Palette,
    pub(crate) job: Job,
    pub(crate) options: Options,
    pub(crate) matrix_open: bool,
    pub(crate) hover_drop: bool,
    pub(crate) swap_flash: Option<Instant>,
    pub(crate) large_image_prompt: bool,
    pub(crate) page_progress: Option<(usize, usize)>,
    pub(crate) combine_images: bool,
    pub(crate) swap_combine_prompt: bool,
    pub(crate) pdfium_prompt: bool,
    large_image_ack: bool,

    worker: Worker,
    generation: u64,
    last_dir: Option<PathBuf>,
}

impl VersoApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        let dark = cc.egui_ctx.system_theme().map_or(true, |theme| {
            theme == egui::Theme::Dark
        });
        let palette = Palette::for_dark(dark);
        theme::apply(&cc.egui_ctx, dark, &palette);

        let mut app = Self {
            source: SourceState::Empty,
            target: Format::Markdown,
            completed: None,
            batch_summary: None,
            notice: Notice::Info(
                "Drop a document, or press Ctrl+O. Nothing leaves this machine.".to_string(),
            ),
            theme: ThemeChoice::Auto,
            dark,
            palette,
            job: Job::Idle,
            options: Options::default(),
            matrix_open: false,
            hover_drop: false,
            swap_flash: None,
            large_image_prompt: false,
            page_progress: None,
            combine_images: false,
            swap_combine_prompt: false,
            pdfium_prompt: false,
            large_image_ack: false,
            worker: Worker::new(cc.egui_ctx.clone()),
            generation: 0,
            last_dir: None,
        };

        // Opening from a file manager or a shell is a first-class way in.
        let paths: Vec<PathBuf> = std::env::args_os()
            .skip(1)
            .map(PathBuf::from)
            .filter(|path| path.is_file() || path.is_dir())
            .collect();
        if !paths.is_empty() {
            app.load_paths(paths);
        }

        app
    }

    // -----------------------------------------------------------------------
    // Queries used by the panels
    // -----------------------------------------------------------------------

    pub(crate) fn is_busy(&self) -> bool {
        self.job.is_busy()
    }

    pub(crate) fn busy_label(&self) -> Option<&'static str> {
        match self.job {
            Job::Idle => None,
            Job::Loading { .. } => Some("READING"),
            Job::Converting { batch: false, .. } => Some("EXPOSING"),
            Job::Converting { batch: true, .. } => Some("EXPOSING BATCH"),
        }
    }

    /// Known progress for batches and page renders; `None` means an
    /// indeterminate exposure.
    pub(crate) fn progress(&self) -> Option<f32> {
        if let Some((done, total)) = self.page_progress {
            if total > 0 {
                return Some(done as f32 / total as f32);
            }
        }
        match self.job {
            Job::Converting {
                batch: true,
                total,
                done,
                ..
            } if total > 0 => Some(done as f32 / total as f32),
            _ => None,
        }
    }

    pub(crate) fn job_elapsed(&self) -> Duration {
        match &self.job {
            Job::Loading { started, .. } | Job::Converting { started, .. } => {
                started.elapsed()
            }
            Job::Idle => Duration::ZERO,
        }
    }

    pub(crate) fn source_format(&self) -> Option<Format> {
        self.source.format()
    }

    /// Dimensions of a single image source that exceed the safe limits.
    pub(crate) fn source_large_image(&self) -> Option<verso_core::ImageInfo> {
        match &self.source {
            SourceState::File(file) => match &file.inspection {
                Inspection::Ready(preview) => preview.image.filter(|info| info.is_large()),
                _ => None,
            },
            _ => None,
        }
    }

    /// The target cannot be changed while a conversion is running.
    pub(crate) fn can_edit_pair(&self) -> bool {
        !self.is_busy()
    }

    pub(crate) fn can_convert(&self) -> bool {
        if self.is_busy() || self.source.is_empty() {
            return false;
        }
        if self.combine_images && self.target == Format::Pdf {
            if let SourceState::Batch(batch) = &self.source {
                if batch.files.iter().any(|file| !file.format.is_image()) {
                    return false;
                }
            }
        }
        match &self.source {
            SourceState::Batch(batch) => batch
                .files
                .iter()
                .any(|file| !file.status.is_done() && is_supported(file.format, self.target)),
            SourceState::File(_) => self
                .source_format()
                .is_some_and(|from| is_supported(from, self.target)),
            _ => false,
        }
    }

    pub(crate) fn convert_blocked_reason(&self) -> Option<String> {
        if self.is_busy() {
            return Some("A job is already running.".to_string());
        }
        if self.source.is_empty() {
            return Some("Drop a document first.".to_string());
        }
        if self.combine_images && self.target == Format::Pdf {
            if let SourceState::Batch(batch) = &self.source {
                if batch.files.iter().any(|file| !file.format.is_image()) {
                    return Some(
                        "Combining into one PDF needs images only. Remove the other files, or turn combining off."
                            .to_string(),
                    );
                }
            }
        }
        if let SourceState::Batch(batch) = &self.source {
            if !batch.files.is_empty() && batch.files.iter().all(|file| file.status.is_done()) {
                return Some(
                    "Every file in this batch is written. Drop more files, or pick another target to start over."
                        .to_string(),
                );
            }
        }
        if !self.can_convert() {
            if let SourceState::Batch(batch) = &self.source {
                if batch.done() > 0 {
                    return Some(
                        "The remaining files cannot become this format. Pick another target, or add other files."
                            .to_string(),
                    );
                }
            }
            if let Some(from) = self.source_format() {
                return Some(worker::pair_refusal(from, self.target));
            }
            return Some("None of these files can become this format.".to_string());
        }
        None
    }

    /// Whether the proof can be turned back into a source.
    pub(crate) fn swap_state(&self) -> (bool, String) {
        match &self.completed {
            Some(completed) => {
                let from = completed.output_format;
                let to = completed.source_format;
                let pair = pair_info(from, to);
                if pair.is_possible() && pair.reversible {
                    if self.is_busy() {
                        (false, "A job is already running.".to_string())
                    } else {
                        (true, String::new())
                    }
                } else {
                    (
                        false,
                        format!(
                            "Cannot swap: {} → {} is not reversible. {}",
                            from.name(),
                            to.name(),
                            pair.notes.first().copied().unwrap_or("")
                        ),
                    )
                }
            }
            None if self.is_busy() => (false, "A job is already running.".to_string()),
            None => (
                false,
                "Convert once first: swap turns the proof back into the source.".to_string(),
            ),
        }
    }

    pub(crate) fn pending_output_name(&self) -> Option<String> {
        let input = self.source.primary_path()?;
        if self.source.is_batch() {
            return None;
        }
        let from = self.source_format().unwrap_or(Format::Txt);
        let stem = input
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("output");
        if pair_info(from, self.target).multi_file_output {
            if self.options.image.bundle_zip {
                return Some(format!("{stem}_pages.zip"));
            }
            return Some(format!(
                "{stem}_pages/{stem}_page-001.{}",
                self.target.extension()
            ));
        }
        let path = verso_core::paths::unique_output(input, self.target);
        Some(state::file_name(&path))
    }

    pub(crate) fn target_pair(&self) -> verso_core::Pair {
        pair_info(self.source_format().unwrap_or(Format::Txt), self.target)
    }

    /// What the footer says when nothing is wrong.
    pub(crate) fn status_line(&self) -> String {
        if self.is_busy() {
            return match &self.job {
                Job::Loading { path, .. } => {
                    format!("Reading {}…", state::file_name(path))
                }
                Job::Converting { batch: true, done, total, .. } => {
                    format!("Exposing {done} of {total} files…")
                }
                Job::Converting { .. } => "Exposing the proof…".to_string(),
                Job::Idle => String::new(),
            };
        }
        self.notice.text().to_string()
    }

    pub(crate) fn open_folder_path(&self) -> Option<PathBuf> {
        self.completed
            .as_ref()
            .map(Completed::folder)
            .or_else(|| {
                self.batch_summary
                    .as_ref()
                    .and_then(|summary| summary.folder.clone())
            })
    }

    // -----------------------------------------------------------------------
    // Frame
    // -----------------------------------------------------------------------

    fn sync_theme(&mut self, ctx: &Context) {
        let system_dark = ctx.system_theme().map_or(self.dark, |theme| {
            theme == egui::Theme::Dark
        });
        let dark = match self.theme {
            ThemeChoice::Auto => system_dark,
            ThemeChoice::Dark => true,
            ThemeChoice::Light => false,
        };
        if dark != self.dark {
            self.dark = dark;
            self.palette = Palette::for_dark(dark);
            theme::apply(ctx, dark, &self.palette);
        }
    }

    fn process_events(&mut self) {
        for event in self.worker.drain() {
            match event {
                Event::Loaded {
                    generation,
                    path,
                    bytes,
                    format,
                    preview,
                } => {
                    if generation != self.generation {
                        continue;
                    }
                    let size = bytes.len() as u64;
                    let name = state::file_name(&path);
                    let inspection = match preview {
                        Ok(preview) => Inspection::Ready(preview),
                        Err(message) => Inspection::Failed(message),
                    };
                    self.source = SourceState::File(SourceFile {
                        path: path.clone(),
                        bytes,
                        format,
                        size,
                        inspection,
                    });
                    self.last_dir = path.parent().map(Path::to_path_buf);
                    self.completed = None;
                    self.batch_summary = None;
                    self.job = Job::Idle;
                    self.ensure_target(format);
                    self.notice = match &self.source {
                        SourceState::File(file) => match &file.inspection {
                            Inspection::Ready(preview) if preview.image.is_some() => {
                                let info = preview.image.unwrap_or_default();
                                Notice::Info(format!(
                                    "{} read · {} × {} px · target {}.",
                                    name,
                                    info.width,
                                    info.height,
                                    self.target.code()
                                ))
                            }
                            Inspection::Ready(preview) => Notice::Info(format!(
                                "{} read · {} block{} · {} word{} · target {}.",
                                name,
                                preview.summary.blocks,
                                if preview.summary.blocks == 1 { "" } else { "s" },
                                preview.summary.words,
                                if preview.summary.words == 1 { "" } else { "s" },
                                self.target.code()
                            )),
                            _ => Notice::Info(format!("{name} read · target {}.", self.target.code())),
                        },
                        _ => Notice::Info(format!("{name} read.")),
                    };
                }
                Event::PageProgress {
                    generation,
                    done,
                    total,
                } => {
                    if generation != self.generation {
                        continue;
                    }
                    self.page_progress = Some((done, total));
                }
                Event::Converted {
                    generation,
                    completed,
                } => {
                    if generation != self.generation {
                        continue;
                    }
                    self.job = Job::Idle;
                    self.page_progress = None;
                    self.swap_flash = Some(Instant::now());
                    if self.combine_images && self.source.is_batch() {
                        let mut done = 0usize;
                        if let SourceState::Batch(batch) = &mut self.source {
                            for file in &mut batch.files {
                                if !file.status.is_done() {
                                    file.status = BatchStatus::Done {
                                        output: completed.output.clone(),
                                        outputs: completed.outputs.clone(),
                                        bytes: completed.bytes,
                                        warnings: completed.warnings.clone(),
                                        elapsed_ms: completed.elapsed_ms,
                                    };
                                    done += 1;
                                }
                            }
                        }
                        self.batch_summary = Some(BatchSummary {
                            done,
                            failed: 0,
                            skipped: 0,
                            bytes: completed.bytes as u64,
                            warnings: completed.warnings.len(),
                            elapsed_ms: completed.elapsed_ms,
                            folder: completed
                                .output
                                .parent()
                                .map(std::path::Path::to_path_buf),
                        });
                    }
                    self.notice = Notice::Success(format!(
                        "Wrote {} · {} in {} ms.",
                        state::file_name(&completed.output),
                        format_size(completed.bytes as u64),
                        completed.elapsed_ms
                    ));
                    self.completed = Some(completed);
                }
                Event::Failed {
                    generation,
                    message,
                } => {
                    if generation != self.generation {
                        continue;
                    }
                    self.job = Job::Idle;
                    self.page_progress = None;
                    if self.source.is_loading() {
                        self.source = SourceState::Empty;
                    }
                    if let SourceState::Batch(batch) = &mut self.source {
                        batch.reset_running();
                    }
                    if message.contains("PDFium") {
                        self.pdfium_prompt = true;
                    }
                    self.notice = Notice::Error(message);
                }
                Event::BatchFile {
                    generation,
                    index,
                    status,
                } => {
                    if generation != self.generation {
                        continue;
                    }
                    if let SourceState::Batch(batch) = &mut self.source {
                        if let Some(file) = batch.files.get_mut(index) {
                            file.status = status;
                        }
                        if let Job::Converting { done, .. } = &mut self.job {
                            *done = batch
                                .files
                                .iter()
                                .filter(|file| !matches!(file.status, BatchStatus::Waiting | BatchStatus::Running))
                                .count();
                        }
                    }
                }
                Event::BatchDone {
                    generation,
                    summary,
                    cancelled,
                } => {
                    if generation != self.generation {
                        continue;
                    }
                    self.job = Job::Idle;
                    self.page_progress = None;
                    if let SourceState::Batch(batch) = &mut self.source {
                        batch.reset_running();
                    }
                    if cancelled {
                        self.notice = Notice::Info(format!(
                            "Stopped. {} file{} written before the stop.",
                            summary.done,
                            if summary.done == 1 { "" } else { "s" }
                        ));
                    } else if summary.failed > 0 || summary.skipped > 0 {
                        self.notice = Notice::Error(format!(
                            "{} written, {} failed, {} skipped.",
                            summary.done, summary.failed, summary.skipped
                        ));
                    } else {
                        self.notice = Notice::Success(format!(
                            "{} file{} written · {} in {} ms.",
                            summary.done,
                            if summary.done == 1 { "" } else { "s" },
                            format_size(summary.bytes),
                            summary.elapsed_ms
                        ));
                    }
                    self.batch_summary = Some(summary);
                }
            }
        }
    }

    fn ensure_target(&mut self, source: Format) {
        if is_supported(source, self.target) {
            return;
        }
        let fallback = fallback_target(source);
        self.target = fallback;
        self.notice = Notice::Info(format!(
            "{} can’t become {}. Target set to {}.",
            source.code(),
            self.target.code(),
            fallback.name()
        ));
    }

    fn handle_shortcuts(&mut self, ctx: &Context) {
        let mut actions = Vec::new();
        if ctx.input_mut(|i| i.consume_shortcut(&OPEN_SHORTCUT)) {
            actions.push(Action::OpenPicker);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&CONVERT_SHORTCUT)) {
            actions.push(Action::Convert);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&SWAP_SHORTCUT)) {
            actions.push(Action::Swap);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&MATRIX_SHORTCUT)) {
            actions.push(Action::ToggleMatrix);
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            if self.matrix_open {
                self.matrix_open = false;
            } else if self.is_busy() {
                actions.push(Action::Cancel);
            }
        }
        self.handle_actions(actions, ctx, false);
    }

    fn handle_actions(&mut self, actions: Vec<Action>, ctx: &Context, from_panels: bool) {
        for action in actions {
            match action {
                Action::OpenPicker => self.pick_files(),
                Action::Clear => self.clear(),
                Action::SetSourceFormat(format) => self.set_source_format(format),
                Action::SetTarget(format) => self.set_target(format, from_panels),
                Action::Convert => self.start_convert(),
                Action::Cancel => self.cancel(),
                Action::Swap => self.swap(),
                Action::OpenOutput => self.open_output(false),
                Action::OpenFolder => self.open_output(true),
                Action::ToggleMatrix => self.matrix_open = !self.matrix_open,
                Action::CycleTheme => self.cycle_theme(ctx),
                Action::RemoveBatchFile(index) => self.remove_batch_file(index),
                Action::SetImageQuality(quality) => {
                    self.options.image.quality = quality.clamp(1, 100);
                    self.clear_completed();
                }
                Action::SetMaxDimension(max) => {
                    self.options.image.max_dimension = max;
                    self.clear_completed();
                }
                Action::SetDpi(dpi) => {
                    self.options.image.dpi = dpi.clamp(72, 300);
                    self.clear_completed();
                }
                Action::SetBundleZip(bundle) => {
                    self.options.image.bundle_zip = bundle;
                    self.clear_completed();
                }
                Action::SetImagePageSize(size) => {
                    self.options.image.page_size = size;
                    self.clear_completed();
                }
                Action::SetCombineImages(combine) => {
                    self.combine_images = combine;
                    self.clear_completed();
                }
                Action::ConfirmLargeImage => {
                    self.large_image_ack = true;
                    self.large_image_prompt = false;
                    self.start_convert();
                }
                Action::DismissLargeImage => {
                    self.large_image_prompt = false;
                    self.notice = Notice::Info("Conversion cancelled.".to_string());
                }
                Action::ConfirmSwapCombine => self.complete_swap_combine(),
                Action::DismissSwapCombine => {
                    self.swap_combine_prompt = false;
                    self.notice = Notice::Info("Swap cancelled.".to_string());
                }
                Action::DismissPdfium => self.pdfium_prompt = false,
            }
        }
    }

    // -----------------------------------------------------------------------
    // Commands
    // -----------------------------------------------------------------------

    fn pick_files(&mut self) {
        if self.is_busy() {
            return;
        }
        let mut dialog = rfd::FileDialog::new()
            .set_title("Choose a document")
            .add_filter("Documents", &SUPPORTED_EXTENSIONS);
        if let Some(directory) = &self.last_dir {
            dialog = dialog.set_directory(directory);
        }
        if let Some(paths) = dialog.pick_files() {
            self.load_paths(paths);
        }
    }

    fn load_paths(&mut self, paths: Vec<PathBuf>) {
        if self.is_busy() {
            self.notice = Notice::Info("A job is still running. Cancel it before loading more.".to_string());
            return;
        }
        if paths.is_empty() {
            return;
        }

        let files = expand_paths(&paths);
        if files.is_empty() {
            self.notice = Notice::Error(
                "No supported document was found there. Try TXT, MD, HTML, DOCX, PDF, RTF, JSON or CSV."
                    .to_string(),
            );
            return;
        }

        // Dropping onto an existing batch adds to it rather than replacing it.
        if self.source.is_batch() && files.iter().all(|path| path.is_file()) {
            self.append_to_batch(files);
            return;
        }

        if files.len() == 1 {
            self.load_single(files[0].clone());
        } else {
            self.start_batch(files);
        }
    }

    fn load_single(&mut self, path: PathBuf) {
        let hint = Format::extension_of(&path).and_then(|ext| Format::from_extension(&ext));
        self.clear_completed();
        self.large_image_ack = false;
        self.large_image_prompt = false;
        self.combine_images = false;
        self.swap_combine_prompt = false;
        self.source = SourceState::Loading {
            path: path.clone(),
            format: hint,
        };
        self.generation += 1;
        self.job = Job::Loading {
            path: path.clone(),
            started: Instant::now(),
        };
        self.notice = Notice::Info(format!("Reading {}…", state::file_name(&path)));
        self.worker
            .load(self.generation, path, None, self.options.clone());
    }

    fn start_batch(&mut self, paths: Vec<PathBuf>) {
        let mut batch = Batch::new(Vec::new());
        for path in paths {
            if batch.files.iter().any(|file| file.path == path) {
                continue;
            }
            let format = detect_format(&path);
            let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
            batch.files.push(BatchFile {
                path,
                format,
                size,
                status: BatchStatus::Waiting,
            });
        }
        if batch.files.is_empty() {
            self.notice = Notice::Error("Those files are already in the batch.".to_string());
            return;
        }
        let first = batch.files[0].format;
        self.clear_completed();
        self.large_image_ack = false;
        self.large_image_prompt = false;
        self.combine_images = false;
        self.swap_combine_prompt = false;
        self.source = SourceState::Batch(batch);
        self.generation += 1;
        self.job = Job::Idle;
        self.ensure_target(first);
        self.notice = Notice::Info(format!(
            "{} files queued. Each output is written next to its source.",
            self.source.count()
        ));
    }

    fn append_to_batch(&mut self, paths: Vec<PathBuf>) {
        let mut added = 0usize;
        if let SourceState::Batch(batch) = &mut self.source {
            for path in paths {
                if batch.files.iter().any(|file| file.path == path) {
                    continue;
                }
                let format = detect_format(&path);
                let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
                batch.files.push(BatchFile {
                    path,
                    format,
                    size,
                    status: BatchStatus::Waiting,
                });
                added += 1;
            }
        }
        self.clear_completed();
        self.batch_summary = None;
        if added == 0 {
            self.notice = Notice::Info("Those files are already in the batch.".to_string());
        } else {
            self.notice = Notice::Info(format!(
                "Added {added}. {} files in the batch.",
                self.source.count()
            ));
        }
    }

    fn remove_batch_file(&mut self, index: usize) {
        if self.is_busy() {
            return;
        }
        let empty = if let SourceState::Batch(batch) = &mut self.source {
            if index < batch.files.len() {
                batch.files.remove(index);
            }
            batch.files.is_empty()
        } else {
            false
        };
        if empty {
            self.source = SourceState::Empty;
        }
        self.clear_completed();
        self.batch_summary = None;
        if !self.notice.is_error() {
            self.notice = Notice::Info("Removed from the batch.".to_string());
        }
    }

    fn clear(&mut self) {
        if self.is_busy() {
            self.worker.cancel();
        }
        self.generation += 1;
        self.source = SourceState::Empty;
        self.job = Job::Idle;
        self.clear_completed();
        self.batch_summary = None;
        self.large_image_ack = false;
        self.large_image_prompt = false;
        self.combine_images = false;
        self.swap_combine_prompt = false;
        self.page_progress = None;
        self.notice = Notice::Info(
            "Drop a document, or press Ctrl+O. Nothing leaves this machine.".to_string(),
        );
    }

    fn clear_completed(&mut self) {
        self.completed = None;
    }

    fn set_source_format(&mut self, format: Format) {
        let path = match &self.source {
            SourceState::File(file) if file.format != format => file.path.clone(),
            _ => return,
        };
        self.clear_completed();
        self.source = SourceState::Loading {
            path: path.clone(),
            format: Some(format),
        };
        self.generation += 1;
        self.job = Job::Loading {
            path: path.clone(),
            started: Instant::now(),
        };
        self.worker
            .load(self.generation, path, Some(format), self.options.clone());
        self.notice = Notice::Info(format!("Reading as {}…", format.name()));
    }

    fn set_target(&mut self, target: Format, from_panels: bool) {
        if self.target == target {
            return;
        }
        if !self.can_edit_pair() {
            return;
        }
        if let Some(from) = self.source_format() {
            if !is_supported(from, target) {
                self.notice = Notice::Error(worker::pair_refusal(from, target));
                return;
            }
        }
        self.target = target;
        if target != Format::Pdf {
            self.combine_images = false;
        }
        self.clear_completed();
        self.batch_summary = None;
        if let SourceState::Batch(batch) = &mut self.source {
            batch.reset();
        }
        if from_panels {
            self.notice = Notice::Info(format!("Target set to {}.", target.name()));
        }
    }

    fn start_convert(&mut self) {
        if !self.can_convert() {
            if let Some(reason) = self.convert_blocked_reason() {
                if !self.source.is_empty() {
                    self.notice = Notice::Error(reason);
                }
            }
            return;
        }

        // Large images get an explicit confirmation before they are decoded.
        if !self.large_image_ack && self.source_large_image().is_some() {
            self.large_image_prompt = true;
            return;
        }

        if self.combine_images && self.target == Format::Pdf {
            if let SourceState::Batch(batch) = &self.source {
                let inputs: Vec<BatchInput> = batch
                    .files
                    .iter()
                    .filter(|file| !file.status.is_done())
                    .map(|file| BatchInput {
                        path: file.path.clone(),
                        format: file.format,
                    })
                    .collect();
                if inputs.is_empty() {
                    self.notice =
                        Notice::Info("Every image in this batch is already written.".to_string());
                    return;
                }
                let total = inputs.len();
                if let SourceState::Batch(batch) = &mut self.source {
                    for file in &mut batch.files {
                        if !file.status.is_done() {
                            file.status = BatchStatus::Waiting;
                        }
                    }
                }
                self.batch_summary = None;
                self.page_progress = None;
                self.generation += 1;
                self.job = Job::Converting {
                    started: Instant::now(),
                    batch: true,
                    total,
                    done: 0,
                };
                self.notice = Notice::Info(format!(
                    "Combining {total} image{} into one PDF…",
                    if total == 1 { "" } else { "s" }
                ));
                self.worker
                    .combine(self.generation, inputs, self.options.clone());
                return;
            }
        }

        match &self.source {
            SourceState::File(file) => {
                let job = ConvertJob {
                    input: file.path.clone(),
                    bytes: file.bytes.clone(),
                    source: file.format,
                    target: self.target,
                };
                self.generation += 1;
                self.page_progress = None;
                self.job = Job::Converting {
                    started: Instant::now(),
                    batch: false,
                    total: 1,
                    done: 0,
                };
                self.notice = Notice::Info(format!(
                    "Exposing {} → {}…",
                    file.format.code(),
                    self.target.code()
                ));
                self.worker
                    .convert(self.generation, job, self.options.clone());
            }
            SourceState::Batch(batch) => {
                let inputs: Vec<BatchInput> = batch
                    .files
                    .iter()
                    .filter(|file| !file.status.is_done())
                    .map(|file| BatchInput {
                        path: file.path.clone(),
                        format: file.format,
                    })
                    .collect();
                if inputs.is_empty() {
                    self.notice = Notice::Info("Every file in this batch is already done.".to_string());
                    return;
                }
                let total = inputs.len();
                if let SourceState::Batch(batch) = &mut self.source {
                    for file in &mut batch.files {
                        if !file.status.is_done() {
                            file.status = BatchStatus::Waiting;
                        }
                    }
                }
                self.batch_summary = None;
                self.generation += 1;
                self.page_progress = None;
                self.job = Job::Converting {
                    started: Instant::now(),
                    batch: true,
                    total,
                    done: 0,
                };
                self.notice = Notice::Info(format!(
                    "Exposing {total} file{} to {}…",
                    if total == 1 { "" } else { "s" },
                    self.target.name()
                ));
                self.worker.batch(
                    self.generation,
                    inputs,
                    self.target,
                    self.options.clone(),
                );
            }
            _ => {}
        }
    }

    fn cancel(&mut self) {
        if !self.is_busy() {
            return;
        }
        self.worker.cancel();
        self.notice = Notice::Info("Stopping after the current file…".to_string());
        // A single in-memory conversion cannot be interrupted safely; the app
        // stays busy until its result arrives. A batch stops between files.
        if let Job::Loading { .. } = self.job {
            self.job = Job::Idle;
            self.source = SourceState::Empty;
        }
    }

    fn swap(&mut self) {
        let (enabled, reason) = self.swap_state();
        if !enabled {
            self.notice = Notice::Error(reason);
            return;
        }
        let Some(completed) = self.completed.take() else {
            return;
        };
        let multi = pair_info(completed.source_format, completed.output_format).multi_file_output;
        if multi {
            self.completed = Some(completed);
            self.swap_combine_prompt = true;
            self.notice =
                Notice::Info("Confirm combining the page images into one PDF.".to_string());
            return;
        }
        let new_target = completed.source_format;
        let format = completed.output_format;
        self.target = new_target;
        self.batch_summary = None;
        self.large_image_ack = false;
        self.large_image_prompt = false;
        self.source = SourceState::Loading {
            path: completed.output.clone(),
            format: Some(format),
        };
        self.generation += 1;
        self.job = Job::Loading {
            path: completed.output.clone(),
            started: Instant::now(),
        };
        self.notice = Notice::Info(format!(
            "Turned the proof: {} → {}. Reverse pair: {}.",
            state::file_name(&completed.output),
            new_target.name(),
            if is_supported(format, new_target) {
                "supported"
            } else {
                "not supported"
            }
        ));
        self.worker.load(
            self.generation,
            completed.output,
            Some(format),
            self.options.clone(),
        );
    }

    /// Turn a multi-file proof into a batch of images targeting one PDF.
    fn complete_swap_combine(&mut self) {
        self.swap_combine_prompt = false;
        let Some(completed) = self.completed.take() else {
            return;
        };
        let target = completed.source_format;
        if !pair_info(completed.output_format, target).reversible {
            self.completed = Some(completed);
            self.notice = Notice::Error("That proof cannot be reversed.".to_string());
            return;
        }
        let outputs = match gather_output_paths(&completed) {
            Ok(paths) => paths,
            Err(message) => {
                self.completed = Some(completed);
                self.notice = Notice::Error(message);
                return;
            }
        };

        let mut batch = Batch::new(Vec::new());
        for path in outputs {
            let format = detect_format(&path);
            if !format.is_image() {
                continue;
            }
            let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
            batch.files.push(BatchFile {
                path,
                format,
                size,
                status: BatchStatus::Waiting,
            });
        }
        if batch.files.is_empty() {
            self.completed = Some(completed);
            self.notice =
                Notice::Error("No images were found in the last output set.".to_string());
            return;
        }

        let count = batch.files.len();
        self.source = SourceState::Batch(batch);
        self.target = target;
        self.combine_images = true;
        self.batch_summary = None;
        self.clear_completed();
        self.generation += 1;
        self.job = Job::Idle;
        self.page_progress = None;
        self.notice = Notice::Info(format!(
            "{count} image{} gathered. Press Convert to combine them into one PDF.",
            if count == 1 { "" } else { "s" }
        ));
    }

    fn cycle_theme(&mut self, ctx: &Context) {        self.theme = self.theme.next();
        let dark = match self.theme {
            ThemeChoice::Auto => ctx
                .system_theme()
                .map_or(self.dark, |theme| theme == egui::Theme::Dark),
            ThemeChoice::Dark => true,
            ThemeChoice::Light => false,
        };
        self.dark = dark;
        self.palette = Palette::for_dark(dark);
        theme::apply(ctx, dark, &self.palette);
        self.notice = Notice::Info(format!(
            "Appearance: {}.",
            match self.theme {
                ThemeChoice::Auto => "auto (follows the system)",
                ThemeChoice::Dark => "film dark",
                ThemeChoice::Light => "light table",
            }
        ));
    }

    fn open_output(&mut self, folder_only: bool) {
        let file = match (&self.completed, &self.batch_summary) {
            (Some(completed), _) if !folder_only => Some(completed.output.clone()),
            (Some(completed), _) => Some(completed.folder()),
            (None, Some(summary)) => summary.folder.clone(),
            _ => None,
        };
        let Some(path) = file else {
            self.notice = Notice::Info("There is no output to open yet.".to_string());
            return;
        };
        if let Err(message) = open_in_file_manager(&path, folder_only) {
            self.notice = Notice::Error(message);
        }
    }

    // -----------------------------------------------------------------------
    // Drag and drop
    // -----------------------------------------------------------------------

    fn dropped_paths(&mut self, ctx: &Context) {
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .filter(|path| !path.as_os_str().is_empty())
                .collect()
        });
        self.hover_drop = ctx.input(|input| !input.raw.hovered_files.is_empty());
        if !dropped.is_empty() {
            self.load_paths(dropped);
        }
    }

    fn paint_drop_overlay(&self, ui: &egui::Ui) {
        if !self.hover_drop {
            return;
        }
        let ctx = ui.ctx();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::unique("verso-drop"),
        ));
        let rect = ctx
            .content_rect()
            .shrink2(egui::vec2(16.0, 16.0));
        painter.rect_filled(
            rect,
            egui::CornerRadius::same(6),
            self.palette.tint(self.palette.amber, 0.10),
        );
        components::dashed_rect(ui, rect, self.palette.amber, 7.0, 5.0);
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "RELEASE TO LOAD",
            theme::condensed_bold(13.0),
            self.palette.amber_ink,
        );
    }
}

impl eframe::App for VersoApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.sync_theme(&ctx);
        self.process_events();
        self.dropped_paths(&ctx);
        self.handle_shortcuts(&ctx);

        let mut actions = Vec::new();
        ui::rail::show(self, ui, &mut actions);
        ui::strip::show(self, ui, &mut actions);
        ui::footer::show(self, ui, &mut actions);
        ui::source::show(self, ui, &mut actions);
        ui::fold::show(self, ui, &mut actions);
        ui::target::show(self, ui, &mut actions);
        ui::confirm::show(self, &ctx, &mut actions);
        ui::confirm::swap_prompt(self, &ctx, &mut actions);
        ui::confirm::pdfium_prompt(self, &ctx, &mut actions);
        ui::matrix::show(self, &ctx, &mut actions);

        self.paint_drop_overlay(ui);
        self.handle_actions(actions, &ctx, true);

        if self.is_busy() {
            ctx.request_repaint_after(Duration::from_millis(70));
        }
        // Keep the swap flash breathing for a few frames.
        if self
            .swap_flash
            .is_some_and(|at| at.elapsed() < Duration::from_millis(700))
        {
            ctx.request_repaint_after(Duration::from_millis(30));
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.palette.background.to_normalized_gamma_f32()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn expand_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for path in paths {
        if path.is_file() {
            out.push(path.clone());
        } else if path.is_dir() {
            let Ok(entries) = std::fs::read_dir(path) else {
                continue;
            };
            let mut found: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|candidate| candidate.is_file() && is_supported_file(candidate))
                .collect();
            found.sort();
            out.extend(found);
        }
    }
    out
}

fn is_supported_file(path: &Path) -> bool {
    Format::extension_of(path)
        .and_then(|extension| Format::from_extension(&extension))
        .is_some()
}

/// The exact files from the last conversion, never a folder scan for anything
/// else: multi-file outputs, a ZIP, a folder, or the single file.
fn gather_output_paths(completed: &state::Completed) -> Result<Vec<PathBuf>, String> {
    if completed.outputs.len() > 1 {
        return Ok(completed.outputs.clone());
    }
    let Some(path) = completed
        .outputs
        .first()
        .cloned()
        .or_else(|| Some(completed.output.clone()))
    else {
        return Ok(Vec::new());
    };

    if path.is_dir() {
        let mut found: Vec<PathBuf> = std::fs::read_dir(&path)
            .map_err(|error| format!("Could not read {}: {error}", path.display()))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|candidate| {
                candidate.is_file()
                    && Format::extension_of(candidate)
                        .and_then(|extension| Format::from_extension(&extension))
                        .is_some_and(Format::is_image)
            })
            .collect();
        found.sort();
        return Ok(found);
    }

    if Format::extension_of(&path).as_deref() == Some("zip") {
        return worker::extract_pages(&path);
    }

    Ok(vec![path])
}

/// Extension first, then a small content sniff for extensionless files.
fn detect_format(path: &Path) -> Format {
    if let Some(format) = Format::extension_of(path).and_then(|ext| Format::from_extension(&ext)) {
        return format;
    }
    use std::io::Read;
    let mut head = [0u8; 4096];
    if let Ok(mut file) = std::fs::File::open(path) {
        if let Ok(read) = file.read(&mut head) {
            return Format::sniff(&head[..read]);
        }
    }
    Format::Txt
}

#[cfg(windows)]
fn open_in_file_manager(path: &Path, folder_only: bool) -> Result<(), String> {
    use std::process::Command;
    let mut command = Command::new("explorer");
    if folder_only || path.is_dir() {
        command.arg(path);
    } else {
        command.arg(format!("/select,{}", path.display()));
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open the file manager: {error}"))
}

#[cfg(target_os = "macos")]
fn open_in_file_manager(path: &Path, folder_only: bool) -> Result<(), String> {
    use std::process::Command;
    let mut command = Command::new("open");
    if !folder_only && path.is_file() {
        command.arg("-R");
    }
    command
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open the file manager: {error}"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_in_file_manager(path: &Path, _folder_only: bool) -> Result<(), String> {
    use std::process::Command;
    let target = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().map(Path::to_path_buf).unwrap_or_else(|| path.to_path_buf())
    };
    Command::new("xdg-open")
        .arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open the file manager: {error}"))
}
