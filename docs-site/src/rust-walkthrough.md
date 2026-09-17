# Rust walkthrough: adding drift to a project

Full step-by-step example, using the repo's own [`examples/minimal-rust`](https://github.com/FelixMiddelhoff/drift/tree/main/examples/minimal-rust) — a real, working crate that intentionally trips every rule once, doubling as this walkthrough's running example and as a manual smoke check for the tool itself.

## 1. Prerequisites

- A Rust toolchain (`rustup`).
- `cargo-dylint` and `dylint-link`, the [dylint](https://github.com/trailofbits/dylint) framework `crates/drift-lint` is built on:

```bash
cargo install cargo-dylint dylint-link
```

Clone drift and look at the demo crate:

```bash
git clone https://github.com/FelixMiddelhoff/drift
cat drift/examples/minimal-rust/src/main.rs
```

## 2. First run: every unconditional rule at once

```bash
cd drift
cargo dylint --path crates/drift-lint -p minimal-rust
```

Five of the six rules — `hashmap_iter`, `unseeded_rng`, `wallclock_read`, `unordered_parallelism`, `usize_in_hashed_state` — need no configuration and fire immediately (real output from this exact command, not paraphrased):

```
warning: usize/isize field on a struct deriving Hash — width varies across platforms
  --> examples\minimal-rust\src\main.rs:20:5
   = help: use a fixed-width integer type (u32/u64/i32/i64) instead

warning: iterating a HashMap/HashSet — order is not guaranteed stable across peers
  --> examples\minimal-rust\src\main.rs:26:25
   = help: use a BTreeMap/BTreeSet, or sort the keys before iterating, if this feeds simulated state

warning: call to an OS-entropy-seeded RNG source
  --> examples\minimal-rust\src\main.rs:31:19
   = help: use an explicit, tracked seed (e.g. StdRng::seed_from_u64) fed by your simulation's deterministic seed

warning: wall-clock read — not guaranteed the same across peers
  --> examples\minimal-rust\src\main.rs:33:18
   = help: use your simulation's own deterministic tick counter if this feeds simulated state

warning: rayon parallel iteration — result order is scheduler-dependent
  --> examples\minimal-rust\src\main.rs:38:23
   = help: confirm the terminal reduction is order-independent (commutative), or suppress if already confirmed
```

Reading `examples/minimal-rust/src/main.rs` alongside the output: each warning points at exactly the line the demo crate built to trigger it — a `#[derive(Hash)] struct Unit { id: usize }`, a `HashMap::iter()`, `rand::random::<u32>()`, `Instant::now()`, and `values.par_iter().sum()`.

## 3. Fixing a real finding

Take the `hashmap_iter` hit. Iterating a `HashMap` directly means two peers running the same logic can see entries in a different order — if that order ever feeds simulated state (accumulating in sequence, picking a "first" element, anything order-sensitive), that's a desync waiting to happen.

```rust
// Before — flagged: iteration order not guaranteed
let units: HashMap<u32, u32> = HashMap::new();
for (_id, _unit) in units.iter() { /* ... */ }

// After — deterministic, sorted by key first
let mut entries: Vec<_> = units.iter().collect();
entries.sort_by_key(|(id, _)| **id);
for (_id, _unit) in entries { /* ... */ }
```

`drift::hashmap_iter` recognizes this collect-then-sort pattern and won't re-flag it — confirmed by the rule's own test fixture, not assumed.

## 4. Enabling `float_outside_fixed_step`

The sixth rule is opt-in — it does nothing until a `dylint.toml` at your workspace root configures `tick_reachable_roots`:

```toml
# dylint.toml
[drift-lint]
tick_reachable_roots = ["main"]
```

This repo's own root `dylint.toml` sets exactly this, which is why `minimal-rust`'s `tick()` function shows up as a sixth warning in the same run above:

```
warning: non-associative float arithmetic reachable from simulation state
  --> examples\minimal-rust\src\main.rs:46:26
46 |     let _next_position = 1.0_f32 + delta * 9.8; // drift::float_outside_fixed_step
   = help: confirm this runs under a fixed-step/fixed-point discipline, or list the enclosing function in dylint.toml's fixed_step_functions
```

**Real, surprising behavior worth knowing before you configure this**: setting `tick_reachable_roots` at all scopes *every* rule except `usize_in_hashed_state` to reachability from those roots, not just `float_outside_fixed_step`. If you point `tick_reachable_roots` at a narrow function, the other five rules can silently stop firing on code that isn't reachable from it — this repo's own demo roots at `main` specifically so every rule's own demo call site stays reachable in one pass. Pick a root that's genuinely upstream of everything you want scanned, not just your physics tick.

```toml
tick_reachable_roots = ["simulation::tick"]
fixed_step_functions = ["physics::integrate"]  # exempt a verified fixed-step integrator
```

## 5. Suppressing a specific finding

```rust
#[allow(drift_hashmap_iter)]
fn known_safe() {
    // ...
}
```

Every rule has its own attribute name (`drift_unseeded_rng`, `drift_wallclock_read`, `drift_unordered_parallelism`, `drift_usize_in_hashed_state`, `drift_float_outside_fixed_step`) — same pattern as any other rustc lint.

## 6. Wiring into CI

```yaml
jobs:
  drift-lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install cargo-dylint dylint-link
      - run: cargo dylint --path crates/drift-lint --workspace
```

`cargo dylint` exits non-zero when there are findings — no extra plumbing needed to fail the job. Commit your `dylint.toml` (if you use `tick_reachable_roots`) alongside your own project so it evolves with your simulation code, same as the [Unreal walkthrough](./unreal-walkthrough.md)'s `config.toml`.

## Known limitations

See the [rule catalog](./rule-catalog.md) for the full, honest list per rule. The short version: reachability is a direct, intra-crate call graph only — `dyn Trait`/function-pointer call targets aren't resolvable via a plain HIR walk, so they don't add an edge. This is a deliberate under-approximation (a missed rule firing is judged less damaging to trust in the tool than a wrong one) — see the rule catalog for why.
