use aegis_lang::{analyze, compile};

#[test]
fn exposes_typed_signatures_to_ide() {
    let source = include_str!("../../../examples/typed.ae");
    let functions = analyze(source).expect("typed example");
    assert_eq!(functions[0].signature, "maximum(a: u64, b: u64) -> u64");
    assert_eq!(
        functions[1].signature,
        "enabled(flags: u64, mask: u64) -> bool"
    );
    assert_eq!(functions[2].signature, "main() -> i64");
}

#[test]
fn rejects_type_confusion_before_optimization() {
    let cases = [
        "fn main() { let x: bool = 1; }",
        "fn main() { let x = true; x = 1; }",
        "fn main() { let x: u64 = 1; let y: i64 = x; }",
        "fn f(x: bool) {} fn main() { f(1); }",
        "fn f() -> bool { return 1; } fn main() {}",
        "fn main() { return true; }",
        "fn main() { printi(true+false); }",
        "fn main() { printi(~true); }",
        "fn main() { printi(1 << false); }",
        "fn main() { printi(true < false); }",
        "fn main() { let x: u64=1; let y: i64=1; printi(x+y); }",
        "fn main() { if(false) { let x: bool=1; } }",
        "fn main() { printi(false && (true+false)); }",
    ];
    for source in cases {
        let build = compile(source).expect_err(source);
        let analysis = analyze(source).expect_err(source);
        assert_eq!(build, analysis);
        assert!(
            build.message.contains("type mismatch") || build.message.contains("require integers"),
            "{build}"
        );
        assert!(source.get(build.span).is_some());
    }
}

#[test]
fn literals_acquire_context_but_named_values_require_conversion() {
    for source in [
        "fn f(x: u64) -> u64 { return 1+x; } fn main() { printi(f(42)); }",
        "fn main() { let x: u64=1+2; x=x+1; printi(x>0); }",
        "fn main() { let x: u64=1; let y: i64=i64(x); printi(u64(y)); }",
        "fn main() { let x: bool=true; let y: bool=!x; printi(x==y); }",
        "fn f() -> bool {} fn main() { printi(f()); }",
    ] {
        compile(source).expect(source);
    }
}

#[test]
fn typed_literals_and_runtime_have_distinct_signed_semantics() {
    let a = compile("fn main() { return i64(u64(0xffffffffffffffff)/2); }").expect("unsigned");
    let b = compile("fn main() { return 9223372036854775807; }").expect("expected");
    assert_eq!(a.executable, b.executable);
    let a = compile("fn main() { return i64(u64(0xffffffffffffffff)>1); }")
        .expect("unsigned comparison");
    let b = compile("fn main() { return 1; }").expect("true as integer");
    assert_eq!(a.executable, b.executable);
}

#[test]
fn narrow_signatures_reach_the_ide_outline() {
    let functions = analyze("fn sample(x: i32, y: u32) -> i32 { return x; } fn main() {}")
        .expect("narrow signature");
    assert_eq!(functions[0].signature, "sample(x: i32, y: u32) -> i32");
    let functions = analyze("fn pixel(r: u8, g: u8, b: u8) -> u16 { return u16(r); } fn main() {}")
        .expect("byte signature");
    assert_eq!(functions[0].signature, "pixel(r: u8, g: u8, b: u8) -> u16");
}

#[test]
fn narrow_literals_use_width_aware_bounds() {
    for source in [
        "fn main() { let x: i32 = 3000000000; }",
        "fn main() { let x: i32 = 2147483648; }",
        "fn main() { let x: i32 = -3000000000; }",
        "fn main() { let x: u32 = 4294967296; }",
        "fn f(x: i32) {} fn main() { f(5000000000); }",
        "fn main() { let x: i32 = 0; let y: u32 = x; }",
        "fn main() { let x: i8 = 128; }",
        "fn main() { let x: i8 = -129; }",
        "fn main() { let x: u8 = 256; }",
        "fn main() { let x: i16 = 32768; }",
        "fn main() { let x: i16 = -32769; }",
        "fn main() { let x: u16 = 65536; }",
    ] {
        let error = compile(source).expect_err(source);
        assert!(
            error.message.contains("does not fit") || error.message.contains("type mismatch"),
            "{error}"
        );
        assert!(source.get(error.span).is_some());
    }
    for source in [
        "fn main() { let x: i32 = 2147483647; let m: i32 = -2147483648; printi(x+m); }",
        "fn main() { let x: i32 = 0x80000000; let f: i32 = 0xffffffff; printi(x+f); }",
        "fn main() { let x: u32 = 4294967295; let y: u32 = 0xffffffff; printi(x+y); }",
        "fn main() { let x: i32 = -(-2147483648); printi(x); }",
        "fn main() { let x: i32 = -(2147483648); printi(x); }",
        "fn main() { let big = 3000000000; printi(big); }",
        "fn main() { let x: i8 = 127; let m: i8 = -128; let h: i8 = 0x80; let f: i8 = 0xff; printi(x+m+h+f); }",
        "fn main() { let x: u8 = 255; let y: u8 = 0xff; printi(x+y); }",
        "fn main() { let x: i16 = 32767; let m: i16 = -32768; let h: i16 = 0x8000; printi(x+m+h); }",
        "fn main() { let x: u16 = 65535; let y: u16 = 0xffff; printi(x+y); }",
    ] {
        compile(source).expect(source);
    }
}

#[test]
fn narrow_folding_matches_the_canonical_runtime_form() {
    for ty in ["i32", "i8", "i16"] {
        let (limit, min, half, logical) = match ty {
            "i8" => ("0x7f", "0x80", "7", "0x0f"),
            "i16" => ("0x7fff", "0x8000", "15", "0x0fff"),
            _ => ("2147483647", "0x80000000", "31", "0x0fffffff"),
        };
        let pairs = [
            (format!("{limit} + 1"), format!("{ty}({min})")),
            (format!("-1 << {half}"), format!("{ty}({min})")),
            ("~0".to_string(), format!("{ty}(-1)")),
            ("-1 >>> 4".to_string(), format!("{ty}({logical})")),
        ];
        for (folded, runtime) in pairs {
            let a = compile(&format!(
                "fn main() {{ let x: {ty} = {folded}; return i64(x); }}"
            ))
            .expect(&folded);
            let b = compile(&format!(
                "fn main() {{ let x: {ty} = {runtime}; return i64(x); }}"
            ))
            .expect(&runtime);
            assert_eq!(a.executable, b.executable, "{folded}");
        }
    }
    let pairs = [
        (
            "fn main() { let x: u32 = 4294967295; return i64(u64(x)); }",
            "fn main() { let x: u32 = u32(-1); return i64(u64(x)); }",
        ),
        (
            "fn main() { let x: u8 = 255; return i64(u64(x)); }",
            "fn main() { let x: u8 = u8(-1); return i64(u64(x)); }",
        ),
        (
            "fn main() { let x: u16 = 65535; return i64(u64(x)); }",
            "fn main() { let x: u16 = u16(-1); return i64(u64(x)); }",
        ),
    ];
    for (folded, runtime) in pairs {
        let a = compile(folded).expect(folded);
        let b = compile(runtime).expect(runtime);
        assert_eq!(a.executable, b.executable);
    }
}
