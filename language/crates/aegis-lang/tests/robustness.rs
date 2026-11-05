use aegis_lang::{compile, highlight};

#[test]
fn malformed_inputs_never_panic() {
    let alphabet = [
        "fn", "main", "(", ")", "{", "}", ";", "let", "x", "=", "0x", "1", "+", "-", "return",
        "//", "\n", "/*", "*/", "λ", "\0", "0b", "~", "^", "&", "|", "<<", ">>>", "i64", "u64",
        "bool", "true", "false", ":", "->", "[", "]", "len", ",", "str", "\"", "\\u{", "print",
        "println",
    ];
    let mut state = 0x89abcdefu32;
    for _ in 0..2000 {
        let mut source = String::new();
        for _ in 0..32 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            source.push_str(alphabet[state as usize % alphabet.len()]);
            source.push(' ');
        }
        let _ = compile(&source);
        for token in highlight(&source) {
            assert!(source.get(token.span).is_some());
        }
    }
}

#[test]
fn limits_protect_both_parse_stack_and_expression_drop() {
    let chain = format!("fn main() {{ return {}; }}", vec!["1"; 20000].join("+"));
    assert!(compile(&chain).is_err());
    let nested = format!(
        "fn main() {{ return {}1{}; }}",
        "(".repeat(1000),
        ")".repeat(1000)
    );
    assert!(compile(&nested).is_err());
    assert!(compile(&" ".repeat(aegis_lang::MAX_SOURCE + 1)).is_err());
}

#[test]
fn code_generation_is_reproducible() {
    let source = include_str!("../../../examples/factorial.ae");
    let a = compile(source).expect("compile example");
    let b = compile(source).expect("compile example again");
    assert_eq!(a.executable, b.executable);
    assert_eq!(&a.executable[..2], b"MZ");
    assert_eq!(a.functions.len(), 2);
}

#[test]
fn constant_math_is_folded_without_changing_runtime_errors() {
    let formula = compile("fn main() { return (2+3)*8+2; }").expect("formula");
    let literal = compile("fn main() { return 42; }").expect("literal");
    assert_eq!(formula.executable, literal.executable);
    assert!(compile("fn main() { return 1/0; }").is_ok());
    assert!(compile("fn main() { return 0 && missing; }").is_err());
}
