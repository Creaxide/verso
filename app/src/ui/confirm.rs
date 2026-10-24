//! Confirmation for very large images before they are decoded.

use eframe::egui::{self, Align, Layout, Margin, RichText};

use crate::app::{Action, VersoApp};
use crate::components;
use crate::theme;

pub fn show(app: &VersoApp, ctx: &egui::Context, actions: &mut Vec<Action>) {
    if !app.large_image_prompt {
        return;
    }
    let Some(info) = app.source_large_image() else {
        return;
    };
    let megapixels = f64::from(info.width) * f64::from(info.height) / 1_000_000.0;

    let modal = egui::Modal::new(egui::Id::unique("large-image"))
        .backdrop_color(egui::Color32::from_black_alpha(96))
        .frame(
            egui::Frame::new()
                .fill(app.palette.leaf)
                .stroke(egui::Stroke::new(1.0, app.palette.hairline))
                .corner_radius(egui::CornerRadius::same(4))
                .inner_margin(Margin::same(18)),
        )
        .show(ctx, |ui| {
            ui.set_width(360.0);
            ui.horizontal(|ui| {
                components::chip(ui, "LARGE IMAGE", app.palette.amber_ink, &app.palette);
                ui.label(
                    RichText::new("Before this decodes")
                        .font(theme::display(16.0))
                        .color(app.palette.text),
                );
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "{} × {} pixels ({megapixels:.0} MP).",
                    info.width, info.height
                ))
                .font(theme::mono(12.0))
                .color(app.palette.text),
            );
            ui.label(
                RichText::new(
                    "Verso holds the whole image in memory. Converting an image this large can take a while and use a lot of RAM.",
                )
                .font(theme::body(11.0))
                .color(app.palette.muted),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if components::primary_button(ui, "CONVERT ANYWAY", true, &app.palette).clicked() {
                    actions.push(Action::ConfirmLargeImage);
                }
                if components::secondary_button(ui, "CANCEL", true, &app.palette).clicked() {
                    actions.push(Action::DismissLargeImage);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new("100 MP limit")
                            .font(theme::medium(9.0))
                            .color(app.palette.muted),
                    );
                });
            });
        });

    if modal.backdrop_response.clicked() {
        actions.push(Action::DismissLargeImage);
    }
}

pub fn pdfium_prompt(app: &VersoApp, ctx: &egui::Context, actions: &mut Vec<Action>) {
    if !app.pdfium_prompt {
        return;
    }
    let modal = egui::Modal::new(egui::Id::unique("pdfium-missing"))
        .backdrop_color(egui::Color32::from_black_alpha(96))
        .frame(
            egui::Frame::new()
                .fill(app.palette.leaf)
                .stroke(egui::Stroke::new(1.0, app.palette.hairline))
                .corner_radius(egui::CornerRadius::same(4))
                .inner_margin(Margin::same(18)),
        )
        .show(ctx, |ui| {
            ui.set_width(400.0);
            ui.horizontal(|ui| {
                components::chip(ui, "PDFIUM", app.palette.amber_ink, &app.palette);
                ui.label(
                    RichText::new("PDF to image needs a renderer")
                        .font(theme::display(16.0))
                        .color(app.palette.text),
                );
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Verso rasterizes PDF pages with PDFium, a free library that is not bundled. Nothing was written and nothing crashed.",
                )
                .font(theme::body(11.0))
                .color(app.palette.muted),
            );
            ui.add_space(8.0);
            ui.label(
                RichText::new("github.com/bblanchon/pdfium-binaries/releases")
                    .font(theme::mono(11.5))
                    .color(app.palette.amber_ink),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Put pdfium.dll beside verso.exe, on PATH, or set VERSO_PDFIUM_PATH to it, then convert again.",
                )
                .font(theme::body(10.5))
                .color(app.palette.muted),
            );
            ui.add_space(12.0);
            if components::secondary_button(ui, "GOT IT", true, &app.palette).clicked() {
                actions.push(Action::DismissPdfium);
            }
        });

    if modal.backdrop_response.clicked() {
        actions.push(Action::DismissPdfium);
    }
}

pub fn swap_prompt(app: &VersoApp, ctx: &egui::Context, actions: &mut Vec<Action>) {
    if !app.swap_combine_prompt {
        return;
    }
    let count = app
        .completed
        .as_ref()
        .map(|completed| completed.pages.max(1))
        .unwrap_or(1);

    let modal = egui::Modal::new(egui::Id::unique("swap-combine"))
        .backdrop_color(egui::Color32::from_black_alpha(96))
        .frame(
            egui::Frame::new()
                .fill(app.palette.leaf)
                .stroke(egui::Stroke::new(1.0, app.palette.hairline))
                .corner_radius(egui::CornerRadius::same(4))
                .inner_margin(Margin::same(18)),
        )
        .show(ctx, |ui| {
            ui.set_width(360.0);
            ui.horizontal(|ui| {
                components::chip(ui, "TURN THE PROOF", app.palette.amber_ink, &app.palette);
                ui.label(
                    RichText::new("Combine the pages")
                        .font(theme::display(16.0))
                        .color(app.palette.text),
                );
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "Combine {count} image{} into one PDF?",
                    if count == 1 { "" } else { "s" }
                ))
                .font(theme::mono(12.0))
                .color(app.palette.text),
            );
            ui.label(
                RichText::new(
                    "Only the files written by the last conversion are used. The new batch targets PDF; press Convert to write it.",
                )
                .font(theme::body(11.0))
                .color(app.palette.muted),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if components::primary_button(ui, "USE THESE IMAGES", true, &app.palette).clicked()
                {
                    actions.push(Action::ConfirmSwapCombine);
                }
                if components::secondary_button(ui, "CANCEL", true, &app.palette).clicked() {
                    actions.push(Action::DismissSwapCombine);
                }
            });
        });

    if modal.backdrop_response.clicked() {
        actions.push(Action::DismissSwapCombine);
    }
}
