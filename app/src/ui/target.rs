//! The target leaf: the pair brief before conversion, the proof after it.

use eframe::egui::{
    self, Align, ComboBox, CornerRadius, Layout, Margin, RichText, ScrollArea, Sense, Stroke,
    StrokeKind, Vec2,
};

use crate::app::{Action, VersoApp};
use crate::components;
use crate::state::{format_size, shorten_path};
use crate::theme;
use verso_core::{pair_info, Format, Support};

pub fn show(app: &VersoApp, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(app.palette.leaf)
                .inner_margin(Margin::same(18)),
        )
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
            target_header(app, ui, actions);
            components::hairline(ui, app.palette.hairline);
            ui.add_space(2.0);

            ScrollArea::vertical()
                .id_salt("target-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if let Some(completed) = &app.completed {
                        proof(app, ui, completed, actions);
                    } else if let Some(summary) = &app.batch_summary {
                        batch_proof(app, ui, summary, actions);
                    } else if app.source.is_empty() {
                        empty(app, ui, actions);
                    } else if let Some(batch) = app.source.batch() {
                        batch_brief(app, ui, batch);
                    } else if app.source.is_loading() {
                        loading(app, ui);
                    } else {
                        brief(app, ui);
                    }
                });
        });
}

fn target_header(app: &VersoApp, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        components::eyebrow(ui, "TARGET", app.palette.muted);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let mode = if app.completed.is_some() || app.batch_summary.is_some() {
                "PROOF"
            } else {
                "BRIEF"
            };
            ui.label(
                RichText::new(mode)
                    .font(theme::mono(9.5))
                    .color(app.palette.muted),
            );
        });
    });

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(app.target.code())
                .font(theme::condensed_bold(34.0))
                .color(app.palette.amber),
        );
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(
                RichText::new(app.target.name())
                    .font(theme::display(18.0))
                    .color(app.palette.text),
            );
            ui.label(
                RichText::new(format!(".{}", app.target.extension()))
                    .font(theme::mono(9.5))
                    .color(app.palette.muted),
            );
        });

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_enabled_ui(app.can_edit_pair(), |ui| {
                ComboBox::from_id_salt("target-format")
                    .selected_text(
                        RichText::new("CHANGE")
                            .font(theme::condensed_bold(10.0))
                            .color(app.palette.muted)
                            .extra_letter_spacing(0.8),
                    )
                    .width(96.0)
                    .height(300.0)
                    .show_ui(ui, |ui| {
                        for format in Format::ALL {
                            let supported = app
                                .source_format()
                                .map_or(true, |from| verso_core::is_supported(from, format));
                            ui.add_enabled_ui(supported, |ui| {
                                if ui
                                    .selectable_label(
                                        format == app.target,
                                        format!("{}  {}", format.code(), format.name()),
                                    )
                                    .clicked()
                                    && format != app.target
                                {
                                    actions.push(Action::SetTarget(format));
                                }
                            })
                            .response
                            .on_disabled_hover_text(format!(
                                "{} cannot become {}.",
                                app.source_format()
                                    .map(|from| from.code())
                                    .unwrap_or("This"),
                                format.code()
                            ));
                        }
                    });
            })
            .response
            .on_disabled_hover_text("A job is running.");
        });
    });
}

// ---------------------------------------------------------------------------
// Empty
// ---------------------------------------------------------------------------

fn empty(app: &VersoApp, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    ui.add_space(10.0);
    let height = (ui.available_height() - 70.0).max(220.0);
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        Sense::hover(),
    );
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(4),
        app.palette.leaf_alt,
    );
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(4),
        Stroke::new(1.0, app.palette.hairline),
        StrokeKind::Inside,
    );

    let center = rect.center();
    ui.painter().text(
        center - Vec2::new(0.0, 28.0),
        egui::Align2::CENTER_CENTER,
        app.target.code(),
        theme::condensed_bold(46.0),
        app.palette.tint(app.palette.amber, 0.55),
    );
    ui.painter().text(
        center + Vec2::new(0.0, 22.0),
        egui::Align2::CENTER_CENTER,
        "The proof appears here",
        theme::display(20.0),
        app.palette.text,
    );
    ui.painter().text(
        center + Vec2::new(0.0, 46.0),
        egui::Align2::CENTER_CENTER,
        "Choose a document on the left. Its losses are stated before anything is written.",
        theme::body(10.5),
        app.palette.muted,
    );

    ui.add_space(10.0);
    ui.vertical_centered(|ui| {
        let response = components::secondary_button(
            ui,
            "OPEN A DOCUMENT",
            !app.is_busy(),
            &app.palette,
        );
        if response.clicked() {
            actions.push(Action::OpenPicker);
        }
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!("default target · {}", app.target.description()))
                .font(theme::medium(9.0))
                .color(app.palette.muted),
        );
    });
}

