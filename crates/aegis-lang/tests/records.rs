use aegis_lang::{analyze, compile};

#[test]
fn records_are_nominal_and_signatures_keep_names() {
    let source = "fn score(v:Entity)->u64 { return v.mask; } struct Entity { name:str, mask:u64, active:bool, x:i64, } fn main() { let v=Entity { x:-2, active:true, mask:0xffffffffffffffff, name:\"player\" }; printi(score(v)); let copy:Entity=v; copy.x=3; v=copy; }";
    let functions = analyze(source).expect("forward layout");
    assert_eq!(functions[0].signature, "score(v: Entity) -> u64");
    compile(source).expect("typed construction and copy");
}

#[test]
fn invalid_records_fail_equally_in_ide_and_compiler() {
    let cases = [
        "struct P {} fn main() {}",
        "struct P { x:i64, x:i64 } fn main() {}",
        "struct P { x:i64 } struct P { y:i64 } fn main() {}",
        "struct P { x:[i64;2] } fn main() {}",
        "struct P { x:P } fn main() {}",
        "struct P { x:Missing } fn main() {}",
        "fn main() { let p:Missing=1; }",
        "struct P { x:i64 } fn main() { let p=P {}; }",
        "struct P { x:i64 } fn main() { let p=P { x:1, x:2 }; }",
        "struct P { x:i64 } fn main() { let p=P { y:1 }; }",
        "struct P { x:i64 } fn main() { let p=P { x:true }; }",
        "struct P { x:i64 } fn main() { let p=P { x:1 }; p.y=2; }",
        "struct P { x:i64 } fn main() { let p=P { x:1 }; p.x=true; }",
        "struct P { x:i64 } fn main() { let p=P { x:1 }; printi(p); }",
        "struct P { x:i64 } fn main() { let p=P { x:1 }; printi(p+1); }",
        "struct P { x:i64 } fn main() { let p=P { x:1 }; printi(p==p); }",
        "struct P { x:i64 } fn main() { let p=P { x:1 }; if(p) {} }",
        "struct P { x:i64 } fn main() { let p=P { x:1 }; printi(p[0]); }",
        "struct P { x:i64 } fn main() { let p=P { x:1 }; printi(len(p)); }",
        "struct P { x:i64 } fn main() { let p=P { x:1 }; let a=[p]; }",
        "struct P { x:i64 } fn main() { let p=P { x:1 }; let a=[p;2]; }",
        "struct P { x:i64 } struct Q { x:i64 } fn main() { let p:P=Q { x:1 }; }",
        "struct P { x:i64 } struct Q { x:i64 } fn f(p:P) {} fn main() { f(Q { x:1 }); }",
        "struct P { x:i64 } fn f()->P { return P { x:1 }; } fn main() {}",
        "fn main() { let p=1; p.x=2; }",
        "struct P { x:i64 } fn main() { printi(P { x:1 }.x); }",
        "struct P { s:str } fn main() { let p=P { s:\"hi\" }; p.s[0]=1; }",
    ];
    for source in cases {
        let error = compile(source).expect_err(source);
        assert_eq!(error, analyze(source).expect_err(source));
        assert!(source.get(error.span).is_some());
    }
}

#[test]
fn layouts_and_live_storage_are_bounded() {
    let fields = (0..256)
        .map(|i| format!("f{i}:i64"))
        .collect::<Vec<_>>()
        .join(",");
    let values = (0..256)
        .map(|i| format!("f{i}:{i}"))
        .collect::<Vec<_>>()
        .join(",");
    compile(&format!(
        "struct P {{ {fields} }} fn main() {{ let p=P {{ {values} }}; printi(p.f255); }}"
    ))
    .expect("full frame");
    let error = compile(&format!(
        "struct P {{ {fields} }} fn main() {{ let p=P {{ {values} }}; let extra=0; }}"
    ))
    .expect_err("live limit");
    assert!(error.message.contains("local variable limit"));
    assert!(
        compile(&format!(
            "struct P {{ {fields}, extra:i64 }} fn main() {{}}"
        ))
        .is_err()
    );
    let definitions = (0..129)
        .map(|i| format!("struct S{i} {{ x:i64 }}"))
        .collect::<String>();
    assert!(compile(&format!("{definitions} fn main() {{}}")).is_err());
}

#[test]
fn truncated_and_mutated_constructors_never_panic() {
    let source = "struct P { x:i64, label:str, active:bool } fn use(p:P) { p.x=p.x+1; println(p.label); } fn main() { let p=P { label:\"abc\", active:true, x:42 }; use(p); }";
    for end in 0..source.len() {
        let _ = compile(&source[..end]);
    }
    for index in 0..source.len() {
        for replacement in [b'.', b'{', b'}', b'[', b';', b'0'] {
            let mut bytes = source.as_bytes().to_vec();
            bytes[index] = replacement;
            let text = std::str::from_utf8(&bytes).expect("ASCII");
            let _ = compile(text);
        }
    }
}
