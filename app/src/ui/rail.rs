//! The film-black rail: wordmark, locality, matrix and appearance.

use eframe::egui::{self, Align, Layout, Margin, RichText};

use crate::app::{Action, VersoApp};
use crate::theme::{self, RAIL_MUTED, RAIL_TEXT};

pub fn show(app: &VersoApp, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    egui::Panel::top("rail")
        .exact_size(44.0)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            egui::Frame::new()
                .fill(theme::FILM)
                .inner_margin(Margin::symmetric(16, 0)),
        )
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                crate::components::aperture(
                    ui.painter(),
                    rect.center(),
                    6.0,
                    app.palette.amber,
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new("VERSO")
                        .font(theme::display(17.0))
                        .color(RAIL_TEXT)
                        .extra_letter_spacing(1.6),
                );
                ui.add_space(6.0);
                if ui.available_width() > 520.0 {
                    ui.label(
                        RichText::new("DOCUMENT TRANSFER")
                            .font(theme::condensed(10.0))
                            .color(RAIL_MUTED)
                            .extra_letter_spacing(1.4),
                    );
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let theme_label = app.theme.label();
                    let response = ui.add(
                        egui::Button::new(
                            RichText::new(theme_label)
                                .font(theme::condensed_bold(10.5))
                                .color(RAIL_MUTED)
                                .extra_letter_spacing(1.1),
                        )
                        .frame(false)
                        .min_size(egui::vec2(48.0, 24.0)),
                    );
                    if response
                        .on_hover_text("Appearance: auto, dark, or light. Click to cycle.")
                        .clicked()
                    {
                        actions.push(Action::CycleTheme);
                    }

                    ui.add_space(4.0);
                    let open = app.matrix_open;
                    let response = ui.add(
                        egui::Button::new(
                            RichText::new("FORMAT MATRIX")
                                .font(theme::condensed_bold(10.5))
                                .color(if open { app.palette.amber } else { RAIL_MUTED })
                                .extra_letter_spacing(1.1),
                        )
                        .frame(false)
                        .min_size(egui::vec2(96.0, 24.0)),
                    );
                    if response
                        .on_hover_text("Which pairs are clean, lossy, or impossible. Ctrl+M")
                        .clicked()
                    {
                        actions.push(Action::ToggleMatrix);
                    }

                    ui.add_space(14.0);
                    let (dot, _) = ui.allocate_exact_size(
                        egui::vec2(12.0, 12.0),
                        egui::Sense::hover(),
                    );
                    ui.painter()
                        .circle_filled(dot.center(), 3.0, app.palette.sage);
                    ui.label(
                        RichText::new("LOCAL ONLY")
                            .font(theme::condensed_bold(9.5))
                            .color(RAIL_MUTED)
                            .extra_letter_spacing(1.0),
                    )
                    .on_hover_text("Every conversion runs on this machine. No uploads.");
                });
            });
        });
}
