use aegis_lang::{analyze, compile};

#[test]
fn dynamic_signatures_and_context_reach_the_shared_frontend() {
    let functions = analyze(
        "fn grow(a: [u32], x: u32) -> [u32] { return push(a, x); }
         fn main() { let a: [u32] = []; let b = grow(a, 4000000000); printi(b[0]); }",
    )
    .expect("dynamic array signatures");
    assert_eq!(functions[0].signature, "grow(a: [u32], x: u32) -> [u32]");
    compile(include_str!("../../../examples/dynamic-arrays.ae")).expect("dynamic array example");
}

#[test]
fn dynamic_arrays_preserve_scalar_type_rules_and_place_safety() {
    for source in [
        "fn main() { let a: [[i64]] = []; }",
        "struct Item { x: i64 } fn main() { let a: [Item] = []; }",
        "fn main() { let a: [i64] = [true]; }",
        "fn main() { let a: [i8] = [128]; }",
        "fn main() { let a: [f32] = [1.5]; let b = push(a, f64(2.0)); }",
        "fn main() { let a: [i64] = []; a[0] = true; }",
        "fn main() { let a: [i64] = []; printi(a[false]); }",
        "fn main() { let a = [1, 2]; let b = push(a, 3); }",
        "fn make() -> [i64] { return [1]; } fn main() { make()[0] = 2; }",
        "fn main() { let a = [0; 257]; }",
        "fn main() { let a: [i64] = [0; 1048577]; }",
    ] {
        assert!(compile(source).is_err(), "accepted: {source}");
    }
}

#[test]
fn dynamic_places_work_inside_value_aggregates_and_modules() {
    for source in [
        "struct Bag { a: [str] } fn main() { let b = Bag { a: [\"x\"] };
         let c = b; b.a[0] = \"y\"; b.a = push(c.a, \"z\"); println(b.a[1]); }",
        "fn main() { let x: [i64] = [1]; let a = [x, x]; a[0][0] = 2; }",
        "mod data { fn values() -> [u64] { return [1, 2]; } }
         use data::values; fn main() { printi(push(values(), 3)[2]); }",
        "fn make() -> [f64] { return [1.25; 1048576]; }
         fn main() { printi(make()[1000]); }",
    ] {
        compile(source).expect(source);
    }
}
