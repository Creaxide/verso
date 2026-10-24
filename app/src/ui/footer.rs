//! The footer: what happened on the left, exposure in the middle, action right.

use eframe::egui::{
    self, Align, Layout, Margin, Rect, RichText, Sense, Stroke, UiBuilder, Vec2,
};

use crate::app::{Action, VersoApp};
use crate::components;
use crate::state::Notice;
use crate::theme;

pub fn show(app: &VersoApp, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    egui::Panel::bottom("footer")
        .exact_size(56.0)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            egui::Frame::new()
                .fill(app.palette.background)
                .inner_margin(Margin::same(0)),
        )
        .show(ui, |ui| {
            let rect = ui.max_rect();
            ui.painter().line_segment(
                [rect.left_top(), rect.right_top()],
                Stroke::new(1.0, app.palette.hairline),
            );

            let inset = 16.0;
            let reserved = 520.0;
            let status_width = (rect.width() - inset * 2.0 - reserved).max(140.0);
            status_block(app, ui, rect, inset, status_width);

            let exposure_rect = Rect::from_center_size(
                egui::pos2(rect.center().x, rect.top() + 30.0),
                Vec2::new(190.0, 44.0),
            );
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(exposure_rect)
                    .layout(Layout::top_down(Align::Center)),
                |ui| exposure(app, ui),
            );

            let controls_rect = Rect::from_min_max(
                egui::pos2(rect.right() - reserved + 40.0, rect.top()),
                egui::pos2(rect.right() - inset, rect.bottom()),
            );
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(controls_rect)
                    .layout(Layout::right_to_left(Align::Center)),
                |ui| controls(app, ui, actions),
            );
        });
}

fn status_block(
    app: &VersoApp,
    ui: &mut egui::Ui,
    rect: Rect,
    inset: f32,
    width: f32,
) {
    let left = rect.left() + inset;
    let color = status_color(app);
    let dot = egui::pos2(left + 3.0, rect.top() + 16.0);
    ui.painter().circle_filled(dot, 3.0, color);

    ui.painter().text(
        egui::pos2(left + 14.0, rect.top() + 11.0),
        egui::Align2::LEFT_TOP,
        components::ellipsize(&app.status_line(), status_chars(width)),
        theme::body(12.5),
        app.palette.text,
    );
    ui.painter().text(
        egui::pos2(left, rect.top() + 31.0),
        egui::Align2::LEFT_TOP,
        components::ellipsize(&hint_line(app), status_chars(width) + 24),
        theme::medium(9.5),
        app.palette.muted,
    );
}

fn controls(app: &VersoApp, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    let convert_label = convert_label(app);
    let enabled = app.can_convert();
    let response = components::primary_button(ui, &convert_label, enabled, &app.palette);
    if response.clicked() {
        actions.push(Action::Convert);
    }
    if let Some(reason) = app.convert_blocked_reason() {
        response
            .on_disabled_hover_text(reason)
            .on_hover_text("Ctrl+Enter");
    } else {
        response.on_hover_text("Ctrl+Enter");
    }

    ui.add_space(8.0);
    let cancel = components::secondary_button(ui, "CANCEL", app.is_busy(), &app.palette);
    if cancel.clicked() {
        actions.push(Action::Cancel);
    }
    if app.is_busy() {
        cancel.on_hover_text("Esc · the current file finishes first");
    } else {
        cancel.on_disabled_hover_text("Nothing is running.");
    }

    ui.add_space(8.0);
    let has_output = app.open_folder_path().is_some();
    let open = components::secondary_button(ui, "OPEN FOLDER", has_output, &app.palette);
    if open.clicked() {
        actions.push(Action::OpenFolder);
    }
    if !has_output {
        open.on_disabled_hover_text("A folder appears after the first conversion.");
    }
}

fn status_color(app: &VersoApp) -> egui::Color32 {
    if app.is_busy() {
        app.palette.amber
    } else {
        match &app.notice {
            Notice::Error(_) => app.palette.red,
            Notice::Success(_) => app.palette.sage,
            _ => app.palette.muted,
        }
    }
}

fn status_chars(width: f32) -> usize {
    ((width / 6.4).max(24.0)) as usize
}

/// The formats in a batch, condensed: `TXT · MD · HTML → PDF`.
fn batch_pair(app: &VersoApp) -> String {
    let Some(batch) = app.source.batch() else {
        return app.target.code().to_string();
    };
    let mut codes: Vec<&'static str> = Vec::new();
    for file in &batch.files {
        let code = file.format.code();
        if !codes.contains(&code) {
            codes.push(code);
        }
    }
    let from = match codes.len() {
        0 => "—".to_string(),
        1..=3 => codes.join(" · "),
        count => format!("{count} formats"),
    };
    format!("{from} → {}", app.target.code())
}