// ---------------------------------------------------------------------------
// Pair brief
// ---------------------------------------------------------------------------

fn brief(app: &VersoApp, ui: &mut egui::Ui) {
    let Some(from) = app.source_format() else {
        return;
    };
    let pair = pair_info(from, app.target);
    pair_brief(app, ui, pair.support, &pair.notes, None);

    if app.source.file().is_some() {
        ui.add_space(10.0);
        output_plan(app, ui);
    }
}

fn pair_brief(
    app: &VersoApp,
    ui: &mut egui::Ui,
    support: Support,
    notes: &[&str],
    unsupported_count: Option<usize>,
) {
    let color = app.palette.support_color(support);
    let headline = match support {
        Support::Native => "A clean transfer",
        Support::Lossy => "A lossy transfer",
        Support::Unsupported => "This pair cannot run",
    };
    let subline = match support {
        Support::Native => "Nothing structural is lost between this pair.",
        Support::Lossy => "These parts of the source will not survive:",
        Support::Unsupported => "Verso will not pretend this is possible:",
    };

    ui.add_space(2.0);
    let response = egui::Frame::new()
        .fill(app.palette.tint(color, 0.06))
        .stroke(Stroke::new(1.0, app.palette.tint(color, 0.38)))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                components::chip(ui, support.label().to_uppercase().as_str(), app.palette.support_ink(support), &app.palette);
                ui.label(
                    RichText::new(headline)
                        .font(theme::display(15.0))
                        .color(app.palette.text),
                );
            });
            ui.add_space(2.0);
            ui.label(
                RichText::new(subline)
                    .font(theme::body(11.0))
                    .color(app.palette.muted),
            );
            for note in notes {
                ui.add_space(3.0);
                ui.horizontal_top(|ui| {
                    let (dot, _) =
                        ui.allocate_exact_size(Vec2::new(10.0, 14.0), Sense::hover());
                    ui.painter().rect_filled(
                        egui::Rect::from_center_size(dot.center(), Vec2::splat(4.0)),
                        CornerRadius::same(1),
                        color,
                    );
                    ui.label(
                        RichText::new(*note)
                            .font(theme::body(11.0))
                            .color(app.palette.text),
                    );
                });
            }
            if let Some(count) = unsupported_count {
                if count > 0 {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!(
                            "{count} file{} in this batch will be skipped.",
                            if count == 1 { "" } else { "s" }
                        ))
                        .font(theme::medium(10.5))
                        .color(app.palette.red),
                    );
                }
            }
        });
    response.response.on_hover_text(support.description());

    ui.add_space(10.0);
    ui.label(
        RichText::new(app.target.description())
            .font(theme::body(11.5))
            .color(app.palette.muted),
    );
}

fn output_plan(app: &VersoApp, ui: &mut egui::Ui) {
    components::eyebrow(ui, "THIS WILL WRITE", app.palette.muted);
    let name = app
        .pending_output_name()
        .unwrap_or_else(|| format!("<name>.{}", app.target.extension()));
    let folder = app
        .source
        .primary_path()
        .and_then(|path| path.parent())
        .map(|path| shorten_path(path, 58))
        .unwrap_or_else(|| "beside the source".to_string());

    let response = components::well(ui, &app.palette, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new(name)
                .font(theme::mono(12.5))
                .color(app.palette.text),
        );
        ui.label(
            RichText::new(folder)
                .font(theme::mono(9.5))
                .color(app.palette.muted),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("The source file is never touched. If that name exists, Verso adds _1, _2, …")
                .font(theme::body(10.0))
                .italics()
                .color(app.palette.muted),
        );
    });
    response.response.on_hover_text("Output location and collision rule");
}

// ---------------------------------------------------------------------------
// Proof
// ---------------------------------------------------------------------------

