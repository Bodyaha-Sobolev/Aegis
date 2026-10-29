use std::{
    fs::File,
    io::{self, Read, Write},
    path::Path,
};

/// Size is enforced while reading, including if the file grows after metadata.
pub fn read_bounded(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file exceeds size limit",
        ));
    }
    Ok(bytes)
}

/// Reads a bounded UTF-8 source/document without accepting invalid text.
pub fn read_text_bounded(path: &Path, limit: usize) -> io::Result<String> {
    let bytes = read_bounded(path, limit)?;
    String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "file is not UTF-8"))
}

/// A failed write or rename leaves the previous destination intact.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// Atomically replaces a UTF-8 text file and refuses an oversized document.
pub fn atomic_write_text(path: &Path, text: &str, limit: usize) -> io::Result<()> {
    if text.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "text exceeds size limit",
        ));
    }
    atomic_write(path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_reads_and_atomic_replacement() {
        let dir = tempfile::tempdir().expect("temp directory");
        let path = dir.path().join("пример.ae");
        atomic_write(&path, b"first").expect("write");
        assert!(read_bounded(&path, 4).is_err());
        atomic_write(&path, b"second").expect("replace");
        assert_eq!(read_bounded(&path, 6).expect("read"), b"second");
        assert_eq!(std::fs::read_dir(dir.path()).expect("entries").count(), 1);
    }

    #[test]
    fn text_helpers_reject_invalid_utf8_and_oversize_without_replacing() {
        let dir = tempfile::tempdir().expect("temp directory");
        let path = dir.path().join("text.ae");
        atomic_write_text(&path, "старый", 32).expect("write");
        assert_eq!(read_text_bounded(&path, 32).expect("read"), "старый");
        assert!(atomic_write_text(&path, "слишком длинный", 2).is_err());
        assert_eq!(read_text_bounded(&path, 32).expect("unchanged"), "старый");
        atomic_write(&path, &[0xff]).expect("invalid fixture");
        assert!(read_text_bounded(&path, 32).is_err());
    }
}
