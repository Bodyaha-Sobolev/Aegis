use aegis_lang::{compile, pe};

#[test]
fn generated_images_have_consistent_relocation_and_permission_metadata() {
    for source in ["fn main() {}", "fn main() { println(\"Привет🙂\"); }"] {
        let bytes = compile(source).expect("compile").executable;
        let image = pe::inspect(&bytes).expect("valid image");
        assert!(image.dynamic_base() && image.nx_compatible());
        assert!(image.high_entropy_va());
        assert_eq!(image.dll_characteristics & 0x20, 0x20); // HighEntropyVA.
        assert_eq!(image.characteristics & 1, 0); // Not RELOCS_STRIPPED.
        assert_eq!(image.characteristics & 0x20, 0x20); // LARGE_ADDRESS_AWARE.
        assert!(image.relocations_declared);
        assert!(image.warnings.is_empty(), "{:?}", image.warnings);
        assert!(
            image
                .sections
                .iter()
                .all(|s| !s.executable() || !s.writable())
        );
        let read32 = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().expect("u32"));
        let read64 = |at| u64::from_le_bytes(bytes[at..at + 8].try_into().expect("u64"));
        assert_eq!(read64(0x98 + 0x18), 0x140000000);
        let header = 0x188 + 3 * 40;
        assert_eq!(&bytes[header..header + 8], b".reloc\0\0");
        let raw = read32(header + 20) as usize;
        let rva = read32(header + 12);
        assert_eq!(read32(0x98 + 0x98), rva);
        assert_eq!(read32(0x98 + 0x9c), 12);
        assert_eq!(read32(header + 8), 12);
        assert_eq!(read32(header + 16), 512);
        assert_eq!(read32(header + 36), 0x42000040); // Read, discardable; no W/X.
        assert_eq!(raw % 512, 0);
        assert_eq!(rva % 4096, 0);
        assert_eq!(read32(0x98 + 0x38), rva + 4096);
        assert_eq!(bytes.len(), raw + 512);
        assert_eq!(read32(raw), 0x1000);
        assert_eq!(read32(raw + 4), 12);
        // ABSOLUTE padding: the unbound IAT holds RVAs the loader resolves,
        // so no DIR64 fixups exist and none may be fabricated for it.
        assert!(bytes[raw + 8..].iter().all(|b| *b == 0));
    }
}

#[test]
fn relocation_section_follows_large_string_data_without_overlap() {
    let text = "Ж🙂x".repeat(10000);
    let bytes = compile(&format!("fn main() {{ println(\"{text}\"); }}"))
        .expect("large image")
        .executable;
    let image = pe::inspect(&bytes).expect("large PE");
    let read32 = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().expect("u32"));
    let data_header = 0x188 + 2 * 40;
    let reloc_header = 0x188 + 3 * 40;
    let data_end = read32(data_header + 12) + read32(data_header + 8);
    assert_eq!(read32(reloc_header + 12), data_end.next_multiple_of(4096));
    assert_eq!(
        read32(reloc_header + 20),
        read32(data_header + 20) + read32(data_header + 16)
    );
    assert_eq!(image.sections.len(), 4);
    assert!(image.warnings.is_empty());
}
