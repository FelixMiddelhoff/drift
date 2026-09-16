//! One intentional violation of every rule in the v1 catalog
//! (docs/rule-catalog.md) — doubles as a demo and a manual smoke check.
//! Run against it with:
//!   cargo dylint --lib-path <path to the built drift_lint@<toolchain> lib> -p minimal-rust
//!
//! `float_outside_fixed_step` is opt-in — the repo's own root `dylint.toml`
//! sets `tick_reachable_roots = ["main"]` so this demo exercises it too.
//! Real, confirmed behavior worth knowing: configuring
//! `tick_reachable_roots` at all scopes *every* rule except
//! `usize_in_hashed_state`, not just `float_outside_fixed_step` — rooting
//! at `main` (not, say, `tick`) is what keeps every other rule's own demo
//! call site reachable too, so all 6 still fire in one pass.

use rayon::prelude::*;
use std::collections::HashMap;
use std::time::Instant;

#[derive(Hash)]
struct Unit {
    id: usize, // drift::usize_in_hashed_state
}

#[allow(clippy::for_kv_map)] // intentional drift::hashmap_iter demo, not a real bug
fn main() {
    let units: HashMap<u32, u32> = HashMap::new();
    for (_id, _unit) in units.iter() {
        // drift::hashmap_iter
        println!("flagged");
    }

    let _x: u32 = rand::random(); // drift::unseeded_rng

    let _start = Instant::now(); // drift::wallclock_read

    let _u = Unit { id: 0 };

    let values = vec![1, 2, 3];
    let _total: i32 = values.par_iter().sum(); // drift::unordered_parallelism

    tick(0.016);
}

/// Reachable from `main` (this repo's own `tick_reachable_roots`) — the
/// only way `float_outside_fixed_step` fires at all.
fn tick(delta: f32) {
    let _next_position = 1.0_f32 + delta * 9.8; // drift::float_outside_fixed_step
}
