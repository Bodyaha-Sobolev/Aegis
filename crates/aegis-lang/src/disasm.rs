//! Replaceable, bounded adapter over the vendored iced-x86 decoder/formatter.
//! Linear decoding is not proof of code boundaries, control flow or safe execution.
use crate::pe;
use iced_x86::{Decoder, DecoderOptions, Formatter, IntelFormatter, OpKind};
use std::fmt;

pub const MAX_BYTES: usize = 65536;
pub const MAX_INSTRUCTIONS: usize = 4096;
pub fn parse_rva(text: &str) -> Result<u32, Error> {
    let text = text.trim();
    let value = if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16)
    } else {
        text.parse()
    };
    value.map_err(|_| Error("RVA must be a u32 decimal or 0x hexadecimal value".into()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Disassembly: {}", self.0)
    }
}
impl std::error::Error for Error {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub address: u64,
    pub offset: usize,
    pub bytes: Vec<u8>,
    pub text: String,
    pub invalid: bool,
    pub branch_target: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub rows: Vec<Row>,
    pub consumed: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub rva: u32,
    pub file_offset: usize,
    pub listing: Listing,
    pub next_rva: Option<u32>,
}
impl fmt::Display for Page {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Linear disassembly · RVA 0x{:X} · file 0x{:X}",
            self.rva, self.file_offset
        )?;
        for row in &self.listing.rows {
            let hex: String = row.bytes.iter().map(|b| format!("{b:02X} ")).collect();
            writeln!(
                f,
                "{:016X}  file {:08X}  {hex:45} {}{}",
                row.address,
                self.file_offset + row.offset,
                row.text,
                if row.invalid { " [invalid]" } else { "" }
            )?;
        }
        if let Some(next) = self.next_rva {
            writeln!(f, "Next RVA: 0x{next:X}")?;
        }
        Ok(())
    }
}

pub fn decode(bytes: &[u8], bits: u8, address: u64, limit: usize) -> Result<Listing, Error> {
    if !matches!(bits, 32 | 64) {
        return Err(Error(
            "only x86/x64 (32/64 bit) decoding is supported".into(),
        ));
    }
    if bytes.len() > MAX_BYTES || !(1..=MAX_INSTRUCTIONS).contains(&limit) {
        return Err(Error(
            "limit exceeded (65536 bytes, 1..4096 instructions)".into(),
        ));
    }
    address
        .checked_add(bytes.len() as u64)
        .ok_or_else(|| Error("address range overflow".into()))?;
    let mut decoder = Decoder::with_ip(u32::from(bits), bytes, address, DecoderOptions::NONE);
    let mut formatter = IntelFormatter::new();
    let mut rows = Vec::new();
    while decoder.can_decode() && rows.len() < limit {
        let offset = decoder.position();
        let instruction = decoder.decode();
        let end = decoder.position();
        if end <= offset || end > bytes.len() {
            return Err(Error("decoder did not advance within the input".into()));
        }
        let invalid = instruction.is_invalid();
        let mut text = String::new();
        formatter.format(&instruction, &mut text);
        if text.len() > 1024 {
            return Err(Error("formatted instruction exceeds 1024 bytes".into()));
        }
        let branch_target = if !invalid
            && matches!(
                instruction.op0_kind(),
                OpKind::NearBranch16 | OpKind::NearBranch32 | OpKind::NearBranch64
            ) {
            Some(instruction.near_branch_target())
        } else {
            None
        };
        rows.push(Row {
            address: instruction.ip(),
            offset,
            bytes: bytes[offset..end].to_vec(),
            text,
            invalid,
            branch_target,
        });
    }
    Ok(Listing {
        rows,
        consumed: decoder.position(),
    })
}

pub fn pe_page(image: &pe::Image, bytes: &[u8], rva: u32, limit: usize) -> Result<Page, Error> {
    if !matches!((image.machine, image.bits), (0x14c, 32) | (0x8664, 64)) {
        return Err(Error("PE machine must be x86 or x64".into()));
    }
    if bytes.len() != image.file_size {
        return Err(Error(
            "PE byte snapshot size does not match metadata".into(),
        ));
    }
    let range = image.file_range(rva).map_err(|e| Error(e.to_string()))?;
    let end = range.start + range.len().min(MAX_BYTES);
    let input = bytes
        .get(range.start..end)
        .ok_or_else(|| Error("mapped byte range is outside the snapshot".into()))?;
    let address = image
        .image_base
        .checked_add(u64::from(rva))
        .ok_or_else(|| Error("image address overflow".into()))?;
    let listing = decode(input, image.bits, address, limit)?;
    let next_rva = if listing.consumed < range.len() {
        rva.checked_add(listing.consumed as u32)
    } else {
        None
    };
    Ok(Page {
        rva,
        file_offset: range.start,
        listing,
        next_rva,
    })
}
