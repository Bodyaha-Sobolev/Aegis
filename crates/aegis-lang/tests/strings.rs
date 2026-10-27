use aegis_lang::{HighlightKind, analyze, compile, highlight};

#[test]
fn accepts_strings_across_function_and_array_boundaries() {
    let source = include_str!("../../../examples/strings.ae");
    let functions = analyze(source).expect("string example");
    assert_eq!(functions[0].signature, "title(enabled: bool) -> str");
    assert_eq!(functions[1].signature, "greet(name: str) -> i64");
    compile("fn empty()->str {} fn main() { println(empty()); }").expect("empty fallback");
    compile(
        "fn first(a:[str;2])->str { return a[0]; } fn main() { println(first([\"a\",\"b\"])); }",
    )
    .expect("string return from owned array");
    let colors = highlight(source);
    assert!(colors.iter().any(|t| t.kind == HighlightKind::String));
    for token in colors {
        assert!(source.get(token.span).is_some());
    }
}

#[test]
fn rejects_invalid_escapes_and_unsafe_string_operations() {
    let cases = [
        r#"fn main() { println("unfinished); }"#,
        r#"fn main() { println("\q"); }"#,
        r#"fn main() { println("\u{}"); }"#,
        r#"fn main() { println("\u{D800}"); }"#,
        r#"fn main() { println("\u{110000}"); }"#,
        r#"fn main() { println("\u{1234567}"); }"#,
        "fn main() { println(\"a\nb\"); }",
        r#"fn main() { let s="abc"; s[0]=65; }"#,
        r#"fn main() { printi("abc"); }"#,
        r#"fn main() { println(42); }"#,
        r#"fn main() { if("abc") {} }"#,
        r#"fn main() { let x=i64("abc"); }"#,
        r#"fn main() { let x=bool("abc"); }"#,
        r#"fn main() { let x="a"+"b"; }"#,
        r#"fn main() { printi("a"<"b"); }"#,
        r#"fn main() { let a=["a"]; a[0][0]=1; }"#,
    ];
    for source in cases {
        let error = compile(source).expect_err(source);
        assert_eq!(error, analyze(source).expect_err(source));
        assert!(source.get(error.span).is_some());
    }
}

#[test]
fn readonly_string_data_is_a_separate_nonexecutable_pe_section() {
    let build = compile("fn main() { println(\"hello\"); }").expect("string PE");
    assert_eq!(
        build.executable,
        compile("fn main() { println(\"hello\"); }")
            .expect("reproducible string PE")
            .executable
    );
    let b = build.executable;
    let section = 0x188 + 2 * 40;
    assert_eq!(&b[section..section + 8], b".rdata\0\0");
    let read32 = |at| u32::from_le_bytes(b[at..at + 4].try_into().expect("four bytes"));
    let flags = read32(section + 36);
    assert_ne!(flags & 0x40000000, 0);
    assert_eq!(flags & (0x80000000 | 0x20000000), 0);
    let raw = read32(section + 20) as usize;
    let size = read32(section + 16) as usize;
    assert_eq!(raw % 512, 0);
    assert_eq!(size % 512, 0);
    assert_eq!(b.len(), raw + size + 512); // Read-only, discardable relocation section follows.
    assert!(b[raw..].windows(5).any(|w| w == b"hello"));
    let pooled =
        compile(r#"fn main() { print("same"); print("\u{73}ame"); }"#).expect("deduplicated pool");
    // 56 bytes of f64 constant pool and printd spellings precede the strings;
    // "same" is interned once (8-byte length header plus four bytes).
    assert_eq!(
        u32::from_le_bytes(
            pooled.executable[section + 8..section + 12]
                .try_into()
                .expect("virtual size")
        ),
        76
    );
}
