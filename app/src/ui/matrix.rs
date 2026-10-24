//! The full 8 × 8 matrix, opened from the rail.
//!
//! Rows are sources, columns are targets. The current row is the one you can
//! click; the other rows are there to be read, not guessed at.

use eframe::egui::{
    self, Align, Align2, CornerRadius, Layout, Margin, RichText, Sense, Stroke, StrokeKind, Vec2,
};
use verso_core::{pair_info, Format, Support};

use crate::app::{Action, VersoApp};
use crate::components;
use crate::theme;

const CELL: Vec2 = Vec2::new(34.0, 28.0);

pub fn show(app: &VersoApp, ctx: &egui::Context, actions: &mut Vec<Action>) {
    if !app.matrix_open {
        return;
    }

    // A modal, not a window over a hand-made dim sheet: egui's modal keeps the
    // backdrop and the sheet in one layer, so the backdrop can swallow a click
    // outside without eating clicks meant for the matrix itself.
    let modal = egui::Modal::new(egui::Id::unique("format-matrix"))
        .backdrop_color(egui::Color32::from_black_alpha(96))
        .frame(
            egui::Frame::new()
                .fill(app.palette.leaf)
                .stroke(Stroke::new(1.0, app.palette.hairline))
                .corner_radius(CornerRadius::same(4))
                .inner_margin(Margin::same(18)),
        )
        .show(ctx, |ui| {
            let columns = Format::ALL.len() as f32;
            ui.set_width(40.0 + 2.0 + columns * (CELL.x + 2.0));
            ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);

            ui.horizontal(|ui| {
                let (mark, _) =
                    ui.allocate_exact_size(Vec2::new(14.0, 14.0), Sense::hover());
                components::aperture(ui.painter(), mark.center(), 6.0, app.palette.amber);
                ui.label(
                    RichText::new("FORMAT MATRIX")
                        .font(theme::display(16.0))
                        .color(app.palette.text)
                        .extra_letter_spacing(0.8),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let close = components::ghost_button(ui, "CLOSE", &app.palette);
                    if close.clicked() {
                        actions.push(Action::ToggleMatrix);
                    }
                });
            });
            ui.label(
                RichText::new("Rows are sources; columns are targets. Hover a cell for the cost.")
                    .font(theme::body(10.0))
                    .color(app.palette.muted),
            );
            ui.add_space(8.0);

            let source = app.source_format();
            ui.horizontal(|ui| {
                header_cell(ui, "", app.palette.muted);
                for to in Format::ALL {
                    header_cell(ui, to.code(), app.palette.muted);
                }
            });

            for from in Format::ALL {
                ui.horizontal(|ui| {
                    row_header(app, ui, from, source);
                    for to in Format::ALL {
                        matrix_cell(app, ui, from, to, source, actions);
                    }
                });
            }

            ui.add_space(10.0);
            components::hairline(ui, app.palette.hairline);
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                legend(app, ui, Support::Native, "clean");
                ui.add_space(8.0);
                legend(app, ui, Support::Lossy, "lossy");
                ui.add_space(8.0);
                legend(app, ui, Support::Unsupported, "impossible");
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "PDF → CSV is refused: a PDF stores positions, not columns. Document ↔ image pairs arrive in the next stage.",
                )
                .font(theme::body(9.5))
                .color(app.palette.muted),
            );
            ui.add_space(3.0);
            ui.label(
                RichText::new(match source {
                    Some(format) => format!(
                        "Showing the {} row. Click a column to change the target.",
                        format.code()
                    ),
                    None => "Choose a source to lock a row; until then, a click changes the target."
                        .to_string(),
                })
                .font(theme::medium(9.0))
                .color(app.palette.muted),
            );
        });

    if modal.backdrop_response.clicked() {
        actions.push(Action::ToggleMatrix);
    }
}

fn header_cell(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(CELL.x, 18.0), Sense::hover());
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        theme::condensed_bold(9.0),
        color,
    );
}

