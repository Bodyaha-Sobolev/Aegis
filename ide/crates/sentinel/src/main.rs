//! Sentinel — окно с живым фоном DeepSeek (точный порт их шейдерного
//! паттерна, см. sky.rs). По указанию владельца интерфейс вычищен целиком:
//! никакого тулбара, сайдбара, табов, редактора и нативного бара — фон на
//! всё окно. Элементы интерфейса добавляются постепенно и только по команде
//! владельца; предыдущая реализация IDE сохранена в истории git (коммит
//! 4ae2242) для поэтапного возврата.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

mod sky;

use eframe::egui::{self, Color32, Key};
use sky::SkyField;
use std::path::PathBuf;

struct App {
    sky: SkyField,
    screenshot: Option<PathBuf>,
    smoke_frames: Option<u32>,
    allow_exit: bool,
    /// True when the Windows caption icon font was found and registered.
    windows_icons: bool,
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // The smoke frame counter exists only for `sentinel shot.png`,
        // which captures a frame and exits on its own.
        if let Some(frames) = &mut self.smoke_frames {
            *frames += 1;
            if *frames == 16 && self.screenshot.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            if *frames >= 40 {
                self.allow_exit = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let full = ui.max_rect();
                self.sky.paint(ui, full);
                // Minimal custom titlebar: drag strip + the three standard
                // window buttons. Nothing else lives here.
                let bar = egui::Rect::from_min_size(full.min, egui::vec2(full.width(), 36.0));
                let button_size = egui::vec2(44.0, bar.height());
                let close_rect =
                    egui::Rect::from_min_size(bar.right_top() - egui::vec2(44.0, 0.0), button_size);
                let maximize_rect = egui::Rect::from_min_size(
                    close_rect.left_top() - egui::vec2(44.0, 0.0),
                    button_size,
                );
                let minimize_rect = egui::Rect::from_min_size(
                    maximize_rect.left_top() - egui::vec2(44.0, 0.0),
                    button_size,
                );
                let drag_rect =
                    egui::Rect::from_min_max(bar.min, egui::pos2(minimize_rect.left(), bar.max.y));
                let drag = ui.interact(
                    drag_rect,
                    egui::Id::new("titlebar-drag"),
                    egui::Sense::click_and_drag(),
                );
                if drag.double_clicked() {
                    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                } else if drag.dragged() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                if window_button(
                    ui,
                    minimize_rect,
                    "titlebar-minimize",
                    Color32::from_rgba_unmultiplied(15, 17, 21, 24),
                    self.windows_icons,
                    false,
                ) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
                if window_button(
                    ui,
                    maximize_rect,
                    "titlebar-maximize",
                    Color32::from_rgba_unmultiplied(15, 17, 21, 24),
                    self.windows_icons,
                    maximized,
                ) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }
                if window_button(
                    ui,
                    close_rect,
                    "titlebar-close",
                    Color32::from_rgba_unmultiplied(232, 17, 35, 230),
                    self.windows_icons,
                    false,
                ) {
                    self.allow_exit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        for event in ctx.input(|i| i.events.clone()) {
            if let egui::Event::Screenshot { image, .. } = event
                && let Some(path) = self.screenshot.take()
            {
                let rgba: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                if let Err(e) = image::save_buffer(
                    path,
                    &rgba,
                    image.size[0] as u32,
                    image.size[1] as u32,
                    image::ColorType::Rgba8,
                ) {
                    eprintln!("screenshot save failed: {e}");
                }
            }
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.allow_exit = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // High-FPS animation loop for the living background.
        ctx.request_repaint_after(std::time::Duration::from_millis(8));
    }
}

fn main() -> eframe::Result {
    let screenshot = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .map(|path| std::fs::canonicalize(&path).unwrap_or(path));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // Default native bar: drag by it, standard close/min/max buttons.
            .with_maximized(true),
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        "Sentinel",
        options,
        Box::new(move |cc| {
            // Use the very glyph font Windows draws its own caption buttons
            // with: Segoe Fluent Icons (Win11) / Segoe MDL2 Assets (Win10).
            let mut fonts = egui::FontDefinitions::default();
            let mut windows_icons = false;
            for path in [
                r"C:\Windows\Fonts\SegoeFluent-Icons.ttf",
                r"C:\Windows\Fonts\segmdl2.ttf",
            ] {
                if let Ok(bytes) = std::fs::read(path) {
                    fonts
                        .font_data
                        .insert("win_icons".into(), egui::FontData::from_owned(bytes).into());
                    fonts.families.insert(
                        egui::FontFamily::Name("WinIcons".into()),
                        vec!["win_icons".into()],
                    );
                    windows_icons = true;
                    break;
                }
            }
            cc.egui_ctx.set_fonts(fonts);
            Ok(Box::new(App {
                sky: SkyField::default(),
                smoke_frames: screenshot.is_some().then_some(0),
                screenshot,
                allow_exit: false,
                windows_icons,
            }))
        }),
    )
}

/// Standard window button over the background: invisible until hovered,
/// then a flat fill. The glyphs are the exact Windows caption icons from
/// Segoe Fluent Icons / Segoe MDL2 Assets (ChromeMinimize E921,
/// ChromeMaximize E922, ChromeRestore E923, ChromeClose E8BB); when the
/// font is missing, plain line shapes are drawn instead.
fn window_button(
    ui: &egui::Ui,
    rect: egui::Rect,
    id: &str,
    hover_fill: Color32,
    windows_icons: bool,
    maximized: bool,
) -> bool {
    let response = ui.interact(rect, egui::Id::new(id), egui::Sense::click());
    let painter = ui.painter();
    let close = id == "titlebar-close";
    if response.hovered() {
        painter.rect_filled(rect, 0.0, hover_fill);
    }
    let ink = if response.hovered() && close {
        Color32::WHITE
    } else if response.hovered() {
        Color32::from_rgb(15, 17, 21)
    } else {
        Color32::from_rgb(129, 133, 140)
    };
    let inner = rect.shrink2(egui::vec2(16.0, 12.0));
    if windows_icons {
        let glyph = match id {
            "titlebar-close" => "\u{E8BB}",
            "titlebar-maximize" if maximized => "\u{E923}",
            "titlebar-maximize" => "\u{E922}",
            _ => "\u{E921}",
        };
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            glyph,
            egui::FontId::new(10.0, egui::FontFamily::Name("WinIcons".into())),
            ink,
        );
        return response.clicked();
    }
    match id {
        "titlebar-close" => {
            painter.line_segment(
                [inner.left_top(), inner.right_bottom()],
                egui::Stroke::new(1.6, ink),
            );
            painter.line_segment(
                [inner.right_top(), inner.left_bottom()],
                egui::Stroke::new(1.6, ink),
            );
        }
        "titlebar-maximize" => {
            painter.rect_stroke(
                inner,
                2.0,
                egui::Stroke::new(1.6, ink),
                egui::StrokeKind::Inside,
            );
        }
        _ => {
            painter.line_segment(
                [inner.left_bottom(), inner.right_bottom()],
                egui::Stroke::new(1.6, ink),
            );
        }
    }
    response.clicked()
}
