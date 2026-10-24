//! Visual world: the darkroom / light table.
//!
//! The rail and the transfer strip are always film black; only the two leaves
//! change with the light. That is the one rule the whole theme rests on.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, Context, FontData, FontDefinitions, FontFamily, FontId, Stroke, Style, Visuals,
    ThemePreference,
};

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

pub const FILM: Color32 = Color32::from_rgb(0x14, 0x13, 0x12);
pub const STRIP: Color32 = Color32::from_rgb(0x1C, 0x1A, 0x18);
pub const RAIL_TEXT: Color32 = Color32::from_rgb(0xE8, 0xE3, 0xD9);
pub const RAIL_MUTED: Color32 = Color32::from_rgb(0x8F, 0x88, 0x7C);
pub const AMBER: Color32 = Color32::from_rgb(0xE9, 0xA2, 0x3B);
pub const RED: Color32 = Color32::from_rgb(0xE0, 0x55, 0x3C);
pub const SAGE: Color32 = Color32::from_rgb(0x9C, 0xC5, 0xA1);

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    pub rail_muted: Color32,
    pub background: Color32,
    pub leaf: Color32,
    pub leaf_alt: Color32,
    pub well: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub hairline: Color32,
    pub control: Color32,
    pub control_hover: Color32,
    pub amber: Color32,
    pub amber_ink: Color32,
    pub amber_soft: Color32,
    pub red: Color32,
    pub sage: Color32,
    pub sage_ink: Color32,
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            dark: true,
            rail_muted: RAIL_MUTED,
            background: Color32::from_rgb(0x10, 0x0F, 0x0E),
            leaf: Color32::from_rgb(0x1A, 0x18, 0x16),
            leaf_alt: Color32::from_rgb(0x20, 0x1D, 0x1A),
            well: Color32::from_rgb(0x0E, 0x0D, 0x0C),
            text: Color32::from_rgb(0xED, 0xE9, 0xE2),
            muted: Color32::from_rgb(0x9A, 0x93, 0x88),
            hairline: Color32::from_rgb(0x30, 0x2B, 0x26),
            control: Color32::from_rgb(0x24, 0x21, 0x1D),
            control_hover: Color32::from_rgb(0x2E, 0x2A, 0x24),
            amber: AMBER,
            amber_ink: AMBER,
            amber_soft: Color32::from_rgb(0x3B, 0x2C, 0x16),
            red: RED,
            sage: SAGE,
            sage_ink: SAGE,
        }
    }

    pub fn light() -> Self {
        Self {
            dark: false,
            rail_muted: RAIL_MUTED,
            background: Color32::from_rgb(0xE9, 0xEA, 0xE5),
            leaf: Color32::from_rgb(0xF7, 0xF7, 0xF3),
            leaf_alt: Color32::from_rgb(0xEF, 0xEE, 0xE8),
            well: Color32::from_rgb(0xEC, 0xEA, 0xE2),
            text: Color32::from_rgb(0x1B, 0x1A, 0x17),
            muted: Color32::from_rgb(0x6A, 0x65, 0x5B),
            hairline: Color32::from_rgb(0xD5, 0xD1, 0xC5),
            control: Color32::from_rgb(0xF0, 0xEF, 0xE9),
            control_hover: Color32::from_rgb(0xE4, 0xE2, 0xD8),
            amber: Color32::from_rgb(0xC9, 0x7E, 0x19),
            amber_ink: Color32::from_rgb(0x8C, 0x55, 0x09),
            amber_soft: Color32::from_rgb(0xF3, 0xE4, 0xC9),
            red: Color32::from_rgb(0xB8, 0x3A, 0x25),
            sage: Color32::from_rgb(0x4F, 0x7D, 0x58),
            sage_ink: Color32::from_rgb(0x3F, 0x6B, 0x49),
        }
    }

    pub fn for_dark(dark: bool) -> Self {
        if dark {
            Self::dark()
        } else {
            Self::light()
        }
    }

    /// A translucent version of a colour, for fills behind text.
    pub fn tint(&self, color: Color32, amount: f32) -> Color32 {
        let alpha = (amount.clamp(0.0, 1.0) * 255.0) as u8;
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
    }
}

// ---------------------------------------------------------------------------
// Type
// ---------------------------------------------------------------------------

pub fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(Arc::from("display")))
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(Arc::from("medium")))
}

pub fn condensed(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(Arc::from("condensed")))
}

