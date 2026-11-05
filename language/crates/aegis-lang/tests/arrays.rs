use aegis_lang::{analyze, compile};

#[test]
fn accepts_typed_arrays_and_value_copies() {
    let functions =
        analyze(include_str!("../../../examples/array-functions.ae")).expect("array functions");
    assert_eq!(functions[0].signature, "sum(values: [i64; 6]) -> i64");
    compile(include_str!("../../../examples/arrays.ae")).expect("array sorting example");
    for source in [
        "fn main() { let a:[u64;2]=[1,2]; let b=a; printi(b[0]); }",
        "fn main() { let a=[1,u64(2)]; a[1]=0xffffffffffffffff; }",
        "fn main() { let a=[true,false,]; a[0]=!a[1]; printi(a[0]); }",
        "fn main() { let a=[1,2]; a=[a[1],a[0]]; }",
        "fn f(a:[u64;2])->u64 { return a[0]+a[1]; } fn main() { printi(f([1,2])); }",
    ] {
        compile(source).expect(source);
    }
}

#[test]
fn rejects_invalid_types_and_storage_escape() {
    let cases = [
        "fn main() { let a=[]; }",
        "fn main() { let a:[i64;0]=[]; }",
        "fn main() { let a:[i64;257]=[1]; }",
        "fn main() { let a:[i64;2]=[1]; }",
        "fn main() { let a=[1,true]; }",
        "fn main() { let a=[1]; a[0]=true; }",
        "fn main() { let a=[1]; printi(a[false]); }",
        "fn main() { let a=1; printi(a[0]); }",
        "fn main() { let a=[1]; printi(a); }",
        "fn main() { let a=[1]; printi(i64(a)); }",
        "fn main() { let a=[1]; if(a) {} }",
        "fn main() { let a=[1]; printi(a==a); }",
        "fn main() { let a=[1]; return a; }",
        "fn f(a:[i64;2]) {} fn main() { f([1]); }",
        "fn main() { printi(len(42)); }",
        "fn main() { printi(len([1,2])); }",
        "fn main() { let a=[1]; a=[true]; }",
    ];
    for source in cases {
        let error = compile(source).expect_err(source);
        assert_eq!(error, analyze(source).expect_err(source));
        assert!(source.get(error.span).is_some());
    }
}

#[test]
fn array_slots_obey_function_storage_limit() {
    let values = vec!["1"; 256].join(",");
    compile(&format!(
        "fn main() {{ let a=[{values}]; printi(a[255]); }}"
    ))
    .expect("256 slots");
    let error =
        compile(&format!("fn main() {{ let a=[{values}]; let b=1; }}")).expect_err("257 slots");
    assert!(error.message.contains("local variable limit"));
    let error = compile(&format!("fn main() {{ let a=[{values},1]; }}")).expect_err("257 elements");
    assert!(error.message.contains("array literal limit"));
    compile(&format!(
        "fn main() {{ {{let a=[{values}];}} {{let b=[{values}];}} }}"
    ))
    .expect("disjoint scopes reuse storage");
}

#[test]
fn repeat_arrays_use_context_and_obey_limits() {
    for source in [
        "fn main() { let a=[0;256]; printi(a[255]); }",
        "fn main() { let a:[u64;3]=[0xffffffffffffffff;3]; }",
        "fn main() { let a=[true;3]; a=[false;3]; }",
        "fn main() { let a=[\"hello\";3]; println(a[2]); }",
        "fn f(a:[u64;3]) {} fn main() { f([1;3]); }",
        "fn main() { let a=[1;3]; a=[a[2]+1;3]; }",
    ] {
        compile(source).expect(source);
    }
    for source in [
        "fn main() { let a=[0;0]; }",
        "fn main() { let a=[0;257]; }",
        "fn main() { let a=[0;-1]; }",
        "fn main() { let a=[0;1+1]; }",
        "fn main() { let a=[0;18446744073709551615]; }",
        "fn main() { let a:[bool;2]=[1;2]; }",
        "fn main() { let a:[i64;3]=[1;2]; }",
        "fn main() { let a=[1;256]; let b=2; }",
        "fn main() { printi([1;2]); }",
        "fn f(a:[str;2]) {} fn main() { f([true;2]); }",
        "fn main() { let a=[false;2]; a=[1;2]; }",
        "fn main() { let a=[true;2]; a=[false;3]; }",
    ] {
        assert_eq!(
            compile(source).expect_err(source),
            analyze(source).expect_err(source)
        );
    }
}
