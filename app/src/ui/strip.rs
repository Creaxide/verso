//! The transfer strip: the largest object on screen and the one signature move.
//!
//! Source frame, target frame, and the exposure route between them. While a
//! conversion runs the route carries light; when it finishes the route turns
//! sage and the proof is on the right leaf.

use eframe::egui::{self, Align2, Margin, Sense, Stroke, Vec2};

use crate::app::{Action, Job, VersoApp};
use crate::components;
use crate::state::SourceState;
use crate::theme::{self, RAIL_MUTED, RAIL_TEXT, STRIP};

pub fn show(app: &VersoApp, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    egui::Panel::top("transfer-strip")
        .exact_size(72.0)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            egui::Frame::new()
                .fill(STRIP)
                .inner_margin(Margin::same(0)),
        )
        .show(ui, |ui| {
            let rect = ui.max_rect();
            components::sprockets(ui.painter(), rect);

            let pad = 26.0;
            let center = rect.center();
            let frame_width = (rect.width() * 0.30).clamp(150.0, 290.0);
            let left_rect = egui::Rect::from_min_max(
                egui::pos2(rect.left() + pad, rect.top() + 8.0),
                egui::pos2(rect.left() + pad + frame_width, rect.bottom() - 8.0),
            );
            let right_rect = egui::Rect::from_min_max(
                egui::pos2(rect.right() - pad - frame_width, rect.top() + 8.0),
                egui::pos2(rect.right() - pad, rect.bottom() - 8.0),
            );

            source_frame(app, ui, left_rect, actions);

            let busy = app.is_busy();
            let pushing = matches!(app.job, Job::Converting { .. });
            let route_from = egui::pos2(left_rect.right() + 18.0, center.y);
            let route_to = egui::pos2(right_rect.left() - 18.0, center.y);
            let pair = app.target_pair();
            let blocked = pair.support == verso_core::Support::Unsupported;
            let done = app.completed.is_some() && !busy;

            let progress = if pushing {
                app.progress().or_else(|| {
                    let elapsed = app.job_elapsed().as_secs_f32();
                    Some(0.06 + (elapsed * 0.7).min(1.0) * 0.82)
                })
            } else {
                None
            };
            components::route(
                ui.painter(),
                route_from,
                route_to,
                &app.palette,
                progress,
                blocked,
                done,
            );

            if busy && app.progress().is_none() && pushing {
                // An indeterminate exposure: a short travelling dash instead
                // of a lying progress fraction.
                let t = (app.job_elapsed().as_secs_f32() * 0.45) % 1.0;
                let span = route_to.x - route_from.x;
                let head = route_from.x + span * t;
                let tail = (head - span * 0.18).max(route_from.x);
                ui.painter().line_segment(
                    [egui::pos2(tail, center.y), egui::pos2(head, center.y)],
                    Stroke::new(1.6, app.palette.amber),
                );
                ui.painter().circle_filled(
                    egui::pos2(head, center.y),
                    2.2,
                    app.palette.amber,
                );
            }

            if !app.source.is_empty() {
                let color = app.palette.support_color(pair.support);
                let label = if blocked {
                    "BLOCKED".to_string()
                } else {
                    pair.support.label().to_uppercase()
                };
                components::pill(
                    ui.painter(),
                    center,
                    &label,
                    if app.is_busy() { app.palette.amber } else { color },
                    STRIP,
                );
            }

            target_frame(app, ui, right_rect);

            if app.source.is_empty() && !app.is_busy() {
                let response = ui.interact(
                    left_rect,
                    egui::Id::unique("strip-source-empty"),
                    Sense::click(),
                );
                if response.clicked() {
                    actions.push(Action::OpenPicker);
                }
                response.on_hover_text("Choose a document");
            }

            // A single hairline closes the strip against the leaves.
            ui.painter().line_segment(
                [rect.left_bottom(), rect.right_bottom()],
                Stroke::new(1.0, egui::Color32::from_rgb(0x08, 0x07, 0x06)),
            );
        });
}