fn row_header(app: &VersoApp, ui: &mut egui::Ui, format: Format, source: Option<Format>) {
    let active = source == Some(format);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(40.0, CELL.y), Sense::hover());
    if active {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(
                rect.left_top() + Vec2::new(0.0, 4.0),
                Vec2::new(2.0, rect.height() - 8.0),
            ),
            CornerRadius::same(1),
            app.palette.amber,
        );
    }
    ui.painter().text(
        rect.center() + Vec2::new(4.0, 0.0),
        Align2::CENTER_CENTER,
        format.code(),
        theme::condensed_bold(10.0),
        if active {
            app.palette.amber_ink
        } else if source.is_none() {
            app.palette.text
        } else {
            app.palette.muted
        },
    );
}

fn matrix_cell(
    app: &VersoApp,
    ui: &mut egui::Ui,
    from: Format,
    to: Format,
    source: Option<Format>,
    actions: &mut Vec<Action>,
) {
    let pair = pair_info(from, to);
    let color = app.palette.support_color(pair.support);
    let current = source == Some(from) && to == app.target;
    let clickable = pair.is_possible() && (source.is_none() || source == Some(from));

    let (rect, response) = ui.allocate_exact_size(CELL, Sense::click());
    let fill = match pair.support {
        Support::Native => app.palette.leaf_alt,
        Support::Lossy => app.palette.tint(app.palette.amber, 0.08),
        Support::Unsupported => app.palette.tint(app.palette.red, 0.06),
    };
    ui.painter().rect_filled(rect, CornerRadius::same(2), fill);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(2),
        Stroke::new(
            if current { 1.6 } else { 0.7 },
            if current {
                app.palette.amber
            } else if pair.support == Support::Native {
                app.palette.hairline
            } else {
                app.palette.tint(color, 0.35)
            },
        ),
        StrokeKind::Inside,
    );

    let center = rect.center();
    match pair.support {
        Support::Native => {
            ui.painter()
                .circle_filled(center, 2.0, app.palette.muted.gamma_multiply(0.8));
        }
        Support::Lossy => {
            ui.painter().circle_filled(center, 2.4, color);
        }
        Support::Unsupported => {
            ui.painter().line_segment(
                [center - Vec2::new(4.0, 0.0), center + Vec2::new(4.0, 0.0)],
                Stroke::new(1.4, color),
            );
        }
    }

    if response.clicked() && clickable {
        actions.push(Action::SetTarget(to));
        actions.push(Action::ToggleMatrix);
    }

    let hover = if clickable {
        match pair.support {
            Support::Native => format!(
                "{} → {} · clean\nNo structural loss specific to this pair.",
                from.code(),
                to.code()
            ),
            Support::Lossy => format!(
                "{} → {} · lossy\n{}",
                from.code(),
                to.code(),
                pair.notes.join("\n")
            ),
            Support::Unsupported => format!("{} → {} · impossible", from.code(), to.code()),
        }
    } else if pair.is_possible() {
        format!(
            "{} → {} is possible. Load a {} source to choose it.",
            from.code(),
            to.code(),
            from.code()
        )
    } else {
        format!(
            "{} → {} · impossible\n{}",
            from.code(),
            to.code(),
            pair.notes.join("\n")
        )
    };
    response.on_hover_text(hover);
}

fn legend(app: &VersoApp, ui: &mut egui::Ui, support: Support, label: &str) {
    let color = app.palette.support_color(support);
    let (dot, _) = ui.allocate_exact_size(Vec2::new(10.0, 10.0), Sense::hover());
    match support {
        Support::Native => {
            ui.painter()
                .circle_filled(dot.center(), 2.0, app.palette.muted);
        }
        Support::Lossy => {
            ui.painter().circle_filled(dot.center(), 2.4, color);
        }
        Support::Unsupported => {
            ui.painter().line_segment(
                [
                    dot.center() - Vec2::new(4.0, 0.0),
                    dot.center() + Vec2::new(4.0, 0.0),
                ],
                Stroke::new(1.4, color),
            );
        }
    }
    ui.label(
        RichText::new(label)
            .font(theme::medium(9.5))
            .color(app.palette.muted),
    );
}
