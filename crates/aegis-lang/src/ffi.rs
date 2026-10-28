//! Static FFI metadata. Validation never reads or loads a library.
pub(crate) const MAX_EXTERNALS: usize = 64;
pub(crate) const SLOT_BASE: usize = 0x200;

#[derive(Debug, Clone)]
pub(crate) struct External {
    pub library: String,
    pub symbol: String,
    pub flags: u32,
}

impl External {
    pub fn new(library: String, symbol: String) -> Result<Self, &'static str> {
        if library.is_empty()
            || library.encode_utf16().count() > 1024
            || library.chars().any(|c| c.is_control())
            || !library.to_ascii_lowercase().ends_with(".dll")
        {
            return Err(
                "external library must be a .dll name or absolute Windows path (up to 1024 UTF-16 units)",
            );
        }
        let bare = library
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
        if library.contains('/') || (bare && library.len() > 255) {
            return Err(
                "DLL names must use backslashes for paths and bare names are limited to 255 bytes",
            );
        }
        let bytes = library.as_bytes();
        let drive = bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && bytes[2] == b'\\';
        let unc = library.starts_with("\\\\")
            && !library.starts_with("\\\\.\\")
            && !library.starts_with("\\\\?\\")
            && library.split('\\').filter(|part| !part.is_empty()).count() >= 3;
        if !bare && !drive && !unc {
            return Err(
                "external library requires a bare System32 DLL name or an absolute Windows path using backslashes",
            );
        }
        if symbol.is_empty()
            || symbol.len() > 255
            || !symbol.bytes().all(|b| (0x21..=0x7e).contains(&b))
        {
            return Err("external symbol must contain 1..255 printable ASCII bytes without spaces");
        }
        Ok(Self {
            library,
            symbol,
            flags: if bare { 0x800 } else { 0x900 },
        })
    }
}
