use crate::disassembly;
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
    export_query: Option<String>,
    export_rows: Vec<(usize, Option<usize>)>,
    disassembly: disassembly::Panel,
}
impl Inspector {
    pub fn new(path: PathBuf, image: pe::Image, bytes: Vec<u8>) -> Self {
        let disassembly = disassembly::Panel::new(image.entry_rva);
        Self {
            path,
            image,
            selected: 0,
            filter: String::new(),
            bytes,
            offset: 0,
            export_query: None,
            export_rows: Vec::new(),
            disassembly,
        }
    }
    pub fn select_exports(&mut self) {
        self.selected = 4;
    }
    pub fn select_disassembly(&mut self) {
        self.selected = 5;
    }
    fn jump_to_bytes(&mut self, offset: usize) {
        if offset < self.bytes.len() {
            self.offset = offset;
            self.selected = 3;
        }
    }
    fn update_export_filter(&mut self) {
        let query = self.filter.to_lowercase();
        if self.export_query.as_ref() == Some(&query) {
            return;
        }
        self.export_rows.clear();
        let library_match = self
            .image
            .export_library
            .as_ref()
            .is_some_and(|name| !query.is_empty() && name.to_lowercase().contains(&query));
        for (index, export) in self.image.exports.iter().enumerate() {
            let common = query.is_empty()
                || library_match
                || export.ordinal.to_string().contains(&query)
                || format!("0x{:x}", export.rva).contains(&query)
                || export
                    .forwarder
                    .as_ref()
                    .is_some_and(|f| f.to_lowercase().contains(&query));
            if export.names.is_empty() {
                if common {
                    self.export_rows.push((index, None));
                }
            } else {
                for (name_index, name) in export.names.iter().enumerate() {
                    if common || name.to_lowercase().contains(&query) {
                        self.export_rows.push((index, Some(name_index)));
                    }
                }
            }
        }
        self.export_query = Some(query);
    }
    fn show_exports(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Filter name / ordinal / RVA / forwarder");
            ui.text_edit_singleline(&mut self.filter);
        });
        self.update_export_filter();
        ui.label(format!(
            "{} · {} exports · {} matching rows",
            self.image
                .export_library
                .as_deref()
                .unwrap_or("No DLL name"),
            self.image.exports.len(),
            self.export_rows.len()
        ));
        ui.label("Ordinal · RVA · name · forwarder · file bytes. Exported addresses can denote code or data.");
        if self.image.exports.is_empty() {
            ui.label("No exports declared.");
            return;
        }
        let height = ui.text_style_height(&egui::TextStyle::Monospace).max(22.0);
        let mut jump = None;
        let mut code_jump = None;
        egui::ScrollArea::both().id_salt("pe-exports").show_rows(
            ui,
            height,
            self.export_rows.len(),
            |ui, rows| {
                for row in rows {
                    let (index, name) = self.export_rows[row];
                    let export = &self.image.exports[index];
                    let name = name.map_or("(unnamed)", |n| export.names[n].as_str());
                    ui.horizontal(|ui| {
                        for (width, text) in [
                            (65.0, format!("#{}", export.ordinal)),
                            (110.0, format!("0x{:08X}", export.rva)),
                            (260.0, name.to_owned()),
                            (210.0, export.forwarder.as_deref().unwrap_or("—").to_owned()),
                        ] {
                            ui.add_sized(
                                [width, height],
                                egui::Label::new(RichText::new(&text).monospace()).truncate(),
                            )
                            .on_hover_text(text);
                        }
                        if let Some(offset) = export.file_offset {
                            if ui.button(format!("File 0x{offset:X}")).clicked() {
                                jump = Some(offset);
                            }
                        } else {
                            ui.label("No unique file mapping");
                        }
                        if export.forwarder.is_none()
                            && export.file_offset.is_some()
                            && ui.small_button("Disasm").clicked()
                        {
                            code_jump = Some(export.rva);
                        }
                    });
                }
            },
        );
        if let Some(offset) = jump {
            self.jump_to_bytes(offset);
        }
        if let Some(rva) = code_jump {
            self.disassembly.goto(rva, &self.image, &self.bytes);
            self.select_disassembly();
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
                    for (index, label) in [(0,"Headers"), (1,"Sections"), (2,"Imports"), (4,"Exports"), (5,"Disassembly"), (3,"Bytes")] {
                        ui.selectable_value(&mut self.selected, index, label);
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
                        ui.label(format!("HighEntropyVA flag (PE64): {}", image.high_entropy_va()));
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
                    4 => self.show_exports(ui),
                    5 => {if let Some(offset)=self.disassembly.show(ui,&self.image,&self.bytes) {self.jump_to_bytes(offset);}},
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
        for selected in 0..6 {
            inspector.selected = selected;
            inspector.filter = "write".into();
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                assert!(inspector.show(ctx));
            });
            assert!(!output.shapes.is_empty());
        }
    }
    #[test]
    fn export_filter_handles_aliases_and_navigation_preserves_the_snapshot() {
        let bytes = aegis_lang::compile("fn main() {}").expect("PE").executable;
        let mut image = pe::inspect(&bytes).expect("inspect");
        image.export_library = Some("fixture.dll".into());
        image.exports = vec![
            pe::Export {
                ordinal: 7,
                names: vec!["Alpha".into(), "Alias".into()],
                rva: 0x1000,
                forwarder: None,
                file_offset: Some(0x400),
            },
            pe::Export {
                ordinal: 8,
                names: vec![],
                rva: 0x2000,
                forwarder: Some("OTHER.#27".into()),
                file_offset: None,
            },
        ];
        let mut inspector = Inspector::new(PathBuf::from("fixture.dll"), image, bytes.clone());
        for (query, expected) in [
            ("", vec![(0, Some(0)), (0, Some(1)), (1, None)]),
            ("alias", vec![(0, Some(1))]),
            ("0x1000", vec![(0, Some(0)), (0, Some(1))]),
            ("other", vec![(1, None)]),
            ("8", vec![(1, None)]),
            ("nothing", vec![]),
        ] {
            inspector.filter = query.into();
            inspector.update_export_filter();
            assert_eq!(inspector.export_rows, expected);
            inspector.update_export_filter();
            assert_eq!(inspector.export_rows, expected);
        }
        inspector.filter = "fixture".into();
        inspector.select_exports();
        let ctx = egui::Context::default();
        assert!(
            !ctx.run(egui::RawInput::default(), |ctx| {
                assert!(inspector.show(ctx));
            })
            .shapes
            .is_empty()
        );
        inspector.jump_to_bytes(0x400);
        assert_eq!((inspector.selected, inspector.offset), (3, 0x400));
        inspector.jump_to_bytes(usize::MAX);
        assert_eq!(inspector.offset, 0x400);
        assert_eq!(inspector.bytes, bytes);
    }
}