fn hint_line(app: &VersoApp) -> String {
    if app.is_busy() {
        let seconds = app.job_elapsed().as_secs_f32();
        return format!("{seconds:.1} s · Esc stops after the current file");
    }
    if let Some(summary) = &app.batch_summary {
        let mut line = format!(
            "{} · {}",
            batch_pair(app),
            crate::state::format_size(summary.bytes)
        );
        if summary.failed > 0 || summary.skipped > 0 {
            line.push_str(&format!(
                " · {} failed · {} skipped",
                summary.failed, summary.skipped
            ));
        }
        if summary.warnings > 0 {
            line.push_str(&format!(
                " · {} warning{}",
                summary.warnings,
                if summary.warnings == 1 { "" } else { "s" }
            ));
        }
        return line;
    }
    if let Some(batch) = app.source.batch() {
        let total = batch.files.len();
        let done = batch.done();
        let size: u64 = batch.files.iter().map(|file| file.size).sum();
        if done == total {
            return format!(
                "{} · {total} file{} · drop more to add",
                batch_pair(app),
                if total == 1 { "" } else { "s" }
            );
        }
        return format!("{} · {}", batch_pair(app), crate::state::format_size(size));
    }
    if let Some(completed) = &app.completed {
        let mut line = format!(
            "{} → {} · {}",
            completed.source_format.code(),
            completed.output_format.code(),
            crate::state::format_size(completed.bytes as u64)
        );
        if !completed.warnings.is_empty() {
            line.push_str(&format!(
                " · {} warning{}",
                completed.warnings.len(),
                if completed.warnings.len() == 1 { "" } else { "s" }
            ));
        }
        return line;
    }
    if let Some(from) = app.source_format() {
        let pair = verso_core::pair_info(from, app.target);
        return format!(
            "{} → {} · {} · Ctrl+Enter to convert",
            pair.from.code(),
            pair.to.code(),
            pair.support.label()
        );
    }
    "Ctrl+O opens files · drop several for a batch · Ctrl+M matrix".to_string()
}

fn convert_label(app: &VersoApp) -> String {
    if app.is_busy() {
        return "CONVERTING".to_string();
    }
    if let Some(batch) = app.source.batch() {
        let waiting = batch.waiting();
        if waiting == 0 {
            return "CONVERTED".to_string();
        }
        return format!("CONVERT {waiting}");
    }
    "CONVERT".to_string()
}

fn exposure(app: &VersoApp, ui: &mut egui::Ui) {
    let busy = app.is_busy();
    let label = if busy {
        app.busy_label().unwrap_or("WORKING")
    } else if app.completed.is_some() || app.batch_summary.is_some() {
        "WRITTEN"
    } else {
        "READY"
    };
    ui.label(
        RichText::new(label)
            .font(theme::condensed_bold(9.0))
            .color(if busy {
                app.palette.amber_ink
            } else if app.completed.is_some() || app.batch_summary.is_some() {
                app.palette.sage_ink
            } else {
                app.palette.muted
            })
            .extra_letter_spacing(1.2),
    );

    let (rect, _) = ui.allocate_exact_size(Vec2::new(180.0, 12.0), Sense::hover());
    let track_y = rect.center().y;
    let start = egui::pos2(rect.left(), track_y);
    let end = egui::pos2(rect.right(), track_y);
    ui.painter()
        .line_segment([start, end], Stroke::new(1.0, app.palette.hairline));

    if busy {
        if let Some(fraction) = app.progress() {
            let head = egui::pos2(start.x + (end.x - start.x) * fraction, track_y);
            ui.painter()
                .line_segment([start, head], Stroke::new(2.0, app.palette.amber));
            ui.painter().circle_filled(head, 2.2, app.palette.amber);
        } else {
            // Indeterminate: a short exposure mark sweeping the track.
            let t = (app.job_elapsed().as_secs_f32() * 0.7) % 1.0;
            let span = end.x - start.x;
            let head_x = start.x + span * t;
            let tail_x = (head_x - span * 0.28).max(start.x);
            ui.painter().line_segment(
                [egui::pos2(tail_x, track_y), egui::pos2(head_x, track_y)],
                Stroke::new(2.0, app.palette.amber),
            );
        }
    } else if app.completed.is_some() || app.batch_summary.is_some() {
        ui.painter()
            .line_segment([start, end], Stroke::new(2.0, app.palette.sage));
    }
}
