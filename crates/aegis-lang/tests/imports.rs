use aegis_lang::{compile, project};

#[test]
fn imports_preserve_nominal_types_and_canonical_signatures() {
    let build = compile("use model::Point as P; use model::make; use model as m; fn main() { let p:P=make(); printi(m::score(p)); } mod model { struct Point { x:i64 } fn make()->Point { return Point { x:42 }; } fn score(p:Point) { return p.x; } }").expect("forward imports");
    assert_eq!(
        build
            .functions
            .iter()
            .find(|f| f.name == "model::make")
            .expect("make")
            .signature,
        "model::make() -> model::Point"
    );
    assert_eq!(build.functions.len(), 3);
    let error=compile("mod a { struct P { x:i64 } } mod b { struct P { x:i64 } } use a::P as A; use b::P as B; fn main() { let p:A=B { x:1 }; }").expect_err("aliases do not change identity");
    assert!(error.message.contains("expected a::P"), "{error}");
}

#[test]
fn imports_are_lexical_and_absolute_paths_bypass_them() {
    compile("mod a { fn f() { return 1; } } mod b { fn f() { return 2; } } use a::f as choose; mod app { use b::f as choose; mod child { fn run() { return choose()+::choose_original(); } } fn run() { return child::run(); } } fn choose_original() { return a::f(); } fn main() { printi(choose()+app::run()); }").expect("parent binding and nearest binding");
    let error = compile("fn f() { return 1; } mod a { struct P { x:i64 } } mod app { use a::P as f; fn run() { return f(); } } fn main() {}").expect_err("wrong kind must shadow root function");
    assert!(error.message.contains("undefined function: f"), "{error}");
    compile("fn f() { return 1; } mod a { struct P { x:i64 } } mod app { use a::P as f; fn run() { return ::f(); } } fn main() {}").expect("absolute escape");
}

#[test]
fn rejects_unused_bad_imports_conflicts_and_alias_chains() {
    for (source, message) in [
        ("use missing::f; fn main() {}", "unknown import target"),
        (
            "mod a { fn f() {} } use a::f as x; use a::f as x; fn main() {}",
            "import name already defined",
        ),
        (
            "mod a { fn f() {} } use a::f; fn f() {} fn main() {}",
            "import name already defined",
        ),
        (
            "mod a { fn f() {} } use a::f as a; fn main() {}",
            "import name already defined",
        ),
        (
            "mod a { fn f() {} } use a::f as P; struct P { x:i64 } fn main() {}",
            "import name already defined",
        ),
        (
            "mod a { fn f() {} } use a as b; use b::f; fn main() {}",
            "unknown import target",
        ),
        (
            "use b as a; use a as b; fn main() {}",
            "unknown import target",
        ),
        (
            "mod a { fn f() {} } use a::*; fn main() {}",
            "expected identifier",
        ),
        ("mod a { fn f() {} } use a::f fn main() {}", "expected ';'"),
        ("fn main() { use missing; }", "expected expression"),
    ] {
        let error = compile(source).expect_err(source);
        assert!(error.message.contains(message), "{source}: {error}");
    }
}

#[test]
fn import_budget_is_bounded() {
    let mut source = String::from("mod a { fn f() {} } fn main() {} ");
    for i in 0..256 {
        source.push_str(&format!("use a::f as f{i};"));
    }
    compile(&source).expect("256 imports");
    source.push_str("use a::f as extra;");
    assert!(
        compile(&source)
            .expect_err("257 imports")
            .message
            .contains("import limit")
    );
}

#[test]
fn file_imports_keep_original_error_locations() {
    let dir = tempfile::tempdir().expect("dir");
    let entry = dir.path().join("main.ae");
    let module = dir.path().join("model.ae");
    std::fs::write(
        &module,
        "struct Point { x:i64 } fn make()->Point { return Point { x:9 }; }",
    )
    .expect("module");
    let source = "mod model; use model::Point as P; use model::make; fn main() { let p:P=make(); printi(p.x); }";
    project::compile(&entry, source, &[]).expect("file import");
    let child = "// ошибка\nuse absent::Point as P; fn make() {}";
    std::fs::write(&module, child).expect("bad module");
    let error = project::compile(&entry, source, &[]).expect_err("import in child");
    assert_eq!(error.path, Some(module.canonicalize().expect("canonical")));
    assert_eq!((error.line, error.column), (2, 1));
    assert_eq!(&child[error.span], "use absent::Point as P;");
}