pub fn condensed_bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(Arc::from("condensed-bold")))
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn body(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

// ---------------------------------------------------------------------------
// Theme switching
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeChoice {
    Auto,
    Dark,
    Light,
}

impl ThemeChoice {
    pub fn next(self) -> Self {
        match self {
            ThemeChoice::Auto => ThemeChoice::Dark,
            ThemeChoice::Dark => ThemeChoice::Light,
            ThemeChoice::Light => ThemeChoice::Auto,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::Auto => "AUTO",
            ThemeChoice::Dark => "DARK",
            ThemeChoice::Light => "LIGHT",
        }
    }
}

impl Default for ThemeChoice {
    fn default() -> Self {
        ThemeChoice::Auto
    }
}

pub fn install_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();

    let faces: [(&str, &[u8]); 6] = [
        (
            "barlow-regular",
            include_bytes!("../../assets/fonts/Barlow-Regular.ttf"),
        ),
        (
            "barlow-medium",
            include_bytes!("../../assets/fonts/Barlow-Medium.ttf"),
        ),
        (
            "barlow-semibold",
            include_bytes!("../../assets/fonts/Barlow-SemiBold.ttf"),
        ),
        (
            "barlow-bold",
            include_bytes!("../../assets/fonts/Barlow-Bold.ttf"),
        ),
        (
            "barlow-cond",
            include_bytes!("../../assets/fonts/BarlowSemiCondensed-Medium.ttf"),
        ),
        (
            "barlow-cond-bold",
            include_bytes!("../../assets/fonts/BarlowSemiCondensed-SemiBold.ttf"),
        ),
    ];
    for (name, bytes) in faces {
        fonts
            .font_data
            .insert(name.to_string(), Arc::new(FontData::from_static(bytes)));
    }

    // Keep the built-in fonts after Barlow so emoji and CJK still render.
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "barlow-regular".to_string());
    fonts.families.insert(
        FontFamily::Name(Arc::from("medium")),
        vec![
            "barlow-medium".to_string(),
            "barlow-regular".to_string(),
            "Ubuntu-Light".to_string(),
            "egui-icons".to_string(),
        ],
    );
    fonts.families.insert(
        FontFamily::Name(Arc::from("display")),
        vec![
            "barlow-semibold".to_string(),
            "barlow-bold".to_string(),
            "barlow-regular".to_string(),
            "Ubuntu-Light".to_string(),
        ],
    );
    fonts.families.insert(
        FontFamily::Name(Arc::from("condensed")),
        vec![
            "barlow-cond".to_string(),
            "barlow-cond-bold".to_string(),
            "barlow-regular".to_string(),
            "Ubuntu-Light".to_string(),
        ],
    );
    fonts.families.insert(
        FontFamily::Name(Arc::from("condensed-bold")),
        vec![
            "barlow-cond-bold".to_string(),
            "barlow-cond".to_string(),
            "barlow-semibold".to_string(),
            "Ubuntu-Light".to_string(),
        ],
    );

    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &Context, dark: bool, palette: &Palette) {
    // Resolve Auto before we get here, then pin the theme so our palette and
    // egui's popup styling can never disagree.
    ctx.set_theme(if dark {
        ThemePreference::Dark
    } else {
        ThemePreference::Light
    });

    let mut style = Style::default();
    style.visuals = visuals(palette);
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.interact_size.y = 26.0;
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.scroll.floating = true;

    style.text_styles = [
        (
            egui::TextStyle::Heading,
            display(21.0),
        ),
        (egui::TextStyle::Body, body(14.0)),
        (egui::TextStyle::Monospace, mono(12.0)),
        (egui::TextStyle::Button, medium(13.5)),
        (egui::TextStyle::Small, medium(11.0)),
    ]
    .into();

    // Keep both theme slots in sync; the active one is pinned above and the
    // other is used by any popup egui resolves while switching.
    ctx.set_style_of(egui::Theme::Dark, style.clone());
    ctx.set_style_of(egui::Theme::Light, style);
}

fn visuals(palette: &Palette) -> Visuals {
    let mut visuals = if palette.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };

    visuals.override_text_color = Some(palette.text);
    visuals.panel_fill = palette.background;
    visuals.window_fill = palette.leaf;
    visuals.extreme_bg_color = palette.well;
    visuals.faint_bg_color = palette.leaf_alt;
    visuals.window_stroke = Stroke::new(1.0, palette.hairline);
    visuals.window_corner_radius = egui::CornerRadius::same(4);
    visuals.menu_corner_radius = egui::CornerRadius::same(3);
    visuals.popup_shadow = egui::epaint::Shadow::NONE;
    visuals.window_shadow = egui::epaint::Shadow::NONE;

    visuals.selection.bg_fill = palette.tint(palette.amber, 0.22);
    visuals.selection.stroke = Stroke::new(1.0, palette.amber);

    visuals.widgets.noninteractive.bg_fill = palette.leaf;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.hairline);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette.text);

    visuals.widgets.inactive.bg_fill = palette.control;
    visuals.widgets.inactive.weak_bg_fill = palette.control;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, palette.hairline);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, palette.text);

    visuals.widgets.hovered.bg_fill = palette.control_hover;
    visuals.widgets.hovered.weak_bg_fill = palette.control_hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, palette.muted);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, palette.text);

    visuals.widgets.active.bg_fill = palette.control_hover;
    visuals.widgets.active.weak_bg_fill = palette.control_hover;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, palette.amber);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, palette.text);

    visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(4);
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(4);
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(4);
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(4);
    visuals.widgets.open.corner_radius = egui::CornerRadius::same(4);

    visuals
}
