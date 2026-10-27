use aegis_lang::{disasm, pe};
use eframe::egui::{self, Color32, RichText};

pub struct Panel {
    rva: u32,
    input: String,
    cache: Option<Result<disasm::Page, String>>,
    history: Vec<u32>,
}
impl Panel {
    pub fn new(rva: u32) -> Self {
        Self {
            rva,
            input: format!("0x{rva:X}"),
            cache: None,
            history: Vec::new(),
        }
    }
    pub fn goto(&mut self, rva: u32, image: &pe::Image, bytes: &[u8]) {
        self.load(rva, true, image, bytes);
    }
    fn load(&mut self, rva: u32, remember: bool, image: &pe::Image, bytes: &[u8]) {
        if remember && rva != self.rva && self.cache.as_ref().is_some_and(|p| p.is_ok()) {
            if self.history.len() == 128 {
                self.history.remove(0);
            }
            self.history.push(self.rva);
        }
        self.rva = rva;
        self.input = format!("0x{rva:X}");
        self.cache = Some(disasm::pe_page(image, bytes, rva, 128).map_err(|e| e.to_string()));
    }
    pub fn show(&mut self, ui: &mut egui::Ui, image: &pe::Image, bytes: &[u8]) -> Option<usize> {
        if self.cache.is_none() {
            self.load(self.rva, false, image, bytes);
        }
        let mut jump = None;
        ui.label(
            "Linear x86/x64 decoding · bytes may be data · instruction boundaries are not inferred",
        );
        ui.horizontal(|ui| {
            ui.label("RVA");
            ui.add_sized([150.0, 22.0], egui::TextEdit::singleline(&mut self.input));
            if ui.button("Go").clicked() {
                match disasm::parse_rva(&self.input) {
                    Ok(rva) => jump = Some((rva, true)),
                    Err(e) => self.cache = Some(Err(e.to_string())),
                }
            }
            if ui.button("Entry").clicked() {
                jump = Some((image.entry_rva, true));
            }
            if ui
                .add_enabled(!self.history.is_empty(), egui::Button::new("Back"))
                .clicked()
                && let Some(rva) = self.history.pop()
            {
                jump = Some((rva, false));
            }
            if let Some(Ok(page)) = &self.cache {
                if ui
                    .add_enabled(page.next_rva.is_some(), egui::Button::new("Next page"))
                    .clicked()
                    && let Some(rva) = page.next_rva
                {
                    jump = Some((rva, true));
                }
                if ui.button("Copy page").clicked() {
                    ui.ctx().copy_text(page.to_string());
                }
            }
        });
        let mut file_jump = None;
        match &self.cache {
            Some(Ok(page)) => {
                ui.label(format!(
                    "RVA 0x{:X} · file 0x{:X} · {} instructions · {} bytes",
                    page.rva,
                    page.file_offset,
                    page.listing.rows.len(),
                    page.listing.consumed
                ));
                egui::ScrollArea::both()
                    .id_salt("pe-disassembly")
                    .show(ui, |ui| {
                        for row in &page.listing.rows {
                            ui.horizontal(|ui| {
                                if ui
                                    .button(format!("{:016X}", row.address))
                                    .on_hover_text("Open instruction bytes in the file snapshot")
                                    .clicked()
                                {
                                    file_jump = Some(page.file_offset + row.offset);
                                }
                                let hex: String =
                                    row.bytes.iter().map(|b| format!("{b:02X} ")).collect();
                                ui.add_sized(
                                    [180.0, 22.0],
                                    egui::Label::new(RichText::new(&hex).monospace()).truncate(),
                                )
                                .on_hover_text(hex);
                                ui.label(RichText::new(&row.text).monospace().color(
                                    if row.invalid {
                                        Color32::LIGHT_RED
                                    } else {
                                        ui.visuals().text_color()
                                    },
                                ));
                                if let Some(target) = row
                                    .branch_target
                                    .and_then(|t| t.checked_sub(image.image_base))
                                    .and_then(|r| u32::try_from(r).ok())
                                    && image.file_range(target).is_ok()
                                    && ui.small_button("Follow").clicked()
                                {
                                    jump = Some((target, true));
                                }
                            });
                        }
                    });
            }
            Some(Err(error)) => {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
            None => {}
        }
        if let Some((rva, remember)) = jump {
            self.load(rva, remember, image, bytes);
        }
        file_jump
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_is_bounded_and_keeps_the_snapshot() {
        let bytes = aegis_lang::compile("fn main() { printi(42); }")
            .expect("PE")
            .executable;
        let original = bytes.clone();
        let image = pe::inspect(&bytes).expect("image");
        let entry = image.entry_rva;
        let mut panel = Panel::new(entry);
        panel.goto(entry, &image, &bytes);
        let next = panel
            .cache
            .as_ref()
            .expect("cache")
            .as_ref()
            .expect("page")
            .next_rva
            .expect("next");
        panel.goto(next, &image, &bytes);
        assert_eq!(panel.history, vec![entry]);
        for i in 0..200 {
            panel.goto(entry + (i % 2), &image, &bytes);
        }
        assert!(panel.history.len() <= 128);
        let ctx = egui::Context::default();
        assert!(
            !ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    panel.show(ui, &image, &bytes);
                });
            })
            .shapes
            .is_empty()
        );
        panel.goto(u32::MAX, &image, &bytes);
        assert!(panel.cache.as_ref().expect("cache").is_err());
        assert_eq!(bytes, original);
    }
}
