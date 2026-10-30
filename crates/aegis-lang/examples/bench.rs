//! Measures compiler throughput, NOT the latency of an individual native formula.
use std::{env, hint::black_box, time::Instant};

fn main() {
    let mut iterations = 1000usize;
    let mut scenario = String::from("factorial");
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--iterations" => {
                iterations = args
                    .next()
                    .expect("--iterations requires a positive integer")
                    .parse()
                    .expect("--iterations requires a positive integer");
                assert!(iterations > 0, "--iterations requires a positive integer");
            }
            "--scenario" => scenario = args.next().expect("--scenario requires a name"),
            "--help" | "-h" => {
                println!("usage: bench [--iterations N] [--scenario factorial|arrays|structs]");
                return;
            }
            other => panic!("unknown argument: {other}"),
        }
    }
    let source = match scenario.as_str() {
        "factorial" => include_str!("../../../examples/factorial.ae"),
        "arrays" => include_str!("../../../examples/arrays.ae"),
        "structs" => include_str!("../../../examples/structs.ae"),
        other => panic!("unknown scenario: {other}"),
    };
    let start = Instant::now();
    let mut bytes = 0;
    for _ in 0..iterations {
        let result = aegis_lang::compile(black_box(source)).expect("valid benchmark source");
        bytes += black_box(result.code_bytes);
    }
    let elapsed = start.elapsed();
    println!(
        "scenario={scenario} iterations={iterations} us_per_compile={:.2} native_code_bytes={bytes}",
        elapsed.as_secs_f64() * 1_000_000.0 / iterations as f64
    );
}
