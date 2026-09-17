# Godot quickstart

Grab a prebuilt binary from the [latest release](https://github.com/FelixMiddelhoff/drift/releases/latest) (Linux/macOS/Windows, no toolchain needed), or build from source:

```bash
cargo run --manifest-path bindings/godot/drift-godot-lint/Cargo.toml -- <file.gd | project directory>
```

`unseeded_rng`, `wallclock_read`, and `unordered_parallelism` run unconditionally. `float_outside_fixed_step` runs automatically wherever `_process`/`_physics_process` is defined — no config file needed, unlike the Rust/Unreal sides (Godot's own tick entry points are fixed, well-known method names).

Pure Rust, no engine dependency (via [gdck-syntax](https://crates.io/crates/gdck-syntax)). See the [rule catalog](./rule-catalog.md) for why `hashmap_iter`/`usize_in_hashed_state` don't apply here.
