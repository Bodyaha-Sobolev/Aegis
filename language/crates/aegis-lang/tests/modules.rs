use aegis_lang::{analyze, compile};

#[test]
fn scopes_resolve_forward_types_functions_and_signatures() {
    let source = "fn main() { let p=world::make(7); printi(world::math::score(p)); } mod world { fn make(n)->Point { return Point { x:n }; } mod math { fn score(p:Point) { return p.x+adjust(1); } } fn adjust(n) { return n+1; } struct Point { x:i64 } }";
    let functions = analyze(source).expect("modules in IDE");
    assert_eq!(functions[1].name, "world::make");
    assert_eq!(
        functions[1].signature,
        "world::make(n: i64) -> world::Point"
    );
    assert_eq!(
        functions[2].signature,
        "world::math::score(p: world::Point) -> i64"
    );
    for function in functions {
        assert!(source[function.span].starts_with("fn "));
    }
    compile(source).expect("native modules");
}

#[test]
fn duplicate_unknown_and_cross_module_types_are_checked() {
    for source in [
        "mod a {} mod a {} fn main() {}",
        "mod a { fn f() {} fn f() {} } fn main() {}",
        "mod a { struct P { x:i64 } struct P { x:i64 } } fn main() {}",
        "mod a { fn main() {} }",
        "fn main() { mod a {} }",
        "mod a; fn main() {}",
        "mod a { fn f() {} fn main() { b::f(); } } fn main() {}",
        "mod a { struct P { x:i64 } } mod b { struct P { x:i64 } } fn main() { let p:a::P=b::P { x:1 }; }",
        "mod a { struct P { q:b::Q } } mod b { struct Q { p:a::P } } fn main() {}",
        "mod a { struct P { x:i64 } } fn main() { let p:a::Missing=1; }",
        "mod a { fn f() {} } fn main() { let x=a::f; }",
        "mod a { fn f() {} } fn main() { a::f(1); }",
        "mod a { struct P { x:i64 } } fn main() { let p=::a::P { x:true }; }",
    ] {
        let error = compile(source).expect_err(source);
        assert_eq!(error, analyze(source).expect_err(source));
        assert!(source.get(error.span).is_some());
    }
}

#[test]
fn module_and_path_limits_protect_the_frontend() {
    let nested = |count| {
        format!(
            "{}fn f() {{}}{} fn main() {{}}",
            (0..count)
                .map(|i| format!("mod m{i} {{"))
                .collect::<String>(),
            "}".repeat(count)
        )
    };
    compile(&nested(16)).expect("16 modules");
    let path = (0..16)
        .map(|i| format!("m{i}"))
        .collect::<Vec<_>>()
        .join("::");
    compile(&nested(16).replace("fn main() {}", &format!("fn main() {{ {path}::f(); }}")))
        .expect("17-component item path");
    let too_long = (0..19).map(|_| "a").collect::<Vec<_>>().join("::");
    assert!(
        compile(&format!("fn main() {{ {too_long}(); }}"))
            .expect_err("path limit")
            .message
            .contains("path limit")
    );

    assert!(
        compile(&nested(17))
            .expect_err("depth")
            .message
            .contains("module nesting limit")
    );
    let modules = (0..129)
        .map(|i| format!("mod m{i} {{}}"))
        .collect::<String>();
    assert!(
        compile(&format!("{modules} fn main() {{}}"))
            .expect_err("count")
            .message
            .contains("module limit")
    );
    let source = "mod a { struct P { x:i64 } mod b { fn f(n)->::a::P { return ::a::P { x:n }; } } } fn main() { let p=a::b::f(42); printi(p.x); }";
    for end in 0..source.len() {
        let _ = compile(&source[..end]);
    }
    for index in 0..source.len() {
        let mut bytes = source.as_bytes().to_vec();
        bytes[index] = b':';
        let _ = compile(std::str::from_utf8(&bytes).expect("ASCII"));
    }
}
