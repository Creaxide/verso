//! The source leaf: one document, or a numbered batch of them.

use eframe::egui::{
    self, Align, Align2, ComboBox, CornerRadius, Layout, Margin, RichText, ScrollArea, Sense,
    Stroke, StrokeKind, Vec2,
};

use crate::app::{Action, VersoApp};
use crate::components;
use crate::state::{
    format_size, shorten_path, Batch, BatchStatus, Inspection, SourceState,
};
use crate::theme;

pub fn show(app: &VersoApp, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    let width = ((ui.available_width() - 48.0) / 2.0).max(320.0);
    egui::Panel::left("source-leaf")
        .exact_size(width)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            egui::Frame::new()
                .fill(app.palette.leaf)
                .inner_margin(Margin::same(18)),
        )
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
            match &app.source {
                SourceState::Empty => empty(app, ui, actions),
                SourceState::Loading { path, format } => {
                    loading(app, ui, path, *format)
                }
                SourceState::File(file) => {
                    file_header(app, ui, file, actions);
                    components::hairline(ui, app.palette.hairline);
                    ui.add_space(2.0);
                    file_body(app, ui, file);
                }
                SourceState::Batch(batch) => {
                    batch_header(app, ui, batch, actions);
                    components::hairline(ui, app.palette.hairline);
                    batch_list(app, ui, batch, actions);
                }
            }
        });
}

// ---------------------------------------------------------------------------
// Empty
// ---------------------------------------------------------------------------

fn empty(app: &VersoApp, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    header_line(app, ui, "SOURCE", None);

    let reserve = 84.0;
    let height = (ui.available_height() - reserve).max(220.0);
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        Sense::click(),
    );

    let hover = app.hover_drop || response.hovered();
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(4),
        if hover {
            app.palette.tint(app.palette.amber, 0.06)
        } else {
            app.palette.leaf_alt
        },
    );
    components::dashed_rect(
        ui,
        rect.shrink(1.0),
        if hover {
            app.palette.amber
        } else {
            app.palette.hairline
        },
        5.0,
        4.0,
    );

    // A tiny empty film frame, drawn not iconed.
    let frame_rect = egui::Rect::from_center_size(
        rect.center() - Vec2::new(0.0, 54.0),
        Vec2::new(108.0, 34.0),
    );
    ui.painter().rect_stroke(
        frame_rect,
        CornerRadius::same(3),
        Stroke::new(1.0, app.palette.muted.gamma_multiply(0.6)),
        StrokeKind::Inside,
    );
    let mut x = frame_rect.left() + 8.0;
    while x < frame_rect.right() - 4.0 {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(x, frame_rect.top() + 4.0),
                Vec2::new(5.0, 3.0),
            ),
            CornerRadius::same(1),
            app.palette.muted.gamma_multiply(0.5),
        );
        ui.painter().rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(x, frame_rect.bottom() - 7.0),
                Vec2::new(5.0, 3.0),
            ),
            CornerRadius::same(1),
            app.palette.muted.gamma_multiply(0.5),
        );
        x += 13.0;
    }

    ui.painter().text(
        rect.center() + Vec2::new(0.0, -4.0),
        Align2::CENTER_CENTER,
        "Drop a document here",
        theme::display(23.0),
        app.palette.text,
    );
    ui.painter().text(
        rect.center() + Vec2::new(0.0, 22.0),
        Align2::CENTER_CENTER,
        "or drop several to convert them as a batch",
        theme::body(11.5),
        app.palette.muted,
    );
    ui.painter().text(
        egui::pos2(rect.center().x, rect.bottom() - 24.0),
        Align2::CENTER_CENTER,
        "TXT   MD   HTML   DOCX   PDF   RTF   JSON   CSV",
        theme::mono(9.5),
        app.palette.muted,
    );

    if response.clicked() {
        actions.push(Action::OpenPicker);
    }
    response.on_hover_text("Click to choose, or drop files anywhere in this window.");

    ui.add_space(6.0);
    ui.vertical_centered(|ui| {
        let response = components::primary_button(
            ui,
            "CHOOSE A FILE",
            true,
            &app.palette,
        );
        if response.clicked() {
            actions.push(Action::OpenPicker);
        }
        ui.add_space(2.0);
        ui.label(
            RichText::new("Ctrl+O · the original is never changed")
                .font(theme::medium(9.5))
                .color(app.palette.muted),
        );
    });
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