fn proof(
    app: &VersoApp,
    ui: &mut egui::Ui,
    completed: &crate::state::Completed,
    actions: &mut Vec<Action>,
) {
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        components::chip(ui, "PROOF", app.palette.sage_ink, &app.palette);
        ui.label(
            RichText::new(components::ellipsize(&completed.output_name(), 48))
                .font(theme::display(18.0))
                .color(app.palette.text),
        )
        .on_hover_text(completed.output.display().to_string());
    });
    ui.label(
        RichText::new(format!(
            "{} → {} · written in {} ms",
            completed.source_format.code(),
            completed.output_format.code(),
            completed.elapsed_ms
        ))
        .font(theme::mono(9.5))
        .color(app.palette.muted),
    );

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        components::stat(ui, "FORMAT", completed.output_format.code(), &app.palette);
        ui.add_space(18.0);
        components::stat(
            ui,
            "SIZE",
            format_size(completed.bytes as u64),
            &app.palette,
        );
        ui.add_space(18.0);
        components::stat(ui, "WARNINGS", completed.warnings.len(), &app.palette);
    });

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let open = components::secondary_button(ui, "OPEN FILE", true, &app.palette);
        if open.clicked() {
            actions.push(Action::OpenOutput);
        }
        let folder = components::secondary_button(ui, "OPEN FOLDER", true, &app.palette);
        if folder.clicked() {
            actions.push(Action::OpenFolder);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(shorten_path(&completed.output, 42))
                    .font(theme::mono(8.5))
                    .color(app.palette.muted),
            );
        });
    });

    if !completed.warnings.is_empty() {
        ui.add_space(8.0);
        egui::Frame::new()
            .fill(app.palette.tint(app.palette.amber, 0.07))
            .stroke(Stroke::new(1.0, app.palette.tint(app.palette.amber, 0.32)))
            .corner_radius(CornerRadius::same(4))
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                ui.label(
                    RichText::new("WHAT THE TARGET COULD NOT KEEP")
                        .font(theme::condensed_bold(9.0))
                        .color(app.palette.amber_ink)
                        .extra_letter_spacing(1.0),
                );
                for warning in &completed.warnings {
                    ui.add_space(3.0);
                    ui.label(
                        RichText::new(format!("— {warning}"))
                            .font(theme::body(10.5))
                            .color(app.palette.text),
                    );
                }
            });
    }

    ui.add_space(10.0);
    ui.horizontal(|ui| {
        components::eyebrow(ui, "PROOF TEXT", app.palette.muted);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(if completed.truncated {
                    "first 1,600 characters"
                } else {
                    "whole preview"
                })
                .font(theme::medium(9.0))
                .color(app.palette.muted),
            );
        });
    });
    let response = components::well(ui, &app.palette, |ui| {
        ui.set_width(ui.available_width());
        components::snippet(ui, &completed.snippet, &app.palette);
    });
    response.response.on_hover_text("Read back from the file that was written.");

    ui.add_space(10.0);
    ui.label(
        RichText::new("The orb in the fold turns this proof back into the source.")
            .font(theme::medium(9.5))
            .color(app.palette.muted),
    );
}

// ---------------------------------------------------------------------------
// Batch
// ---------------------------------------------------------------------------

fn batch_brief(app: &VersoApp, ui: &mut egui::Ui, batch: &crate::state::Batch) {
    let unsupported = batch
        .files
        .iter()
        .filter(|file| !verso_core::is_supported(file.format, app.target))
        .count();
    let total = batch.files.len();
    let done = batch.done();

    let mut distinct: Vec<Format> = Vec::new();
    for file in &batch.files {
        if !distinct.contains(&file.format) {
            distinct.push(file.format);
        }
    }

    let mut support = Support::Native;
    let mut notes: Vec<&str> = Vec::new();
    for format in &distinct {
        let pair = pair_info(*format, app.target);
        if pair.support == Support::Unsupported {
            support = Support::Unsupported;
        } else if pair.support == Support::Lossy && support != Support::Unsupported {
            support = Support::Lossy;
        }
        for note in pair.notes {
            if !notes.contains(&note) {
                notes.push(note);
            }
        }
    }

    ui.add_space(2.0);
    ui.horizontal(|ui| {
        components::chip(ui, "BATCH", app.palette.muted, &app.palette);
        ui.label(
            RichText::new(format!("{total} files → {}", app.target.code()))
                .font(theme::display(17.0))
                .color(app.palette.text),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if done > 0 {
                ui.label(
                    RichText::new(format!("{done}/{total} written"))
                        .font(theme::mono(9.0))
                        .color(app.palette.sage_ink),
                );
            }
        });
    });
    ui.label(
        RichText::new("Each source is read with its own format; one target for all.")
            .font(theme::body(10.5))
            .color(app.palette.muted),
    );

    ui.add_space(6.0);
    pair_brief(
        app,
        ui,
        support,
        &notes,
        Some(unsupported),
    );

    ui.add_space(10.0);
    components::eyebrow(ui, "WHERE THEY LAND", app.palette.muted);
    components::well(ui, &app.palette, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new("Next to each source file")
                .font(theme::mono(12.0))
                .color(app.palette.text),
        );
        ui.label(
            RichText::new("Existing names are never replaced; Verso adds _1, _2, … instead.")
                .font(theme::body(10.0))
                .italics()
                .color(app.palette.muted),
        );
    });

    if unsupported == 0 {
        ui.add_space(10.0);
        ui.label(
            RichText::new("Press Convert. Files run in parallel, and progress is counted in the footer.")
                .font(theme::body(10.5))
                .color(app.palette.muted),
        );
    }
}

