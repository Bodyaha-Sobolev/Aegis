use aegis_lang::{analyze, compile};

#[test]
fn enum_values_and_exhaustive_match_reach_the_shared_frontend() {
    let source = "enum Result { Ok(str), Err(u32), Empty }
        fn make(ok:bool)->Result { if(ok) { return Result::Ok(\"hello\"+\"!\"); } return Result::Err(5); }
        fn main() { let value=make(true); match value {
            Result::Ok(text) => { println(text); }, Result::Err(code) => { printi(code); },
            Result::Empty => {} } }";
    let info = analyze(source).expect("enum program");
    assert_eq!(info[0].signature, "make(ok: bool) -> Result");
    compile(source).expect("enum code generation");
}

#[test]
fn patterns_are_exhaustive_nominal_and_scoped() {
    let prefix = "enum E { Text(str), Number(i64), Empty } enum Other { Empty } ";
    for body in [
        "let e=E::Empty; match e { E::Empty => {} }",
        "let e=E::Empty; match e { E::Empty => {}, E::Empty => {}, E::Text(t) => {}, E::Number(n) => {} }",
        "let e=E::Empty; match e { Other::Empty => {}, E::Text(t) => {}, E::Number(n) => {} }",
        "let e=E::Empty; match e { E::Empty(x) => {}, E::Text(t) => {}, E::Number(n) => {} }",
        "let e=E::Empty; match e { E::Empty => {}, E::Text => {}, E::Number(n) => {} }",
        "let e=E::Empty; match e { E::Empty => {}, E::Text(t) => {}, E::Number(n) => {} } println(t);",
        "let x=1; match x { E::Empty => {} }",
        "let e=E::Text(3);",
        "let e=E::Text;",
        "let e=E::Empty(1);",
        "let e:E=Other::Empty;",
        "let e=E::Missing;",
        "let e=E::Empty; printi(e);",
        "let e=E::Empty; let a:[E]=[];",
    ] {
        assert!(
            compile(&format!("{prefix}fn main() {{ {body} }}")).is_err(),
            "accepted {body}"
        );
    }
    compile(&format!("{prefix}fn main() {{ match E::Text(\"x\") {{ E::Text(_) => {{}}, E::Number(_) => {{}}, E::Empty => {{}} }} }}"))
        .expect("discard patterns");
}

#[test]
fn enum_layouts_are_bounded_and_cannot_recurse_by_value() {
    for declaration in [
        "enum E {}",
        "enum E { A, A }",
        "enum E { Again(E) }",
        "enum E { Again(S) } struct S { value:E }",
        "enum E { Huge([i64;256]) }",
        "enum E { A } struct E { x:i64 }",
        "enum E { A } mod E {}",
    ] {
        assert!(
            compile(&format!("{declaration} fn main() {{}} ")).is_err(),
            "accepted {declaration}"
        );
    }
    let variants = (0..128)
        .map(|i| format!("V{i}"))
        .collect::<Vec<_>>()
        .join(",");
    compile(&format!(
        "enum E {{ {variants} }} fn main() {{ let e=E::V127; }}"
    ))
    .expect("128 variants");
    assert!(
        compile(&format!(
            "enum E {{ {variants}, Overflow }} fn main() {{}} "
        ))
        .is_err()
    );
    let mut source = "enum E0 { Text(str), Number(i64) }".to_owned();
    for index in 1..=10 {
        source.push_str(&format!(
            "enum E{index} {{ A(E{}), B(E{}) }}",
            index - 1,
            index - 1
        ));
    }
    compile(&(source.clone() + "fn main() {} ")).expect("bounded owned variant graph");
    source.push_str("enum Overflow { A(E10), B(E10) } fn main() {}");
    assert!(
        compile(&source)
            .expect_err("owned expansion bound")
            .message
            .contains("owned layout")
    );
}

#[test]
fn enum_imports_forward_types_and_return_analysis_work() {
    let source = "mod io { enum Outcome { Good(Entry), Error(u32) } struct Entry { text:str } }
        use io::Outcome as Answer; use io::Entry;
        fn create()->Answer { return Answer::Good(Entry { text:\"ok\" }); }
        fn describe(value:Answer)->str { match value { Answer::Good(entry) => { return entry.text; }, Answer::Error(_) => { return \"error\"; } } }
        fn main() { println(describe(create())); }";
    compile(source).expect("forward types and imported enum");
    assert!(compile("enum E { A, B } fn f(e:E)->E { match e { E::A => { return E::A; }, E::B => {} } } fn main() {} ").is_err());
}

#[test]
fn native_code_budget_rejects_expansion_without_panicking() {
    let repeated = format!("fn main() {{ {} }}", "printi(1);".repeat(10000));
    assert!(
        compile(&repeated)
            .expect_err("code budget")
            .message
            .contains("native code limit")
    );
    let mut declarations = "enum E0 { Text(str), Number(i64) }".to_owned();
    let mut value = "E0::Text(\"x\"+\"y\")".to_owned();
    for index in 1..=10 {
        declarations.push_str(&format!(
            "enum E{index} {{ A(E{}), B(E{}) }}",
            index - 1,
            index - 1
        ));
        value = format!("E{index}::A({value})");
    }
    let source = format!("{declarations} fn main() {{ let value={value}; }}");
    assert!(
        compile(&source)
            .expect_err("drop expansion code budget")
            .message
            .contains("native code limit")
    );
}
