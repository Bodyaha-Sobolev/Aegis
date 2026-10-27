use aegis_lang::pe;
use eframe::egui::{self, Color32, RichText};
use std::path::PathBuf;

pub struct Inspector {
    path: PathBuf,
    image: pe::Image,
    selected: usize,
    filter: String,
    bytes: Vec<u8>,
    offset: usize,
}
impl Inspector {
    pub fn new(path: PathBuf, image: pe::Image, bytes: Vec<u8>) -> Self {
        Self {
            path,
            image,
            selected: 0,
            filter: String::new(),
            bytes,
            offset: 0,
        }
    }
    fn show_bytes(&mut self, ui: &mut egui::Ui) {
        ui.label("File offsets · snapshot captured during inspection · 256 bytes per page");
        ui.horizontal(|ui| {
            ui.label("Offset (decimal)");
            ui.add(
                egui::DragValue::new(&mut self.offset)
                    .range(0..=self.bytes.len().saturating_sub(1)),
            );
            if ui.button("Previous").clicked() {
                self.offset = self.offset.saturating_sub(256);
            }
            if ui.button("Next").clicked() {
                self.offset = self
                    .offset
                    .saturating_add(256)
                    .min(self.bytes.len().saturating_sub(1));
            }
        });
        let end = self.offset.saturating_add(256).min(self.bytes.len());
        egui::ScrollArea::both().show(ui, |ui| {
            if let Some(page) = self.bytes.get(self.offset..end) {
                for (row, bytes) in page.chunks(16).enumerate() {
                    let hex: String = bytes.iter().map(|b| format!("{b:02X} ")).collect();
                    let ascii: String = bytes
                        .iter()
                        .map(|&b| {
                            if b.is_ascii_graphic() || b == b' ' {
                                b as char
                            } else {
                                '.'
                            }
                        })
                        .collect();
                    ui.monospace(format!("{:08X}  {hex:48}  {ascii}", self.offset + row * 16));
                }
            }
            ui.separator();
            ui.label("Unsigned integer at offset (unaligned reads allowed)");
            for width in [1, 2, 4, 8] {
                let value = read_integer(&self.bytes, self.offset, width);
                ui.monospace(match value {
                    Some((le, be)) => format!("u{}  LE {le}  BE {be}", width * 8),
                    None => format!("u{}  outside file", width * 8),
                });
            }
        });
    }
    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        let mut open = true;
        egui::Window::new("Binary inspector").id(egui::Id::new("pe-inspector"))
            .open(&mut open).default_size([850.0, 580.0]).resizable(true).show(ctx, |ui| {
                ui.label(RichText::new(self.path.display().to_string()).strong());
                ui.label("Read-only file analysis · input was not executed · header flags are not a safety verdict");
                ui.horizontal(|ui| {
                    for (index, label) in ["Headers", "Sections", "Imports", "Bytes"].iter().enumerate() {
                        ui.selectable_value(&mut self.selected, index, *label);
                    }
                    if ui.button("Copy report").clicked() { ui.ctx().copy_text(self.image.to_string()); }
                });
                ui.separator();
                match self.selected {
                    0 => egui::ScrollArea::vertical().show(ui, |ui| {
                        let image = &self.image;
                        ui.heading(format!("PE{} {}", image.bits, image.architecture()));
                        ui.monospace(format!("Machine       0x{:04X}\nFile size     {} bytes\nImage base    0x{:X}\nEntry RVA     0x{:X}\nImage size    0x{:X}\nHeaders       0x{:X}\nSubsystem     {}\nDLL           {}", image.machine, image.file_size, image.image_base, image.entry_rva, image.image_size, image.headers_size, image.subsystem, image.is_dll()));
                        ui.add_space(12.0);
                        ui.label(format!("NX-compatible flag: {}", image.nx_compatible()));
                        ui.label(format!("DynamicBase flag: {} · relocation directory: {}", image.dynamic_base(), image.relocations_declared));
                        ui.label(format!("CFG flag: {}", image.cfg_declared()));
                        ui.label(format!("Certificate directory: {} · signature has not been verified", image.certificate_declared));
                        for warning in &image.warnings { ui.colored_label(Color32::from_rgb(235, 189, 124), warning); }
                    }).inner,
                    1 => egui::ScrollArea::both().show(ui, |ui| {
                        egui::Grid::new("pe-sections").striped(true).spacing([18.0, 10.0]).show(ui, |ui| {
                            for label in ["Section", "RVA", "Virtual size", "File offset", "Raw size", "Access"] { ui.strong(label); }
                            ui.end_row();
                            for section in &self.image.sections {
                                if ui.selectable_label(false, &section.name).clicked() && section.raw_size != 0 {
                                    self.offset = section.raw_offset as usize;
                                    self.selected = 3;
                                }
                                for value in [section.rva, section.virtual_size, section.raw_offset, section.raw_size] { ui.monospace(format!("0x{value:X}")); }
                                ui.monospace(section.permissions()); ui.end_row();
                            }
                        });
                    }).inner,
                    3 => self.show_bytes(ui),
                    _ => {
                        ui.horizontal(|ui| { ui.label("Filter"); ui.text_edit_singleline(&mut self.filter); });
                        let query = self.filter.to_lowercase();
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for library in &self.image.imports {
                                let library_match = library.name.to_lowercase().contains(&query);
                                let visible: Vec<_> = library.symbols.iter().filter(|s| library_match || s.to_string().to_lowercase().contains(&query)).collect();
                                if !library_match && visible.is_empty() { continue; }
                                ui.collapsing(format!("{} ({})", library.name, visible.len()), |ui| {
                                    for symbol in visible { ui.monospace(symbol.to_string()); }
                                });
                            }
                            if self.image.imports.is_empty() { ui.label("No normal imports declared. Delay imports are not inspected yet."); }
                        });
                    }
                }
            });
        open
    }
}

fn read_integer(bytes: &[u8], offset: usize, width: usize) -> Option<(u64, u64)> {
    if ![1, 2, 4, 8].contains(&width) {
        return None;
    }
    let data = bytes.get(offset..offset.checked_add(width)?)?;
    let mut le = 0u64;
    let mut be = 0u64;
    for (index, &byte) in data.iter().enumerate() {
        le |= u64::from(byte) << (index * 8);
        be = (be << 8) | u64::from(byte);
    }
    Some((le, be))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn integer_reads_check_bounds_and_endianness() {
        assert_eq!(
            read_integer(&[0x12, 0x34, 0x56], 1, 2),
            Some((0x5634, 0x3456))
        );
        assert_eq!(read_integer(&[255; 8], 0, 8), Some((u64::MAX, u64::MAX)));
        assert_eq!(read_integer(&[1], 0, 2), None);
        assert_eq!(read_integer(&[1], usize::MAX, 8), None);
        assert_eq!(read_integer(&[1], 0, 3), None);
        assert_eq!(read_integer(&[], 0, 1), None);
    }
    #[test]
    fn renders_headers_sections_and_filtered_imports() {
        let bytes = aegis_lang::compile("fn main() {}")
            .expect("sample")
            .executable;
        let image = pe::inspect(&bytes).expect("PE");
        let mut inspector = Inspector::new(PathBuf::from("sample.exe"), image, bytes);
        let ctx = egui::Context::default();
        for selected in 0..4 {
            inspector.selected = selected;
            inspector.filter = "write".into();
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                assert!(inspector.show(ctx));
            });
            assert!(!output.shapes.is_empty());
        }
    }
}
