//! Bounded, read-only PE inspection. Never loads a library or executes input.
//! Layout reference: https://learn.microsoft.com/windows/win32/debug/pe-format
use std::{fmt, ops::Range};

pub const MAX_FILE: usize = 64 * 1024 * 1024;
const MAX_SECTIONS: usize = 96;
const MAX_LIBRARIES: usize = 128;
const MAX_SYMBOLS: usize = 4096;
const MAX_NAME: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PE: {}", self.0)
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub rva: u32,
    pub virtual_size: u32,
    pub raw_offset: u32,
    pub raw_size: u32,
    pub characteristics: u32,
}
impl Section {
    pub fn readable(&self) -> bool {
        self.characteristics & 0x40000000 != 0
    }
    pub fn writable(&self) -> bool {
        self.characteristics & 0x80000000 != 0
    }
    pub fn executable(&self) -> bool {
        self.characteristics & 0x20000000 != 0
    }
    pub fn permissions(&self) -> String {
        [
            if self.readable() { 'R' } else { '-' },
            if self.writable() { 'W' } else { '-' },
            if self.executable() { 'X' } else { '-' },
        ]
        .iter()
        .collect()
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Symbol {
    Name(String),
    Ordinal(u16),
}
impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name(name) => f.write_str(name),
            Self::Ordinal(n) => write!(f, "ordinal #{n}"),
        }
    }
}
#[derive(Debug, Clone)]
pub struct Library {
    pub name: String,
    pub symbols: Vec<Symbol>,
}
#[derive(Debug, Clone)]
pub struct Image {
    pub file_size: usize,
    pub bits: u8,
    pub machine: u16,
    pub image_base: u64,
    pub entry_rva: u32,
    pub image_size: u32,
    pub headers_size: u32,
    pub subsystem: u16,
    pub characteristics: u16,
    pub dll_characteristics: u16,
    pub relocations_declared: bool,
    pub certificate_declared: bool,
    pub sections: Vec<Section>,
    pub imports: Vec<Library>,
    pub warnings: Vec<String>,
}
impl Image {
    pub fn architecture(&self) -> &'static str {
        match self.machine {
            0x14c => "x86",
            0x8664 => "x64",
            0xaa64 => "ARM64",
            0xa641 => "ARM64EC",
            _ => "unknown machine",
        }
    }
    pub fn is_dll(&self) -> bool {
        self.characteristics & 0x2000 != 0
    }
    pub fn nx_compatible(&self) -> bool {
        self.dll_characteristics & 0x100 != 0
    }
    pub fn dynamic_base(&self) -> bool {
        self.dll_characteristics & 0x40 != 0
    }
    pub fn cfg_declared(&self) -> bool {
        self.dll_characteristics & 0x4000 != 0
    }
}
impl fmt::Display for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "PE{} {} (machine 0x{:04X}) {}",
            self.bits,
            self.architecture(),
            self.machine,
            if self.is_dll() { "DLL" } else { "image" }
        )?;
        writeln!(
            f,
            "File: {} bytes | Image: 0x{:X} | Headers: 0x{:X}",
            self.file_size, self.image_size, self.headers_size
        )?;
        writeln!(
            f,
            "Image base: 0x{:X} | Entry RVA: 0x{:X} | Subsystem: {}",
            self.image_base, self.entry_rva, self.subsystem
        )?;
        writeln!(
            f,
            "Header flags: NX-compatible={} DynamicBase={} CFG={}",
            self.nx_compatible(),
            self.dynamic_base(),
            self.cfg_declared()
        )?;
        writeln!(
            f,
            "Relocations declared={} Certificate data declared={} (signature not verified)",
            self.relocations_declared, self.certificate_declared
        )?;
        writeln!(f, "\nSections ({})", self.sections.len())?;
        for s in &self.sections {
            writeln!(
                f,
                "  {:12} RVA 0x{:08X} virtual 0x{:X} raw 0x{:X}+0x{:X} {}",
                s.name,
                s.rva,
                s.virtual_size,
                s.raw_offset,
                s.raw_size,
                s.permissions()
            )?;
        }
        writeln!(f, "\nImports ({})", self.imports.len())?;
        for library in &self.imports {
            writeln!(f, "  {}", library.name)?;
            for symbol in &library.symbols {
                writeln!(f, "    {symbol}")?;
            }
        }
        for warning in &self.warnings {
            writeln!(f, "NOTE: {warning}")?;
        }
        Ok(())
    }
}

struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn bytes(&self, at: usize, len: usize) -> Result<&'a [u8]> {
        let end = at
            .checked_add(len)
            .ok_or_else(|| Error("file offset overflow".into()))?;
        self.0
            .get(at..end)
            .ok_or_else(|| Error(format!("truncated file range 0x{at:X}..0x{end:X}")))
    }
    fn u16(&self, at: usize) -> Result<u16> {
        Ok(u16::from_le_bytes(
            self.bytes(at, 2)?
                .try_into()
                .map_err(|_| Error("invalid u16".into()))?,
        ))
    }
    fn u32(&self, at: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.bytes(at, 4)?
                .try_into()
                .map_err(|_| Error("invalid u32".into()))?,
        ))
    }
    fn u64(&self, at: usize) -> Result<u64> {
        Ok(u64::from_le_bytes(
            self.bytes(at, 8)?
                .try_into()
                .map_err(|_| Error("invalid u64".into()))?,
        ))
    }
}
fn printable(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| {
            if (0x20..=0x7e).contains(&b) {
                char::from(b).to_string()
            } else {
                format!("\\x{b:02X}")
            }
        })
        .collect()
}
fn add_rva(rva: u32, delta: usize) -> Result<u32> {
    let delta = u32::try_from(delta).map_err(|_| Error("RVA displacement overflow".into()))?;
    rva.checked_add(delta)
        .ok_or_else(|| Error("RVA overflow".into()))
}
impl Image {
    fn mapped_range(&self, rva: u32) -> Result<Range<usize>> {
        let mut result = if rva < self.headers_size {
            Some(rva as usize..self.headers_size as usize)
        } else {
            None
        };
        for s in &self.sections {
            let delta = u64::from(rva).checked_sub(u64::from(s.rva));
            if let Some(delta) = delta
                && delta < u64::from(s.virtual_size.max(s.raw_size))
            {
                if result.is_some() {
                    return Err(Error(format!("ambiguous section mapping at RVA 0x{rva:X}")));
                }
                if delta >= u64::from(s.raw_size) {
                    return Err(Error(format!(
                        "RVA 0x{rva:X} refers to unbacked virtual data"
                    )));
                }
                let start = s.raw_offset as usize + delta as usize;
                result = Some(start..s.raw_offset as usize + s.raw_size as usize);
            }
        }
        result.ok_or_else(|| Error(format!("unmapped RVA 0x{rva:X}")))
    }
    fn rva_bytes<'a>(&self, reader: &Reader<'a>, rva: u32, len: usize) -> Result<&'a [u8]> {
        let range = self.mapped_range(rva)?;
        if len > range.len() {
            return Err(Error(format!("RVA range crosses raw region at 0x{rva:X}")));
        }
        reader.bytes(range.start, len)
    }
    fn name(&self, reader: &Reader<'_>, rva: u32, budget: &mut usize) -> Result<String> {
        let range = self.mapped_range(rva)?;
        let bytes = reader.bytes(range.start, range.len().min(MAX_NAME + 1))?;
        let length = bytes
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| Error("unterminated or oversized import name".into()))?;
        if length == 0 || length > MAX_NAME || length > *budget {
            return Err(Error("import name size limit exceeded".into()));
        }
        *budget -= length;
        Ok(printable(&bytes[..length]))
    }
}