fn loading(
    app: &VersoApp,
    ui: &mut egui::Ui,
    path: &std::path::Path,
    format: Option<verso_core::Format>,
) {
    header_line(
        app,
        ui,
        "SOURCE",
        format.map(|format| format.code().to_string()).as_deref(),
    );
    ui.add_space(4.0);
    ui.label(
        RichText::new(crate::state::file_name(path))
            .font(theme::display(18.0))
            .color(app.palette.text),
    );
    ui.label(
        RichText::new(shorten_path(path, 64))
            .font(theme::mono(10.0))
            .color(app.palette.muted),
    );

    ui.add_space(18.0);
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 3.0),
        Sense::hover(),
    );
    ui.painter().line_segment(
        [rect.left_center(), rect.right_center()],
        Stroke::new(1.0, app.palette.hairline),
    );
    let t = (app.job_elapsed().as_secs_f32() * 0.8) % 1.0;
    let span = rect.width();
    let head = rect.left() + span * t;
    let tail = (head - span * 0.25).max(rect.left());
    ui.painter().line_segment(
        [egui::pos2(tail, rect.center().y), egui::pos2(head, rect.center().y)],
        Stroke::new(2.0, app.palette.amber),
    );

    ui.add_space(18.0);
    ui.label(
        RichText::new("Reading the structure, not the upload.")
            .font(theme::body(12.0))
            .italics()
            .color(app.palette.muted),
    );
}

// ---------------------------------------------------------------------------
// File
// ---------------------------------------------------------------------------

fn file_header(
    app: &VersoApp,
    ui: &mut egui::Ui,
    file: &crate::state::SourceFile,
    actions: &mut Vec<Action>,
) {
    header_line(app, ui, "SOURCE", Some(file.format.code()));

    ui.label(
        RichText::new(components::ellipsize(&file.display_name(), 54))
            .font(theme::display(19.0))
            .color(app.palette.text),
    )
    .on_hover_text(file.path.display().to_string());
    ui.label(
        RichText::new(shorten_path(&file.path, 62))
            .font(theme::mono(10.0))
            .color(app.palette.muted),
    );
    ui.add_space(3.0);

    ui.horizontal(|ui| {
        ui.scope(|ui| {
            ui.add_enabled_ui(app.can_edit_pair(), |ui| {
                ComboBox::from_id_salt("source-format")
                    .selected_text(
                        RichText::new(format!("{} · {}", file.format.code(), file.format.name()))
                            .font(theme::medium(11.5)),
                    )
                    .width(178.0)
                    .height(300.0)
                    .show_ui(ui, |ui| {
                        for format in verso_core::Format::ALL {
                            if ui
                                .selectable_label(
                                    format == file.format,
                                    format!("{}  {}", format.code(), format.name()),
                                )
                                .clicked()
                                && format != file.format
                            {
                                actions.push(Action::SetSourceFormat(format));
                            }
                        }
                    });
            })
            .response
            .on_disabled_hover_text("A job is running.");
        });

        ui.label(
            RichText::new(format_size(file.size))
                .font(theme::mono(10.5))
                .color(app.palette.muted),
        );

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let clear = components::ghost_button(ui, "CLEAR", &app.palette);
            if clear.clicked() {
                actions.push(Action::Clear);
            }
            let replace = components::ghost_button(ui, "REPLACE", &app.palette);
            if replace.clicked() {
                actions.push(Action::OpenPicker);
            }
        });
    });

    ui.add_space(4.0);
    match &file.inspection {
        Inspection::Ready(preview) => {
            let summary = preview.summary;
            ui.horizontal(|ui| {
                components::stat(ui, "BLOCKS", summary.blocks, &app.palette);
                ui.add_space(16.0);
                components::stat(ui, "HEADINGS", summary.headings, &app.palette);
                ui.add_space(16.0);
                components::stat(ui, "TABLES", summary.tables, &app.palette);
                ui.add_space(16.0);
                components::stat(ui, "WORDS", summary.words, &app.palette);
            });
        }
        Inspection::Pending => {
            ui.label(
                RichText::new("Reading…")
                    .font(theme::body(11.5))
                    .color(app.palette.muted),
            );
        }
        Inspection::Failed(message) => {
            let response = components::well(ui, &app.palette, |ui| {
                ui.label(
                    RichText::new("This preview could not be read")
                        .font(theme::condensed_bold(10.0))
                        .color(app.palette.red),
                );
                ui.label(
                    RichText::new(message)
                        .font(theme::body(11.0))
                        .color(app.palette.text),
                );
                ui.label(
                    RichText::new("Choose the real source format above, then try again.")
                        .font(theme::body(10.5))
                        .italics()
                        .color(app.palette.muted),
                );
            });
            response.response.on_hover_text("The converter will use the same reader.");
        }
    }
}