fn batch_proof(
    app: &VersoApp,
    ui: &mut egui::Ui,
    summary: &crate::state::BatchSummary,
    actions: &mut Vec<Action>,
) {
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        components::chip(ui, "BATCH PROOF", app.palette.sage_ink, &app.palette);
        ui.label(
            RichText::new(format!(
                "{} file{}",
                summary.done,
                if summary.done == 1 { "" } else { "s" }
            ))
            .font(theme::display(18.0))
            .color(app.palette.text),
        );
    });
    ui.label(
        RichText::new(format!(
            "mixed sources → {} · {} ms",
            app.target.code(),
            summary.elapsed_ms
        ))
        .font(theme::mono(9.5))
        .color(app.palette.muted),
    );

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        components::stat(ui, "DONE", summary.done, &app.palette);
        ui.add_space(18.0);
        components::stat(ui, "FAILED", summary.failed, &app.palette);
        ui.add_space(18.0);
        components::stat(ui, "SKIPPED", summary.skipped, &app.palette);
        ui.add_space(18.0);
        components::stat(
            ui,
            "WRITTEN",
            format_size(summary.bytes),
            &app.palette,
        );
    });

    if summary.failed > 0 || summary.skipped > 0 {
        ui.add_space(8.0);
        ui.label(
            RichText::new("Open the batch list on the left for the reason on each file.")
                .font(theme::body(10.5))
                .color(app.palette.amber_ink),
        );
    }

    if let Some(folder) = &summary.folder {
        ui.add_space(10.0);
        components::eyebrow(ui, "OUTPUT FOLDER", app.palette.muted);
        components::well(ui, &app.palette, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(shorten_path(folder, 58))
                    .font(theme::mono(11.0))
                    .color(app.palette.text),
            );
        });
        ui.add_space(8.0);
        let response = components::secondary_button(ui, "OPEN FOLDER", true, &app.palette);
        if response.clicked() {
            actions.push(Action::OpenFolder);
        }
    }

    if summary.warnings > 0 {
        ui.add_space(8.0);
        ui.label(
            RichText::new(format!(
                "{} warning{} across the batch. Per-file warnings are in the list.",
                summary.warnings,
                if summary.warnings == 1 { "" } else { "s" }
            ))
            .font(theme::body(10.5))
            .color(app.palette.amber_ink),
        );
    }

    ui.add_space(10.0);
    ui.label(
        RichText::new("Changing the target resets the list so the next run is unambiguous.")
            .font(theme::medium(9.5))
            .color(app.palette.muted),
    );
}

fn loading(app: &VersoApp, ui: &mut egui::Ui) {
    ui.add_space(6.0);
    ui.label(
        RichText::new("Waiting for the source")
            .font(theme::display(18.0))
            .color(app.palette.text),
    );
    ui.label(
        RichText::new("The pair brief appears as soon as the structure is read.")
            .font(theme::body(11.0))
            .color(app.palette.muted),
    );
    ui.add_space(10.0);
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 120.0),
        Sense::hover(),
    );
    ui.painter().rect_filled(rect, CornerRadius::same(4), app.palette.leaf_alt);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(4),
        Stroke::new(1.0, app.palette.hairline),
        StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        app.target.code(),
        theme::condensed_bold(30.0),
        app.palette.tint(app.palette.amber, 0.5),
    );
}

trait SupportText {
    fn description(self) -> &'static str;
}

impl SupportText for Support {
    fn description(self) -> &'static str {
        match self {
            Support::Native => "This pair is clean: no structural loss specific to it.",
            Support::Lossy => "This pair is lossy: the brief above lists what is not carried.",
            Support::Unsupported => "This pair is impossible; choose another target.",
        }
    }
}
