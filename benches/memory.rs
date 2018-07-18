//! Measures the memory used by an IR instance.
extern crate env_logger;
extern crate itertools;
extern crate jemalloc_ctl;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate telamon_utils;

mod common;

use itertools::Itertools;

/// Reads the amount of resident memory.
fn resident_memory() -> usize {
    // many statistics are cached and only updated when the epoch is advanced.
    unwrap!(jemalloc_ctl::epoch());
    unwrap!(jemalloc_ctl::stats::resident())
}

const NUM_DESCENTS: usize = 1000;

fn main() {
    let mem_beg = resident_memory();
    let space = common::MM.clone();
    let mem_one = resident_memory();
    println!("candidate size: {} bytes", mem_one - mem_beg);
    let copies = (0..NUM_DESCENTS).map(|_| {
        common::descend_with_copies(space.clone())
    }).collect_vec();
    let mem_descents = resident_memory();
    let mem_per_descent = (mem_descents - mem_one)/NUM_DESCENTS;
    println!("memory used per descent: {} bytes", mem_per_descent);
    std::mem::drop(copies);
}
