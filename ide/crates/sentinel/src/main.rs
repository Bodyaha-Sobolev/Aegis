//! Sentinel — окно с живым фоном DeepSeek (точный порт их шейдерного
//! паттерна, см. sky.rs). По указанию владельца интерфейс вычищен целиком:
//! никакого тулбара, сайдбара, табов, редактора и нативного бара — фон на
//! всё окно. Элементы интерфейса добавляются постепенно и только по команде
//! владельца; предыдущая реализация IDE сохранена в истории git (коммит
//! 4ae2242) для поэтапного возврата.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

mod sky;

use eframe::egui::{self, Key};
use sky::SkyField;
use std::path::PathBuf;

struct App {
    sky: SkyField,
    screenshot: Option<PathBuf>,
    smoke_frames: Option<u32>,
    allow_exit: bool,
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
                self.sky.paint(ui, ui.max_rect());
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
        // Keep the background alive even when idle.
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }
}

fn main() -> eframe::Result {
    let screenshot = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .map(|path| std::fs::canonicalize(&path).unwrap_or(path));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // Frameless on purpose: the owner removed the native bar and all
            // interface chrome; only the animated background remains.
            .with_decorations(false)
            .with_maximized(true),
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        "Sentinel",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(App {
                sky: SkyField::default(),
                smoke_frames: screenshot.is_some().then_some(0),
                screenshot,
                allow_exit: false,
            }))
        }),
    )
}