fn source_frame(
    app: &VersoApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    _actions: &mut Vec<Action>,
) {
    let painter = ui.painter();
    let x = rect.left() + 2.0;
    let label_y = rect.top() - 1.0;

    let (eyebrow, code, name) = match &app.source {
        SourceState::Empty => ("SOURCE", "—".to_string(), "Drop a document".to_string()),
        SourceState::Loading { path, .. } => (
            "SOURCE",
            "···".to_string(),
            crate::state::file_name(path),
        ),
        SourceState::File(file) => (
            "SOURCE",
            file.format.code().to_string(),
            file.display_name(),
        ),
        SourceState::Batch(batch) => {
            let mut counts: Vec<(verso_core::Format, usize)> = Vec::new();
            for file in &batch.files {
                if let Some(entry) = counts.iter_mut().find(|(format, _)| *format == file.format) {
                    entry.1 += 1;
                } else {
                    counts.push((file.format, 1));
                }
            }
            let summary = counts
                .iter()
                .map(|(format, count)| format!("{count} {}", format.code()))
                .collect::<Vec<_>>()
                .join(" · ");
            (
                "BATCH",
                format!("{}", batch.files.len()),
                format!("files · {summary}"),
            )
        }
    };

    painter.text(
        egui::pos2(x, label_y),
        Align2::LEFT_TOP,
        eyebrow,
        theme::medium(8.5),
        RAIL_MUTED,
    );
    painter.text(
        egui::pos2(x, rect.top() + 10.0),
        Align2::LEFT_TOP,
        code,
        theme::condensed_bold(26.0),
        RAIL_TEXT,
    );
    painter.text(
        egui::pos2(x, rect.bottom() - 1.0),
        Align2::LEFT_BOTTOM,
        components::ellipsize(&name, 46),
        theme::mono(10.0),
        RAIL_MUTED,
    );

    // A thin frame edge, not a film border: two corners only.
    let corner = 6.0;
    painter.line_segment(
        [rect.left_top(), rect.left_top() + Vec2::new(corner, 0.0)],
        Stroke::new(1.0, RAIL_MUTED.gamma_multiply(0.35)),
    );
    painter.line_segment(
        [rect.left_bottom(), rect.left_bottom() + Vec2::new(corner, 0.0)],
        Stroke::new(1.0, RAIL_MUTED.gamma_multiply(0.35)),
    );
}

fn target_frame(app: &VersoApp, ui: &mut egui::Ui, rect: egui::Rect) {
    let painter = ui.painter();
    let x = rect.left();
    let label_y = rect.top() - 1.0;

    painter.text(
        egui::pos2(x, label_y),
        Align2::LEFT_TOP,
        "TARGET",
        theme::medium(8.5),
        RAIL_MUTED,
    );
    painter.text(
        egui::pos2(x, rect.top() + 10.0),
        Align2::LEFT_TOP,
        app.target.code(),
        theme::condensed_bold(26.0),
        app.palette.amber,
    );

    let line = if let Some(completed) = &app.completed {
        completed.output_name()
    } else if let Some(name) = app.pending_output_name() {
        name
    } else if app.source.is_batch() {
        "written beside each source".to_string()
    } else if app.source.is_empty() {
        "awaiting a source".to_string()
    } else {
        format!("name.{}", app.target.extension())
    };
    painter.text(
        egui::pos2(x, rect.bottom() - 1.0),
        Align2::LEFT_BOTTOM,
        components::ellipsize(&line, 44),
        theme::mono(10.0),
        RAIL_MUTED,
    );

    // The target is the destination, so its corner marks close on the right.
    let corner = 6.0;
    painter.line_segment(
        [rect.right_top(), rect.right_top() - Vec2::new(corner, 0.0)],
        Stroke::new(1.0, RAIL_MUTED.gamma_multiply(0.35)),
    );
    painter.line_segment(
        [rect.right_bottom(), rect.right_bottom() - Vec2::new(corner, 0.0)],
        Stroke::new(1.0, RAIL_MUTED.gamma_multiply(0.35)),
    );

    // Hovering the target explains the current pair in one sentence.
    let response = ui.interact(rect, egui::Id::unique("strip-target"), Sense::hover());
    let pair = verso_core::pair_info(
        app.source_format().unwrap_or(verso_core::Format::Txt),
        app.target,
    );
    if app.source.is_empty() {
        response.on_hover_text(format!("Target: {}. Choose a source first.", app.target.name()));
    } else {
        response.on_hover_text(format!(
            "{} → {} · {}",
            pair.from.code(),
            pair.to.code(),
            pair.support.label()
        ));
    }

}
