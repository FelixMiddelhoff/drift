# Drift

Static lints that catch game-simulation non-determinism — `HashMap`/`HashSet` iteration, unseeded RNG, wall-clock reads, unordered parallelism, pointer-width fields on hashed state, non-associative float arithmetic reachable from simulation code — before they cause a lockstep/rollback-netcode desync, instead of debugging the desync after the fact.

Companion to [Foldback](https://github.com/FelixMiddelhoff/foldback) (runtime desync detection and bisection): drift prevents what it can at build time, Foldback finds what slips through at runtime.

## Pick your language

| Language | What you get |
|---|---|
| Rust | `crates/drift-lint`, a [dylint](https://github.com/trailofbits/dylint) lint library, 6 rules |
| C# / Unity | `bindings/csharp/Drift.Analyzers`, a Roslyn analyzer, 5 rules (DRIFT0001–0005) — works in any C# project; Unity-specific rules are scoped to `UnityEngine.*` types; `float_outside_fixed_step` not ported here; published on NuGet as [`GameDeterminism.Analyzers`](https://www.nuget.org/packages/GameDeterminism.Analyzers), not `Drift.Analyzers` (see the [C# quickstart](./quickstart-csharp.md) for why) |
| C++ / Unreal | `bindings/unreal/drift-unreal-lint`, a standalone binary over stock LLVM/Clang (no engine fork needed), full 6-rule taxonomy parity, against a project's `compile_commands.json` |
| GDScript / Godot | `bindings/godot/drift-godot-lint`, a standalone binary over [gdck-syntax](https://crates.io/crates/gdck-syntax) (pure Rust, no engine dependency), 4 of 6 rules (`hashmap_iter`/`usize_in_hashed_state` don't apply to GDScript) |

Full rule reference, one entry per rule with a real example and fix: [Rule catalog](./rule-catalog.md).

Pick a quickstart from the sidebar for your language, or jump straight to the [Unreal walkthrough](./unreal-walkthrough.md) for a full step-by-step example.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/FelixMiddelhoff/drift/blob/main/LICENSE-APACHE))
- MIT license ([LICENSE-MIT](https://github.com/FelixMiddelhoff/drift/blob/main/LICENSE-MIT))

at your option.
