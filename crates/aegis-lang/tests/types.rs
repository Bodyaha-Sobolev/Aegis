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
