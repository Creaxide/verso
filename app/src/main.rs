#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Verso: a quiet document converter.
//!
//! One window, one binary, no network. The core crate does the reading and
//! writing; this crate draws the darkroom around it.

mod app;
mod components;
mod state;
mod theme;
mod ui;
mod worker;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Verso")
            .with_inner_size([1180.0, 780.0])
            .with_min_inner_size([900.0, 620.0])
            .with_icon(app_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "Verso",
        options,
        Box::new(|cc| Ok(Box::new(app::VersoApp::new(cc)))),
    )
}

/// A 64 px aperture mark, drawn once so the taskbar icon belongs to the app.
fn app_icon() -> egui::IconData {
    const SIZE: u32 = 64;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    let centre = (SIZE as f32 - 1.0) / 2.0;

    for y in 0..SIZE {
        for x in 0..SIZE {
            let px = x as f32 - centre;
            let py = y as f32 - centre;
            let radius = (px * px + py * py).sqrt();

            // Rounded film-black tile, 2 px clear of the icon edge.
            let tile = {
                let half = 28.0;
                let corner = 10.0;
                let qx = px.abs() - (half - corner);
                let qy = py.abs() - (half - corner);
                let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
                let inside = qx.max(qy).min(0.0);
                outside + inside - corner <= 0.0
            };

            let (r, g, b, a) = if !tile {
                (0, 0, 0, 0)
            } else if (17.5..=23.5).contains(&radius) {
                (0xE9, 0xA2, 0x3B, 255)
            } else if radius < 17.5 {
                (0x14, 0x13, 0x12, 255)
            } else {
                (0x14, 0x13, 0x12, 255)
            };

            let index = ((y * SIZE + x) * 4) as usize;
            rgba[index] = r;
            rgba[index + 1] = g;
            rgba[index + 2] = b;
            rgba[index + 3] = a;
        }
    }

    egui::IconData {
        rgba,
        width: SIZE,
        height: SIZE,
    }
}
