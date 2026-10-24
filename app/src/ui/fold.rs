//! The fold between the leaves. Its one control turns the proof back into a
//! source, which is the second half of the conversion story.

use eframe::egui::{self, Align, Layout, Margin, RichText, Sense, Stroke, Vec2};

use crate::app::{Action, VersoApp};
use crate::components;
use crate::theme;

pub fn show(app: &VersoApp, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    egui::Panel::left("fold")
        .exact_size(48.0)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            egui::Frame::new()
                .fill(app.palette.background)
                .inner_margin(Margin::same(0)),
        )
        .show(ui, |ui| {
            let rect = ui.max_rect();
            let center_x = rect.center().x;

            // Hairlines mark the two sheet edges; the gap is the table.
            ui.painter().line_segment(
                [rect.left_top(), rect.left_bottom()],
                Stroke::new(1.0, app.palette.hairline),
            );
            ui.painter().line_segment(
                [rect.right_top(), rect.right_bottom()],
                Stroke::new(1.0, app.palette.hairline),
            );

            let (enabled, reason) = app.swap_state();
            let blocked = !enabled && app.completed.is_some();

            let top = rect.top() + 18.0;
            let bottom = rect.bottom() - 18.0;
            let orb_center_y = rect.center().y;

            // Dotted guides lead the eye from each sheet to the orb.
            for (from, to) in [
                (top, orb_center_y - 22.0),
                (orb_center_y + 22.0, bottom),
            ] {
                if to > from {
                    let mut y = from;
                    while y < to {
                        ui.painter().line_segment(
                            [egui::pos2(center_x, y), egui::pos2(center_x, (y + 2.0).min(to))],
                            Stroke::new(1.0, app.palette.hairline),
                        );
                        y += 6.0;
                    }
                }
            }

            if let Some(at) = app.swap_flash {
                let progress = (at.elapsed().as_secs_f32() / 0.7).clamp(0.0, 1.0);
                if progress < 1.0 {
                    let radius = 19.0 + progress * 16.0;
                    ui.painter().circle_stroke(
                        egui::pos2(center_x, orb_center_y),
                        radius,
                        Stroke::new(
                            1.5 * (1.0 - progress),
                            app.palette.tint(app.palette.amber, 1.0 - progress),
                        ),
                    );
                }
            }

            let offset = (orb_center_y - rect.top() - 19.0).max(0.0);
            ui.allocate_ui_with_layout(
                Vec2::new(rect.width(), rect.height()),
                Layout::top_down(Align::Center),
                |ui| {
                    ui.add_space(offset);
                    let response = components::swap_orb(
                        ui,
                        enabled,
                        blocked,
                        &app.palette,
                    );
                    if response.clicked() && enabled {
                        actions.push(Action::Swap);
                    }
                    if enabled {
                        response.on_hover_text("Swap · Ctrl+Shift+S\nTurns the proof into the source.");
                    } else {
                        response.on_hover_text(reason);
                    }

                    ui.add_space(5.0);
                    ui.label(
                        RichText::new("SWAP")
                            .font(theme::condensed_bold(8.0))
                            .color(if enabled {
                                app.palette.amber_ink
                            } else {
                                app.palette.muted
                            })
                            .extra_letter_spacing(1.0),
                    );
                },
            );

            let _ = Sense::hover();
        });
}
