use aegis_lang::{compile, disasm, pe};

#[test]
fn decodes_x86_x64_branches_and_marks_invalid_bytes() {
    let bytes = [0x55, 0x48, 0x89, 0xe5, 0xe8, 1, 0, 0, 0, 0xc3, 0x90];
    let page = disasm::decode(&bytes, 64, 0x140001000, 64).expect("decode");
    assert_eq!(page.consumed, bytes.len());
    assert_eq!(page.rows.len(), 5);
    assert!(page.rows[0].text.starts_with("push"));
    assert_eq!(page.rows[1].bytes, [0x48, 0x89, 0xe5]);
    assert_eq!(page.rows[2].address, 0x140001004);
    assert_eq!(page.rows[2].branch_target, Some(0x14000100a));
    assert!(page.rows.iter().all(|r| !r.invalid));
    let bad = disasm::decode(&[0x0f], 64, 0, 1).expect("truncated instruction");
    assert!(bad.rows[0].invalid);
    assert_eq!(bad.consumed, 1);
    let x86 = disasm::decode(&[0x48, 0xc3], 32, 0, 2).expect("x86");
    assert_eq!(x86.rows.len(), 2);
    assert!(x86.rows[0].text.starts_with("dec"));
    assert_eq!(
        disasm::decode(&[0x48, 0xc3], 64, 0, 2)
            .expect("x64")
            .rows
            .len(),
        1
    );
}

#[test]
fn page_mapping_uses_snapshot_offsets_and_rejects_unsupported_images() {
    let bytes = compile("fn main() { printi(42); }")
        .expect("compile")
        .executable;
    let mut image = pe::inspect(&bytes).expect("PE");
    let page = disasm::pe_page(&image, &bytes, image.entry_rva, 3).expect("page");
    assert_eq!(page.file_offset, 0x400);
    assert_eq!(page.listing.rows.len(), 3);
    assert_eq!(
        page.listing.rows[0].address,
        image.image_base + u64::from(image.entry_rva)
    );
    assert_eq!(
        page.next_rva,
        Some(image.entry_rva + page.listing.consumed as u32)
    );
    assert!(page.to_string().contains("file 00000400"));
    assert!(disasm::pe_page(&image, &bytes[..bytes.len() - 1], image.entry_rva, 3).is_err());
    assert!(disasm::pe_page(&image, &bytes, u32::MAX, 3).is_err());
    image.machine = 0xaa64;
    assert!(disasm::pe_page(&image, &bytes, image.entry_rva, 3).is_err());
}

#[test]
fn decoding_budgets_and_rva_input_are_bounded() {
    assert!(disasm::decode(&[], 16, 0, 1).is_err());
    assert!(disasm::decode(&[], 64, 0, 0).is_err());
    assert!(disasm::decode(&[], 64, 0, 4097).is_err());
    assert!(disasm::decode(&[0x90], 64, u64::MAX, 1).is_err());
    assert!(disasm::decode(&vec![0x90; 65537], 64, 0, 1).is_err());
    let listing = disasm::decode(&vec![0x90; 65536], 64, 0, 4096).expect("budget");
    assert_eq!(listing.rows.len(), 4096);
    assert_eq!(listing.consumed, 4096);
    for (text, value) in [
        ("0x1000", 4096),
        ("4096", 4096),
        (" 0XFF ", 255),
        ("4294967295", u32::MAX),
    ] {
        assert_eq!(disasm::parse_rva(text).expect("RVA"), value);
    }
    for text in ["", "0x", "-1", "0x100000000", "4294967296", "1.0"] {
        assert!(disasm::parse_rva(text).is_err());
    }
}

#[test]
fn random_and_prefix_heavy_inputs_always_advance_inside_the_snapshot() {
    let mut state = 0x51a62837u32;
    for bits in [32, 64] {
        for round in 0..2000 {
            let mut bytes = vec![0; round % 128];
            for b in &mut bytes {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                *b = state as u8;
            }
            let listing = disasm::decode(&bytes, bits, 0x1000, 128).expect("arbitrary bytes");
            assert_eq!(listing.consumed, bytes.len());
            let mut offset = 0;
            for row in listing.rows {
                assert_eq!(row.offset, offset);
                assert_eq!(row.address, 0x1000 + offset as u64);
                assert!((1..=15).contains(&row.bytes.len()));
                assert_eq!(row.bytes, bytes[offset..offset + row.bytes.len()]);
                offset += row.bytes.len();
            }
            assert_eq!(offset, bytes.len());
        }
        let prefixes = disasm::decode(&[0x66; 128], bits, 0, 128).expect("prefixes");
        assert_eq!(prefixes.consumed, 128);
    }
}