fn file_body(app: &VersoApp, ui: &mut egui::Ui, file: &crate::state::SourceFile) {
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        components::eyebrow(ui, "STRUCTURE", app.palette.muted);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new("first 1,600 characters")
                    .font(theme::medium(9.0))
                    .color(app.palette.muted),
            );
        });
    });

    let available = (ui.available_height() - 8.0).max(120.0);
    ScrollArea::vertical()
        .id_salt("source-preview")
        .auto_shrink([false, false])
        .max_height(available)
        .show(ui, |ui| {
            components::well(ui, &app.palette, |ui| {
                ui.set_width(ui.available_width());
                match &file.inspection {
                    Inspection::Ready(preview) => {
                        components::snippet(ui, &preview.snippet, &app.palette);
                        if preview.truncated {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new("Preview ends here. The conversion reads all of it.")
                                    .font(theme::medium(9.5))
                                    .color(app.palette.muted),
                            );
                        }
                    }
                    Inspection::Pending => {
                        ui.label(
                            RichText::new("Reading…")
                                .font(theme::body(12.0))
                                .color(app.palette.muted),
                        );
                    }
                    Inspection::Failed(message) => {
                        ui.label(
                            RichText::new(message)
                                .font(theme::mono(11.0))
                                .color(app.palette.red),
                        );
                    }
                }
            });
        });
}

// ---------------------------------------------------------------------------
// Batch
// ---------------------------------------------------------------------------

fn batch_header(
    app: &VersoApp,
    ui: &mut egui::Ui,
    batch: &Batch,
    actions: &mut Vec<Action>,
) {
    header_line(app, ui, "BATCH", Some("MIXED"));
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(
                RichText::new(format!(
                    "{} file{}",
                    batch.files.len(),
                    if batch.files.len() == 1 { "" } else { "s" }
                ))
                .font(theme::display(19.0))
                .color(app.palette.text),
            );
            ui.label(
                RichText::new(format!(
                    "{} total · numbered in drop order",
                    format_size(batch.files.iter().map(|file| file.size).sum())
                ))
                .font(theme::mono(10.0))
                .color(app.palette.muted),
            );
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let clear = components::ghost_button(ui, "CLEAR", &app.palette);
            if clear.clicked() {
                actions.push(Action::Clear);
            }
            let add = components::secondary_button(ui, "ADD FILES", !app.is_busy(), &app.palette);
            if add.clicked() {
                actions.push(Action::OpenPicker);
            }
            if app.is_busy() {
                add.on_disabled_hover_text("A batch is already running.");
            }
        });
    });

    if let Some(summary) = &app.batch_summary {
        ui.add_space(2.0);
        let color = if summary.failed > 0 || summary.skipped > 0 {
            app.palette.amber_ink
        } else {
            app.palette.sage_ink
        };
        ui.label(
            RichText::new(format!(
                "{} done · {} failed · {} skipped · {} · {} ms",
                summary.done,
                summary.failed,
                summary.skipped,
                format_size(summary.bytes),
                summary.elapsed_ms
            ))
            .font(theme::medium(10.0))
            .color(color),
        );
    }
}

