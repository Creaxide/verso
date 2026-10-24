//! Small painted parts shared by the leaves: sprockets, the exposure route,
//! the swap orb, cells and wells. Everything here draws, nothing here decides.

use eframe::egui::{
    self, Color32, CornerRadius, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2,
};

use crate::theme::{self, Palette};

/// A 1px divider, inset from the leaf edge.
pub fn hairline(ui: &mut Ui, color: Color32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 1.0), Sense::hover());
    ui.painter()
        .line_segment([rect.left_center(), rect.right_center()], Stroke::new(1.0, color));
}

/// Small uppercase label used above values.
pub fn eyebrow(ui: &mut Ui, text: &str, color: Color32) {
    ui.label(
        RichText::new(text)
            .font(theme::medium(9.5))
            .color(color)
            .extra_letter_spacing(0.9),
    );
}

/// A value with an eyebrow above it, used in the structure row.
pub fn stat(ui: &mut Ui, label: &str, value: impl std::fmt::Display, palette: &Palette) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        eyebrow(ui, label, palette.muted);
        ui.label(
            RichText::new(value.to_string())
                .font(theme::condensed_bold(20.0))
                .color(palette.text),
        );
    });
}

/// A filled, rounded box for previews and error text.
pub fn well<R>(
    ui: &mut Ui,
    palette: &Palette,
    add: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<R> {
    egui::Frame::new()
        .fill(palette.well)
        .stroke(Stroke::new(1.0, palette.hairline))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(egui::Margin::same(12))
        .show(ui, add)
}

pub fn snippet(ui: &mut Ui, text: &str, palette: &Palette) {
    if text.trim().is_empty() {
        ui.label(
            RichText::new("No readable text to preview.")
                .font(theme::body(12.5))
                .italics()
                .color(palette.muted),
        );
        return;
    }
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .font(theme::mono(11.5))
                .color(palette.text),
        )
        .wrap()
        .selectable(true),
    );
}

/// Dashed outline, used for the drop target and the blocked route.
pub fn dashed_rect(ui: &Ui, rect: Rect, color: Color32, dash: f32, gap: f32) {
    dashed_line(ui, rect.left_top(), rect.right_top(), color, dash, gap);
    dashed_line(ui, rect.right_top(), rect.right_bottom(), color, dash, gap);
    dashed_line(ui, rect.right_bottom(), rect.left_bottom(), color, dash, gap);
    dashed_line(ui, rect.left_bottom(), rect.left_top(), color, dash, gap);
}

pub fn dashed_line(
    ui: &Ui,
    from: egui::Pos2,
    to: egui::Pos2,
    color: Color32,
    dash: f32,
    gap: f32,
) {
    let delta = to - from;
    let length = delta.length();
    if length <= 0.0 {
        return;
    }
    let direction = delta / length;
    let mut travelled = 0.0;
    let painter = ui.painter();
    while travelled < length {
        let end = (travelled + dash).min(length);
        painter.line_segment(
            [from + direction * travelled, from + direction * end],
            Stroke::new(1.0, color),
        );
        travelled += dash + gap;
    }
}

/// The film perforations that run along the transfer strip.
pub fn sprockets(painter: &egui::Painter, rect: Rect) {
    let hole = Color32::from_rgb(0x0A, 0x09, 0x08);
    let y_top = rect.top() + 2.0;
    let y_bottom = rect.bottom() - 6.0;
    let mut x = rect.left() + 11.0;
    while x < rect.right() - 10.0 {
        painter.rect_filled(
            Rect::from_min_size(egui::pos2(x, y_top), Vec2::new(10.0, 4.0)),
            CornerRadius::same(2),
            hole,
        );
        painter.rect_filled(
            Rect::from_min_size(egui::pos2(x, y_bottom), Vec2::new(10.0, 4.0)),
            CornerRadius::same(2),
            hole,
        );
        x += 22.0;
    }
}

/// The aperture mark: an amber ring with a dark centre.
pub fn aperture(painter: &egui::Painter, center: egui::Pos2, radius: f32, color: Color32) {
    painter.circle_filled(center, radius, color);
    painter.circle_filled(center, radius * 0.45, crate::theme::FILM);
}

