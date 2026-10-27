use aegis_lang::{
    compile,
    pe::{self, Symbol},
};

#[test]
fn inspects_native_compiler_output_without_mutating_it() {
    let build = compile("fn main() { println(\"hello\"); }").expect("compile");
    let original = build.executable.clone();
    let image = pe::inspect(&build.executable).expect("valid PE64");
    assert_eq!(build.executable, original);
    assert_eq!(image.bits, 64);
    assert_eq!(image.architecture(), "x64");
    assert_eq!(image.sections.len(), 3);
    assert!(image.nx_compatible());
    assert!(!image.dynamic_base());
    assert!(!image.sections[2].writable());
    assert!(!image.sections[2].executable());
    assert_eq!(image.imports[0].name, "KERNEL32.DLL");
    assert_eq!(
        image.imports[0].symbols,
        [
            Symbol::Name("GetStdHandle".into()),
            Symbol::Name("WriteFile".into()),
            Symbol::Name("ExitProcess".into())
        ]
    );
}
fn fixture32() -> Vec<u8> {
    let mut b = vec![0; 0x600];
    b[..2].copy_from_slice(b"MZ");
    let set16 =
        |b: &mut [u8], at: usize, value: u16| b[at..at + 2].copy_from_slice(&value.to_le_bytes());
    let set32 =
        |b: &mut [u8], at: usize, value: u32| b[at..at + 4].copy_from_slice(&value.to_le_bytes());
    set32(&mut b, 0x3c, 0x80);
    b[0x80..0x84].copy_from_slice(b"PE\0\0");
    set16(&mut b, 0x84, 0x14c);
    set16(&mut b, 0x86, 1);
    set16(&mut b, 0x94, 224);
    set16(&mut b, 0x98, 0x10b);
    set32(&mut b, 0x98 + 28, 0x400000);
    set32(&mut b, 0x98 + 56, 0x2000);
    set32(&mut b, 0x98 + 60, 0x200);
    set32(&mut b, 0x98 + 92, 16);
    set32(&mut b, 0x98 + 104, 0x1000);
    set32(&mut b, 0x98 + 108, 40);
    b[0x178..0x180].copy_from_slice(b".idata\0\0");
    for (at, value) in [
        (0x180, 0x400),
        (0x184, 0x1000),
        (0x188, 0x400),
        (0x18c, 0x200),
        (0x19c, 0x40000040),
        (0x200, 0x1040),
        (0x20c, 0x1060),
        (0x210, 0x1040),
        (0x240, 0x1080),
        (0x244, 0x80000007),
    ] {
        set32(&mut b, at, value);
    }
    b[0x260..0x266].copy_from_slice(b"A.dll\0");
    b[0x282..0x286].copy_from_slice(b"foo\0");
    b
}
#[test]
fn pe32_supports_names_ordinals_and_iat_fallback() {
    let mut b = fixture32();
    let image = pe::inspect(&b).expect("PE32");
    assert_eq!(image.bits, 32);
    assert_eq!(image.image_base, 0x400000);
    assert_eq!(
        image.imports[0].symbols,
        [Symbol::Name("foo".into()), Symbol::Ordinal(7)]
    );
    b[0x200..0x204].fill(0);
    assert_eq!(
        pe::inspect(&b).expect("IAT fallback").imports[0].symbols,
        image.imports[0].symbols
    );
    b[0x260] = 0x1b;
    assert!(
        pe::inspect(&b).expect("escaped names").imports[0]
            .name
            .starts_with("\\x1B")
    );
}
#[test]
fn truncated_and_adversarial_inputs_never_panic() {
    let original = fixture32();
    for len in 0..original.len() {
        let _ = pe::inspect(&original[..len]);
    }
    let mutations = [
        (0x3c, u32::MAX),
        (0x94, 1),
        (0x98 + 92, u32::MAX),
        (0x98 + 104, 0xfffffff0),
        (0x98 + 108, u32::MAX),
        (0x188, u32::MAX),
        (0x18c, u32::MAX),
        (0x240, u32::MAX - 1),
        (0x20c, 0),
    ];
    for (at, value) in mutations {
        let mut b = original.clone();
        b[at..at + 4].copy_from_slice(&value.to_le_bytes());
        assert!(pe::inspect(&b).is_err(), "mutation at {at:X}");
    }
    let mut state = 0x12345678u32;
    for _ in 0..3000 {
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
#[test]
fn refuses_virtual_only_and_overlapping_import_mappings() {
    let mut b = fixture32();
    b[0x188..0x18c].copy_from_slice(&0x20u32.to_le_bytes());
    assert!(pe::inspect(&b).is_err());
    let mut b = compile("fn main() {}").expect("PE64").executable;
    b[0x188 + 12..0x188 + 16].copy_from_slice(&0x11000u32.to_le_bytes());
    assert!(pe::inspect(&b).is_err());
}
