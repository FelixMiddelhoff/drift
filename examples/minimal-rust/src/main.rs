//! One intentional violation of every rule in the v1 catalog
//! (docs/rule-catalog.md) — doubles as a demo and a manual smoke check.
//! Run against it with:
//!   cargo dylint --lib-path <path to the built drift_lint@<toolchain> lib> -p minimal-rust

use std::collections::HashMap;
use std::time::Instant;

#[derive(Hash)]
struct Unit {
    id: usize, // drift::usize_in_hashed_state
}

fn main() {
    let units: HashMap<u32, u32> = HashMap::new();
    for (_id, _unit) in units.iter() {
        // drift::hashmap_iter
        println!("flagged");
    }

    let _x: u32 = rand::random(); // drift::unseeded_rng

    let _start = Instant::now(); // drift::wallclock_read

    let _u = Unit { id: 0 };
}
