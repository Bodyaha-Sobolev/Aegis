use aegis_lang::{analyze, compile};

#[test]
fn composite_arrays_infer_types_and_preserve_signatures() {
    let source = "struct P { x:i64,y:i64 } struct World { actors:[P;2],grid:[[u64;2];3] } fn score(p:[P;2]) { return p[0].x+p[1].y; } fn main() { let w=World { actors:[P { x:1,y:2 };2],grid:[[0;2];3] }; w.actors[1].x=3; let p=w.actors[0]; w.actors[1]=p; let row=w.grid[0]; w.grid[1]=row; printi(score(w.actors)); }";
    assert_eq!(
        analyze(source).expect("IDE")[0].signature,
        "score(p: [P; 2]) -> i64"
    );
    compile(source).expect("composite places and copies");
    for source in [
        "fn main() { let a=[[1,2],[3,4]]; a[0]=a[1]; }",
        "fn main() { let a=[[1],[u64(2)]]; a[0][0]=0xffffffffffffffff; }",
        "fn main() { let a:[[[bool;2];2];2]=[[[true;2];2];2]; printi(a[1][0][1]); }",
    ] {
        compile(source).expect(source);
    }
}

#[test]
fn invalid_shapes_types_cycles_and_layouts_fail_consistently() {
    for source in [
        "fn main() { let a=[[1],[2,3]]; }",
        "fn main() { let a=[[1],[true]]; }",
        "fn main() { let a=[[]]; }",
        "fn main() { let a=[[1;16];17]; }",
        "fn main() { let a:[[i64;16];17]=[[1;16];17]; }",
        "struct A { x:[A;1] } fn main() {}",
        "struct A { x:[B;1] } struct B { x:[A;1] } fn main() {}",
        "struct P { x:i64,y:i64 } fn main() { let a=[P { x:1,y:2 };129]; }",
        "struct P { x:i64 } struct Q { x:i64 } fn main() { let a:[P;1]=[Q { x:1 }]; }",
        "struct P { x:i64 } fn main() { let a=[P { x:1 };2]; a[0].x=true; }",
        "struct P { x:[str;2] } fn main() { let a=[P { x:[\"hi\";2] };2]; a[0].x[0][0]=1; }",
        "fn main() { let a=[[1;16];16]; let b=1; }",
    ] {
        assert_eq!(
            compile(source).expect_err(source),
            analyze(source).expect_err(source)
        );
    }
}

#[test]
fn composite_depth_is_bounded_before_stack_exhaustion() {
    let array = |depth| format!("{}i64{}", "[".repeat(depth), ";1]".repeat(depth));
    compile(&format!("fn f(a:{}) {{}} fn main() {{}}", array(32))).expect("32 levels");
    for depth in [33, 1000] {
        assert!(compile(&format!("fn f(a:{}) {{}} fn main() {{}}", array(depth))).is_err());
    }
    let source = "struct P { x:[P;1] } fn main() { let a=[[1,2],[3,4]]; a[1][0]=a[0][1]; printi(len(a[1])); }";
    for end in 0..source.len() {
        let _ = compile(&source[..end]);
    }
}
