//! Measures compiler throughput, NOT the latency of an individual native formula.
use std::{hint::black_box, time::Instant};

fn main() {
    let source = include_str!("../../../examples/factorial.ae");
    let iterations = 1000;
    let start = Instant::now();
    let mut bytes = 0;
    for _ in 0..iterations {
        let result = aegis_lang::compile(black_box(source)).expect("valid benchmark source");
        bytes += black_box(result.code_bytes);
    }
    let elapsed = start.elapsed();
    println!(
        "{iterations} complete compilations; {:.2} us/compile; {bytes} accumulated native code bytes",
        elapsed.as_secs_f64() * 1_000_000.0 / iterations as f64
    );
}
