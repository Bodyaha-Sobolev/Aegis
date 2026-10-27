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
    ] {
        compile(source).expect(source);
    }
}

#[test]
fn narrow_folding_matches_the_canonical_runtime_form() {
    let pairs = [
        ("2000000000 + 2000000000", "i32(4000000000)"),
        ("-1 << 31", "i32(0x80000000)"),
        ("2147483647 + 1", "i32(0x80000000)"),
        ("~0", "i32(-1)"),
        ("-1 >>> 4", "i32(0x0fffffff)"),
    ];
    for (folded, runtime) in pairs {
        let a = compile(&format!(
            "fn main() {{ let x: i32 = {folded}; return i64(x); }}"
        ))
        .expect(folded);
        let b = compile(&format!(
            "fn main() {{ let x: i32 = {runtime}; return i64(x); }}"
        ))
        .expect(runtime);
        assert_eq!(a.executable, b.executable, "{folded}");
    }
    let a =
        compile("fn main() { let x: u32 = 4294967295; return i64(u64(x)); }").expect("u32 folded");
    let b =
        compile("fn main() { let x: u32 = u32(-1); return i64(u64(x)); }").expect("u32 runtime");
    assert_eq!(a.executable, b.executable);
}
