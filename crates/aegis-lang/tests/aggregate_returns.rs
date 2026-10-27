use aegis_lang::{analyze, compile};

#[test]
fn returns_keep_nominal_and_nested_types() {
    let source = "struct P { x:i64,s:str } fn make(n)->P { return P { x:n,s:\"hi\" }; } fn row()->[P;2] { return [make(7);2]; } fn main() { let a=row(); a[1]=make(9); }";
    let functions = analyze(source).expect("IDE");
    assert_eq!(functions[0].signature, "make(n: i64) -> P");
    assert_eq!(functions[1].signature, "row() -> [P; 2]");
    compile(source).expect("nested return");
    compile("fn f(x)->[i64;2] { if(x) { return [1,2]; } else { return [3,4]; } } fn main() { let a=f(1); }").expect("both branches");
}

#[test]
fn every_aggregate_path_returns_a_matching_value() {
    for source in [
        "fn f()->[i64;2] {} fn main() {}",
        "fn f(x)->[i64;2] { if(x) { return [1,2]; } } fn main() {}",
        "fn f()->[i64;2] { while(1) { return [1,2]; } } fn main() {}",
        "fn f()->[i64;2] { return [1]; } fn main() {}",
        "fn f()->[i64;2] { return [true;2]; } fn main() {}",
        "struct P { x:i64 } struct Q { x:i64 } fn f()->P { return Q { x:1 }; } fn main() {}",
        "struct P { x:i64 } fn f()->P { return P { x:1 }; } fn main() { return f(); }",
        "fn f()->[i64;2] { return [1,2]; } fn main() { printi(f()); }",
    ] {
        assert_eq!(
            compile(source).expect_err(source),
            analyze(source).expect_err(source)
        );
    }
}

#[test]
fn full_return_layout_and_visible_slot_limit() {
    compile("fn f(a:[i64;256])->[i64;256] { return a; } fn main() { let a=f([7;256]); }")
        .expect("256 visible slots plus hidden buffer address");
    assert!(
        compile("fn f(a:[i64;256])->[i64;256] { let extra=1; return a; } fn main() {}").is_err()
    );
}
