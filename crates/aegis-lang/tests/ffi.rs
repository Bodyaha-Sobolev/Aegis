use aegis_lang::{analyze, compile};

#[test]
fn external_signatures_require_explicit_unsafe_calls_and_reach_the_ide() {
    let source =
        "mod sys { unsafe extern \"kernel32.dll\" fn mul(a:i32,b:i32,c:i32)->i32 = \"MulDiv\"; }
        use sys::mul as scale;
        fn main() { unsafe { printi(scale(9,10,3)); } }";
    let info = analyze(source).expect("external signature");
    assert_eq!(
        info[0].signature,
        "unsafe extern sys::mul(a: i32, b: i32, c: i32) -> i32"
    );
    compile(source).expect("external module import");
    for source in [
        "unsafe extern \"kernel32.dll\" fn tick()->u64=\"GetTickCount64\"; fn main() { tick(); }",
        "mod sys { unsafe extern \"kernel32.dll\" fn tick()->u64=\"GetTickCount64\"; } use sys::tick as now; fn main() { if(false) { now(); } }",
        "unsafe extern \"kernel32.dll\" fn tick()->u64=\"GetTickCount64\"; fn main() { unsafe { tick(); } tick(); }",
    ] {
        let error = compile(source).expect_err("unsafe boundary");
        assert!(error.message.contains("unsafe block"), "{error}");
    }
}

#[test]
fn rejects_invalid_external_contracts_before_any_library_access() {
    for source in [
        "unsafe extern \"missing.dll\" fn f(a)->i64=\"f\"; fn main() {}",
        "unsafe extern \"missing.dll\" fn f()=\"f\"; fn main() {}",
        "unsafe extern \"missing.dll\" fn f(a:str)->i64=\"f\"; fn main() {}",
        "unsafe extern \"missing.dll\" fn f()->str=\"f\"; fn main() {}",
        "unsafe extern \"missing.dll\" fn f(a:[u8])->i64=\"f\"; fn main() {}",
        "struct S { x:i64 } unsafe extern \"missing.dll\" fn f(a:S)->i64=\"f\"; fn main() {}",
        "unsafe extern \"missing.dll\" fn main()->i64=\"f\";",
        "unsafe extern \"relative\\\\missing.dll\" fn f()->i64=\"f\"; fn main() {}",
        "unsafe extern \"C:missing.dll\" fn f()->i64=\"f\"; fn main() {}",
        "unsafe extern \"C:/missing.dll\" fn f()->i64=\"f\"; fn main() {}",
        "unsafe extern \"missing.exe\" fn f()->i64=\"f\"; fn main() {}",
        "unsafe extern \"missing\\0.dll\" fn f()->i64=\"f\"; fn main() {}",
        "unsafe extern \"missing.dll\" fn f()->i64=\"bad\\0name\"; fn main() {}",
        "unsafe extern \"missing.dll\" fn f()->i64=\"\"; fn main() {}",
        "unsafe extern \"missing.dll\" fn f()->i64=\"имя\"; fn main() {}",
        "unsafe extern \"missing.dll\" fn f()->i64=\"with space\"; fn main() {}",
    ] {
        assert!(compile(source).is_err(), "accepted {source}");
    }
}

#[test]
fn bounded_external_tables_and_pure_analysis() {
    let mut source = String::new();
    for index in 0..64 {
        source.push_str(&format!(
            "unsafe extern \"definitely-not-present.dll\" fn external_fn_{index}()->i64=\"missing\";"
        ));
    }
    compile(&(source.clone() + "fn main() {} ")).expect("64 unused declarations do not load DLLs");
    source.push_str("unsafe extern \"missing.dll\" fn overflow()->i64=\"missing\"; fn main() {}");
    assert!(
        compile(&source)
            .expect_err("64-slot bound")
            .message
            .contains("64")
    );
    let path = "unsafe extern \"C:\\\\not-installed\\\\библиотека.dll\" fn f()->f64=\"probe\"; fn main() {}";
    compile(path).expect("absolute Unicode path needs no installed library for analysis");
}

#[test]
fn external_diagnostics_identify_the_offending_literal() {
    for (source, literal) in [
        (
            r#"unsafe extern "sub\\a.dll" fn f()->i64="valid"; fn main() {}"#,
            r#""sub\\a.dll""#,
        ),
        (
            r#"unsafe extern "missing.dll" fn f()->i64="имя"; fn main() {}"#,
            r#""имя""#,
        ),
    ] {
        let error = compile(source).expect_err("invalid external literal");
        assert_eq!(&source[error.span], literal);
    }
}