/// Inspects PE32/PE32+ file bytes with bounded names/tables. Not a loader,
/// disassembler, signature verifier or proof that the program is safe to run.
pub fn inspect(bytes: &[u8]) -> Result<Image> {
    if bytes.len() > MAX_FILE {
        return Err(Error("file exceeds 64 MiB inspection limit".into()));
    }
    let r = Reader(bytes);
    if r.bytes(0, 2)? != b"MZ" {
        return Err(Error("missing DOS MZ signature".into()));
    }
    let pe = r.u32(0x3c)? as usize;
    if r.bytes(pe, 4)? != b"PE\0\0" {
        return Err(Error("missing PE signature".into()));
    }
    let coff = pe
        .checked_add(4)
        .ok_or_else(|| Error("header offset overflow".into()))?;
    r.bytes(coff, 20)?;
    let count = r.u16(coff + 2)? as usize;
    if !(1..=MAX_SECTIONS).contains(&count) {
        return Err(Error("section count must be in 1..96".into()));
    }
    let optional = coff + 20;
    let optional_size = r.u16(coff + 16)? as usize;
    r.bytes(optional, optional_size)?;
    let (bits, minimum, directory_offset, count_offset) = match r.u16(optional)? {
        0x10b => (32, 96, 96, 92),
        0x20b => (64, 112, 112, 108),
        _ => {
            return Err(Error(
                "unsupported optional header (expected PE32 or PE32+)".into(),
            ));
        }
    };
    if optional_size < minimum {
        return Err(Error("optional header is too short".into()));
    }
    let directory_count = r.u32(optional + count_offset)? as usize;
    if directory_count > (optional_size - directory_offset) / 8 {
        return Err(Error("data directories exceed optional header".into()));
    }
    let directory = |index: usize| -> Result<(u32, u32)> {
        if index >= directory_count {
            return Ok((0, 0));
        }
        let at = optional + directory_offset + index * 8;
        Ok((r.u32(at)?, r.u32(at + 4)?))
    };
    let section_table = optional + optional_size;
    r.bytes(section_table, count * 40)?;
    let mut image = Image {
        file_size: bytes.len(),
        bits,
        machine: r.u16(coff)?,
        image_base: if bits == 64 {
            r.u64(optional + 24)?
        } else {
            u64::from(r.u32(optional + 28)?)
        },
        entry_rva: r.u32(optional + 16)?,
        image_size: r.u32(optional + 56)?,
        headers_size: r.u32(optional + 60)?,
        subsystem: r.u16(optional + 68)?,
        dll_characteristics: r.u16(optional + 70)?,
        characteristics: r.u16(coff + 18)?,
        relocations_declared: directory(5)?.0 != 0 && directory(5)?.1 != 0,
        certificate_declared: directory(4)?.0 != 0 && directory(4)?.1 != 0,
        sections: Vec::with_capacity(count),
        imports: Vec::new(),
        warnings: Vec::new(),
    };
    if (image.headers_size as usize) < section_table + count * 40 {
        return Err(Error("SizeOfHeaders does not cover section table".into()));
    }
    r.bytes(0, image.headers_size as usize)?;
    for index in 0..count {
        let at = section_table + index * 40;
        let name = r.bytes(at, 8)?;
        let section = Section {
            name: printable(&name[..name.iter().position(|&b| b == 0).unwrap_or(8)]),
            virtual_size: r.u32(at + 8)?,
            rva: r.u32(at + 12)?,
            raw_size: r.u32(at + 16)?,
            raw_offset: r.u32(at + 20)?,
            characteristics: r.u32(at + 36)?,
        };
        if section.raw_size > 0 {
            if section.raw_offset < image.headers_size {
                return Err(Error("section raw data overlaps headers".into()));
            }
            r.bytes(section.raw_offset as usize, section.raw_size as usize)?;
        }
        let virtual_end =
            u64::from(section.rva) + u64::from(section.virtual_size.max(section.raw_size));
        if virtual_end > u64::from(u32::MAX) + 1 {
            return Err(Error("section virtual range overflows RVA space".into()));
        }
        if virtual_end > u64::from(image.image_size) {
            image.warnings.push(format!(
                "{} extends beyond declared SizeOfImage",
                section.name
            ));
        }
        if section.writable() && section.executable() {
            image.warnings.push(format!(
                "{} declares writable and executable memory",
                section.name
            ));
        }
        image.sections.push(section);
    }
    if image.entry_rva != 0
        && !image.sections.iter().any(|s| {
            s.executable()
                && u64::from(image.entry_rva) >= u64::from(s.rva)
                && u64::from(image.entry_rva)
                    < u64::from(s.rva) + u64::from(s.virtual_size.max(s.raw_size))
        })
    {
        image
            .warnings
            .push("entry point is outside declared executable sections".into());
    }
    if image.dynamic_base() && !image.relocations_declared {
        image
            .warnings
            .push("DynamicBase flag is set without a relocation directory".into());
    }
    let certificate = directory(4)?;
    if image.certificate_declared {
        r.bytes(certificate.0 as usize, certificate.1 as usize)?;
    }
    let imports = directory(1)?;
    if imports == (0, 0) {
        return Ok(image);
    }
    if imports.0 == 0 || imports.1 < 20 {
        return Err(Error("invalid import directory".into()));
    }
    image.rva_bytes(&r, imports.0, imports.1 as usize)?;
    let mut terminated = false;
    let mut symbol_count = 0;
    let mut name_budget = 1024 * 1024;
    for index in 0..(imports.1 as usize / 20).min(MAX_LIBRARIES + 1) {
        let descriptor = image.rva_bytes(&r, add_rva(imports.0, index * 20)?, 20)?;
        if descriptor.iter().all(|&b| b == 0) {
            terminated = true;
            break;
        }
        if image.imports.len() == MAX_LIBRARIES {
            return Err(Error("import library limit exceeded (128)".into()));
        }
        let descriptor = Reader(descriptor);
        let name_rva = descriptor.u32(12)?;
        if name_rva == 0 {
            return Err(Error("missing import library name RVA".into()));
        }
        let name = image.name(&r, name_rva, &mut name_budget)?;
        let lookup = match descriptor.u32(0)? {
            0 => descriptor.u32(16)?,
            value => value,
        };
        if lookup == 0 {
            return Err(Error("missing import lookup/IAT".into()));
        }
        let mut library = Library {
            name,
            symbols: Vec::new(),
        };
        for slot in 0..=MAX_SYMBOLS {
            let at = add_rva(lookup, slot * (bits as usize / 8))?;
            let thunk = Reader(image.rva_bytes(&r, at, bits as usize / 8)?);
            let value = if bits == 64 {
                thunk.u64(0)?
            } else {
                u64::from(thunk.u32(0)?)
            };
            if value == 0 {
                break;
            }
            if symbol_count == MAX_SYMBOLS || slot == MAX_SYMBOLS {
                return Err(Error("import symbol limit exceeded (4096)".into()));
            }
            let ordinal_flag = 1u64 << (bits - 1);
            let symbol = if value & ordinal_flag != 0 {
                if value & !(ordinal_flag | 0xffff) != 0 {
                    return Err(Error("reserved bits are set in ordinal import".into()));
                }
                Symbol::Ordinal(value as u16)
            } else {
                let address = u32::try_from(value)
                    .map_err(|_| Error("import name RVA exceeds 32 bits".into()))?;
                image.rva_bytes(&r, address, 2)?;
                Symbol::Name(image.name(&r, add_rva(address, 2)?, &mut name_budget)?)
            };
            library.symbols.push(symbol);
            symbol_count += 1;
        }
        image.imports.push(library);
    }
    if !terminated {
        return Err(Error("unterminated import descriptor table".into()));
    }
    Ok(image)
}
