use aegis_lang::pe;

fn set16(b: &mut [u8], at: usize, value: u16) {
    b[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn set32(b: &mut [u8], at: usize, value: u32) {
    b[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn fixture(bits: u8) -> Vec<u8> {
    let mut b = vec![0; 0xe00];
    b[..2].copy_from_slice(b"MZ");
    set32(&mut b, 0x3c, 0x80);
    b[0x80..0x84].copy_from_slice(b"PE\0\0");
    set16(&mut b, 0x84, if bits == 64 { 0x8664 } else { 0x14c });
    set16(&mut b, 0x86, 2);
    set16(&mut b, 0x94, if bits == 64 { 0xf0 } else { 0xe0 });
    set16(&mut b, 0x96, 0x2002);
    set16(&mut b, 0x98, if bits == 64 { 0x20b } else { 0x10b });
    if bits == 64 {
        b[0xb0..0xb8].copy_from_slice(&0x140000000u64.to_le_bytes());
    } else {
        set32(&mut b, 0xb4, 0x400000);
    }
    set32(&mut b, 0x98 + 56, 0x3000);
    set32(&mut b, 0x98 + 60, 0x400);
    let count = if bits == 64 { 108 } else { 92 };
    let dir = if bits == 64 { 112 } else { 96 };
    set32(&mut b, 0x98 + count, 16);
    set32(&mut b, 0x98 + dir, 0x1000);
    set32(&mut b, 0x98 + dir + 4, 0x300);
    let sections = if bits == 64 { 0x188 } else { 0x178 };
    for (at, name, rva, raw, size, virtual_size, flags) in [
        (
            sections,
            b".edata\0\0",
            0x1000,
            0x400,
            0x800,
            0x800,
            0x40000040,
        ),
        (
            sections + 40,
            b".text\0\0\0",
            0x2000,
            0xc00,
            0x200,
            0x400,
            0x60000020,
        ),
    ] {
        b[at..at + 8].copy_from_slice(name);
        for (offset, value) in [
            (8, virtual_size),
            (12, rva),
            (16, size),
            (20, raw),
            (36, flags),
        ] {
            set32(&mut b, at + offset, value);
        }
    }
    for (offset, value) in [
        (12, 0x1160),
        (16, 7),
        (20, 5),
        (24, 3),
        (28, 0x1100),
        (32, 0x1120),
        (36, 0x1140),
    ] {
        set32(&mut b, 0x400 + offset, value);
    }
    for (index, rva) in [0x2000, 0, 0x1210, 0x2100, 0x2200].into_iter().enumerate() {
        set32(&mut b, 0x500 + 4 * index, rva);
    }
    for (index, rva) in [0x1180, 0x1190, 0x11a0].into_iter().enumerate() {
        set32(&mut b, 0x520 + 4 * index, rva);
    }
    set16(&mut b, 0x544, 2);
    for (offset, text) in [
        (0x560, &b"fixture.dll\0"[..]),
        (0x580, &b"alias\0"[..]),
        (0x590, &b"alpha\0"[..]),
        (0x5a0, &b"forward\0"[..]),
        (0x610, &b"OTHER.#27\0"[..]),
    ] {
        b[offset..offset + text.len()].copy_from_slice(text);
    }
    b[0xc00] = 0xc3;
    b
}

#[test]
fn pe32_and_pe64_exports_preserve_aliases_ordinals_holes_and_forwarders() {
    for bits in [32, 64] {
        let b = fixture(bits);
        let original = b.clone();
        let image = pe::inspect(&b).expect("exports");
        assert_eq!(b, original);
        assert_eq!(image.bits, bits);
        assert!(image.imports.is_empty());
        assert_eq!(image.export_library.as_deref(), Some("fixture.dll"));
        assert_eq!(image.exports.len(), 4);
        assert_eq!(
            image.exports[0],
            pe::Export {
                ordinal: 7,
                names: vec!["alias".into(), "alpha".into()],
                rva: 0x2000,
                forwarder: None,
                file_offset: Some(0xc00)
            }
        );
        assert_eq!(image.exports[1].ordinal, 9);
        assert_eq!(image.exports[1].forwarder.as_deref(), Some("OTHER.#27"));
        assert_eq!(image.exports[1].file_offset, Some(0x610));
        assert_eq!(image.exports[2].ordinal, 10);
        assert!(image.exports[2].names.is_empty());
        assert_eq!(image.exports[2].file_offset, Some(0xd00));
        assert_eq!(image.exports[3].ordinal, 11);
        assert_eq!(image.exports[3].file_offset, None);
        let report = image.to_string();
        for text in [
            "Exports (4)",
            "fixture.dll",
            "ordinal #7",
            "alias",
            "alpha",
            "forwarder OTHER.#27",
            "no unique file mapping",
        ] {
            assert!(report.contains(text), "{text}");
        }
    }
}

#[test]
fn export_tables_can_be_separate_and_forwarders_end_at_the_directory_boundary() {
    let mut b = fixture(64);
    b.copy_within(0x500..0x514, 0xc30);
    b.copy_within(0x520..0x52c, 0xc70);
    b.copy_within(0x540..0x546, 0xc90);
    set32(&mut b, 0x400 + 28, 0x2030);
    set32(&mut b, 0x400 + 32, 0x2070);
    set32(&mut b, 0x400 + 36, 0x2090);
    assert_eq!(
        pe::inspect(&b).expect("separate tables").exports[0]
            .names
            .len(),
        2
    );
    set32(&mut b, 0x98 + 112 + 4, 0x218); // Cuts the NUL off OTHER.#27.
    assert!(
        pe::inspect(&b)
            .expect_err("forwarder must terminate inside directory")
            .0
            .contains("unterminated")
    );
    set32(&mut b, 0x98 + 112 + 4, 40);
    assert!(
        pe::inspect(&b).expect("now ordinary data RVA").exports[1]
            .forwarder
            .is_none()
    );
}

#[test]
fn rejects_invalid_export_ranges_tables_ordinals_and_strings() {
    for (at, value) in [
        (0x98 + 112, 0),
        (0x98 + 116, 39),
        (0x98 + 116, u32::MAX),
        (0x400 + 16, u32::MAX),
        (0x400 + 20, 65537),
        (0x400 + 24, 65537),
        (0x400 + 28, 0),
        (0x400 + 28, 0x27ff),
        (0x400 + 32, 0),
        (0x400 + 36, 0),
        (0x520, 0),
        (0x520, 0x27ff),
        (0x544, 5),
        (0x544, 1),
    ] {
        let mut b = fixture(64);
        set32(&mut b, at, value);
        assert!(pe::inspect(&b).is_err(), "mutation {at:X} -> {value:X}");
    }
    let mut b = fixture(64);
    b[0x580..0x580 + 513].fill(b'a');
    assert!(pe::inspect(&b).is_err());
    let mut b = fixture(64);
    b[0x580] = 0x1b;
    assert!(pe::inspect(&b).expect("escaped").exports[0].names[0].starts_with("\\x1B"));
}

#[test]
fn export_address_and_name_budgets_are_checked_before_unbounded_work() {
    let mut b = fixture(64);
    b.resize(0x44800, 0);
    set32(&mut b, 0x98 + 56, 0x48000);
    set32(&mut b, 0x188 + 8, 0x44000);
    set32(&mut b, 0x188 + 16, 0x44000);
    set32(&mut b, 0x188 + 40 + 12, 0x47000);
    set32(&mut b, 0x188 + 40 + 20, 0x44600);
    set32(&mut b, 0x400 + 20, 65536);
    set32(&mut b, 0x400 + 24, 0);
    set32(&mut b, 0x400 + 28, 0x2000);
    set32(&mut b, 0x400 + 16, u32::MAX - 65535);
    assert!(
        pe::inspect(&b)
            .expect("65536 address slots, all holes")
            .exports
            .is_empty()
    );
    set32(&mut b, 0x400 + 20, 65537);
    assert!(pe::inspect(&b).expect_err("65537").0.contains("limit"));
    let mut b = fixture(64);
    b.resize(0x10600, 0);
    set32(&mut b, 0x98 + 56, 0x13000);
    set32(&mut b, 0x188 + 8, 0x10000);
    set32(&mut b, 0x188 + 16, 0x10000);
    set32(&mut b, 0x188 + 40 + 12, 0x12000);
    set32(&mut b, 0x188 + 40 + 20, 0x10400);
    set32(&mut b, 0x400 + 20, 1);
    set32(&mut b, 0x500, 0x12000);
    set32(&mut b, 0x400 + 24, 2050);
    set32(&mut b, 0x400 + 32, 0x2000);
    set32(&mut b, 0x400 + 36, 0x5000);
    for i in 0..2050 {
        set32(&mut b, 0x1400 + i * 4, 0x8000);
    }
    b[0x4400..0x4400 + 4100].fill(0);
    b[0x7400..0x7600].fill(b'a');
    assert!(
        pe::inspect(&b)
            .expect_err("shared text budget")
            .0
            .contains("name size limit")
    );
    let mut b = fixture(64);
    set32(&mut b, 0x400 + 12, 0);
    set32(&mut b, 0x400 + 24, 0);
    for slot in 0..5 {
        set32(&mut b, 0x500 + slot * 4, 0x2000);
    }
    set32(&mut b, 0x188 + 40 + 12, 0x1108);
    assert!(
        pe::inspect(&b)
            .expect_err("table starts uniquely but crosses an overlapping section")
            .0
            .contains("crosses raw region")
    );
}

#[test]
fn many_aliases_do_not_amplify_forwarder_text_in_reports() {
    let mut b = fixture(64);
    b.resize(0xa0600, 0);
    set32(&mut b, 0x98 + 56, 0xa3000);
    set32(&mut b, 0x98 + 116, 0x1000);
    set32(&mut b, 0x188 + 8, 0xa0000);
    set32(&mut b, 0x188 + 16, 0xa0000);
    set32(&mut b, 0x188 + 40 + 12, 0xa2000);
    set32(&mut b, 0x188 + 40 + 20, 0xa0400);
    set32(&mut b, 0x400 + 20, 1);
    set32(&mut b, 0x400 + 24, 65536);
    set32(&mut b, 0x500, 0x1210);
    set32(&mut b, 0x400 + 32, 0x2000);
    set32(&mut b, 0x400 + 36, 0x43000);
    for i in 0..65536 {
        set32(&mut b, 0x1400 + i * 4, 0x1180);
    }
    b[0x610..0x810].fill(b'x');
    let image = pe::inspect(&b).expect("65536 aliases");
    assert_eq!(image.exports[0].names.len(), 65536);
    assert!(
        image.to_string().len() < 2 * 1024 * 1024,
        "report must not multiply long target text by alias count"
    );
}

#[test]
fn truncated_and_mutated_export_images_never_panic() {
    for bits in [32, 64] {
        let original = fixture(bits);
        for len in 0..original.len() {
            let _ = pe::inspect(&original[..len]);
        }
        let mut state = 0x87654321u32;
        for _ in 0..2000 {
            let mut b = original.clone();
            for _ in 0..8 {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let at = state as usize % b.len();
                b[at] = (state >> 8) as u8;
            }
            let _ = pe::inspect(&b);
        }
    }
}