/// The round control in the fold. Enabled means the reverse pair exists.
pub fn swap_orb(ui: &mut Ui, enabled: bool, blocked: bool, palette: &Palette) -> Response {
    let size = Vec2::splat(38.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered() && enabled;
    let center = rect.center();
    let radius = 18.0;

    let fill = if hovered {
        palette.amber_soft
    } else {
        palette.control
    };
    let ring = if blocked {
        palette.red
    } else if enabled {
        palette.amber
    } else {
        palette.hairline
    };
    let ink = if enabled { palette.amber_ink } else { palette.muted };

    ui.painter().circle_filled(center, radius, fill);
    ui.painter()
        .circle_stroke(center, radius, Stroke::new(1.4, ring));

    // Two opposing arrows, drawn rather than typed: Barlow has no U+21C4.
    let span = 8.0;
    let offset = 4.2;
    let top_y = center.y - offset;
    let bottom_y = center.y + offset;
    ui.painter().line_segment(
        [
            egui::pos2(center.x - span, top_y),
            egui::pos2(center.x + span, top_y),
        ],
        Stroke::new(1.4, ink),
    );
    ui.painter().line_segment(
        [
            egui::pos2(center.x - span, bottom_y),
            egui::pos2(center.x + span, bottom_y),
        ],
        Stroke::new(1.4, ink),
    );
    arrow_head(ui, egui::pos2(center.x + span, top_y), 1.0, ink);
    arrow_head(ui, egui::pos2(center.x - span, bottom_y), -1.0, ink);

    if blocked {
        ui.painter().line_segment(
            [
                egui::pos2(center.x - 5.0, center.y + 9.0),
                egui::pos2(center.x + 5.0, center.y + 15.0),
            ],
            Stroke::new(1.5, palette.red),
        );
    }

    response
}

fn arrow_head(ui: &Ui, tip: egui::Pos2, direction: f32, color: Color32) {
    ui.painter().line_segment(
        [tip, tip + Vec2::new(-4.0 * direction, -3.2)],
        Stroke::new(1.4, color),
    );
    ui.painter().line_segment(
        [tip, tip + Vec2::new(-4.0 * direction, 3.2)],
        Stroke::new(1.4, color),
    );
}

/// A pill used for the support label on the route.
pub fn pill(
    painter: &egui::Painter,
    center: egui::Pos2,
    text: &str,
    color: Color32,
    fill: Color32,
) {
    let font = theme::condensed_bold(9.5);
    let galley = painter.layout_no_wrap(text.to_string(), font, color);
    let size = galley.size() + Vec2::new(13.0, 6.0);
    let rect = Rect::from_center_size(center, size);
    painter.rect_filled(rect, CornerRadius::same(3), fill);
    painter.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(1.0, color.gamma_multiply(0.55)),
        StrokeKind::Inside,
    );
    painter.galley(
        rect.center() - galley.size() * 0.5,
        galley,
        color,
    );
}

/// The hairline between the frames, with the exposure line travelling on it.
pub fn route(
    painter: &egui::Painter,
    from: egui::Pos2,
    to: egui::Pos2,
    palette: &Palette,
    progress: Option<f32>,
    blocked: bool,
    done: bool,
) {
    let color = if blocked {
        palette.red.gamma_multiply(0.65)
    } else if done {
        palette.sage
    } else {
        palette.rail_muted.gamma_multiply(0.55)
    };
    if blocked {
        let length = (to.x - from.x).max(0.0);
        let mut x = from.x;
        while x < to.x {
            painter.line_segment(
                [egui::pos2(x, from.y), egui::pos2((x + 4.0).min(to.x), from.y)],
                Stroke::new(1.0, color),
            );
            x += 8.0;
        }
        let _ = length;
    } else {
        painter.line_segment(
            [from, to],
            Stroke::new(if done { 1.4 } else { 1.0 }, color),
        );
    }

    // Arrowhead at the target end.
    let head = if blocked {
        palette.red
    } else if done {
        palette.sage
    } else {
        palette.rail_muted
    };
    painter.line_segment(
        [to, to + Vec2::new(-5.0, -3.4)],
        Stroke::new(1.0, head),
    );
    painter.line_segment([to, to + Vec2::new(-5.0, 3.4)], Stroke::new(1.0, head));

    if let Some(fraction) = progress.filter(|value| value.is_finite()) {
        let fraction = fraction.clamp(0.0, 1.0);
        let end = egui::pos2(from.x + (to.x - from.x) * fraction, from.y);
        painter.line_segment(
            [from, end],
            Stroke::new(3.0, palette.tint(palette.amber, 0.22)),
        );
        painter.line_segment([from, end], Stroke::new(1.4, palette.amber));
        painter.circle_filled(end, 2.4, palette.amber);
    }
}