fn batch_list(
    app: &VersoApp,
    ui: &mut egui::Ui,
    batch: &Batch,
    actions: &mut Vec<Action>,
) {
    ui.add_space(2.0);
    let available = ui.available_height();
    ScrollArea::vertical()
        .id_salt("batch-list")
        .auto_shrink([false, false])
        .max_height(available)
        .show(ui, |ui| {
            for (index, file) in batch.files.iter().enumerate() {
                batch_row(app, ui, index, file, actions);
                ui.add_space(4.0);
            }
            ui.add_space(6.0);
            ui.label(
                RichText::new("Each output is written next to its source, never over it.")
                    .font(theme::medium(9.5))
                    .color(app.palette.muted),
            );
        });
}

fn batch_row(
    app: &VersoApp,
    ui: &mut egui::Ui,
    index: usize,
    file: &crate::state::BatchFile,
    actions: &mut Vec<Action>,
) {
    let status_color = match &file.status {
        BatchStatus::Waiting => app.palette.muted,
        BatchStatus::Running => app.palette.amber_ink,
        BatchStatus::Done { .. } => app.palette.sage_ink,
        BatchStatus::Failed(_) => app.palette.red,
        BatchStatus::Skipped(_) => app.palette.amber_ink,
    };

    components::sheet(ui, &app.palette, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{:02}", index + 1))
                    .font(theme::mono(11.0))
                    .color(app.palette.muted),
            );

            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(
                    RichText::new(components::ellipsize(&file.display_name(), 40))
                        .font(theme::medium(12.5))
                        .color(app.palette.text),
                )
                .on_hover_text(file.path.display().to_string());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{} · {}", file.format.code(), format_size(file.size)))
                            .font(theme::mono(9.0))
                            .color(app.palette.muted),
                    );
                    if let BatchStatus::Done {
                        output,
                        bytes,
                        warnings,
                        elapsed_ms,
                    } = &file.status
                    {
                        ui.label(
                            RichText::new(format!("→ {}", format_size(*bytes as u64)))
                                .font(theme::mono(9.0))
                                .color(app.palette.sage_ink),
                        )
                        .on_hover_text(format!(
                            "Wrote {}\nin {} ms",
                            output.display(),
                            elapsed_ms
                        ));
                        if !warnings.is_empty() {
                            ui.label(
                                RichText::new(format!(
                                    "· {} warning{}",
                                    warnings.len(),
                                    if warnings.len() == 1 { "" } else { "s" }
                                ))
                                .font(theme::medium(9.0))
                                .color(app.palette.amber_ink),
                            )
                            .on_hover_text(warnings.join("\n"));
                        }
                    }
                });
                if let Some(message) = file.status.message() {
                    ui.label(
                        RichText::new(components::ellipsize(message, 58))
                            .font(theme::body(9.5))
                            .color(status_color),
                    )
                    .on_hover_text(message);
                }
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if !app.is_busy() {
                    let remove = components::ghost_button(ui, "×", &app.palette);
                    if remove.clicked() {
                        actions.push(Action::RemoveBatchFile(index));
                    }
                    remove.on_hover_text("Remove from this batch");
                }
                let (dot, _) =
                    ui.allocate_exact_size(Vec2::new(8.0, 8.0), Sense::hover());
                ui.painter()
                    .circle_filled(dot.center(), 2.8, status_color);
                ui.label(
                    RichText::new(file.status.label())
                        .font(theme::condensed_bold(8.5))
                        .color(status_color)
                        .extra_letter_spacing(0.6),
                );
            });
        });
    });
}

// ---------------------------------------------------------------------------
// Shared header
// ---------------------------------------------------------------------------

fn header_line(app: &VersoApp, ui: &mut egui::Ui, label: &str, code: Option<&str>) {
    ui.horizontal(|ui| {
        components::eyebrow(ui, label, app.palette.muted);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(code) = code {
                ui.label(
                    RichText::new(code)
                        .font(theme::mono(9.5))
                        .color(app.palette.muted),
                );
            }
            ui.label(
                RichText::new("LOCAL")
                    .font(theme::condensed(8.5))
                    .color(app.palette.muted),
            );
        });
    });
}