/// Truncate with an ellipsis on a character budget; good enough for one-line
/// labels where the font is known.
pub fn ellipsize(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

// ---------------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------------

pub fn primary_button(ui: &mut Ui, label: &str, enabled: bool, palette: &Palette) -> Response {
    let fill = if enabled {
        palette.amber
    } else {
        palette.control
    };
    let ink = if enabled { theme::FILM } else { palette.muted };
    let button = egui::Button::new(
        RichText::new(label)
            .font(theme::condensed_bold(13.5))
            .color(ink)
            .extra_letter_spacing(0.7),
    )
    .fill(fill)
    .stroke(Stroke::NONE)
    .corner_radius(CornerRadius::same(4))
    .min_size(Vec2::new(148.0, 34.0));
    ui.add_enabled(enabled, button)
}

pub fn secondary_button(ui: &mut Ui, label: &str, enabled: bool, palette: &Palette) -> Response {
    let ink = if enabled { palette.text } else { palette.muted };
    let button = egui::Button::new(
        RichText::new(label)
            .font(theme::condensed(12.5))
            .color(ink)
            .extra_letter_spacing(0.5),
    )
    .fill(palette.control)
    .stroke(Stroke::new(1.0, palette.hairline))
    .corner_radius(CornerRadius::same(3))
    .min_size(Vec2::new(0.0, 28.0));
    ui.add_enabled(enabled, button)
}

pub fn ghost_button(ui: &mut Ui, label: &str, palette: &Palette) -> Response {
    ui.add(
        egui::Button::new(
            RichText::new(label)
                .font(theme::condensed(11.5))
                .color(palette.muted)
                .extra_letter_spacing(0.5),
        )
        .frame(false)
        .min_size(Vec2::new(0.0, 22.0)),
    )
}

pub fn chip(ui: &mut Ui, text: &str, color: Color32, palette: &Palette) {
    let font = theme::condensed_bold(9.5);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font, color);
    let size = galley.size() + Vec2::new(12.0, 5.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(3),
        palette.tint(color, 0.12),
    );
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(1.0, palette.tint(color, 0.45)),
        StrokeKind::Inside,
    );
    ui.painter().galley(
        rect.center() - galley.size() * 0.5,
        galley,
        color,
    );
}

impl Palette {
    /// Convenience used by chips and route labels.
    pub fn support_color(&self, support: verso_core::Support) -> Color32 {
        match support {
            verso_core::Support::Native => self.sage,
            verso_core::Support::Lossy => self.amber,
            verso_core::Support::Unsupported => self.red,
        }
    }

    /// The same meaning as [`Self::support_color`], dark enough for small text
    /// on a leaf in both themes.
    pub fn support_ink(&self, support: verso_core::Support) -> Color32 {
        match support {
            verso_core::Support::Native => self.sage_ink,
            verso_core::Support::Lossy => self.amber_ink,
            verso_core::Support::Unsupported => self.red,
        }
    }
}

/// A frame drawn as a sheet edge; used for the empty drop area and batch rows.
pub fn sheet(ui: &mut Ui, palette: &Palette, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(palette.leaf_alt)
        .stroke(Stroke::new(1.0, palette.hairline))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}
