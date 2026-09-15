# Drift

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Static lints that catch game-simulation non-determinism — `HashMap`/`HashSet` iteration, unseeded RNG, wall-clock reads, unordered parallelism, pointer-width fields on hashed state — before they cause a lockstep/rollback-netcode desync, instead of debugging the desync after the fact.

Companion to [Foldback](https://github.com/FelixMiddelhoff/foldback) (runtime desync detection and bisection): drift prevents what it can at build time, Foldback finds what slips through at runtime.

## Pick your language

| Language | What you get |
|---|---|
| Rust | `crates/drift-lint`, a [dylint](https://github.com/trailofbits/dylint) lint library, 5 rules |
| C# / Unity | `bindings/csharp/Drift.Analyzers`, a Roslyn analyzer, 5 rules (DRIFT0001–0005) — works in any C# project; Unity-specific rules are scoped to `UnityEngine.*` types |
| C++ / Unreal | `bindings/unreal/drift-unreal-lint`, a standalone binary over stock LLVM/Clang (no engine fork needed), 2 rules against a project's `compile_commands.json` |

Full rule reference, one entry per rule with a real example and fix: [docs/rule-catalog.md](docs/rule-catalog.md).

## Rust quickstart

```bash
cargo install cargo-dylint dylint-link
```

```bash
cargo dylint --path crates/drift-lint --workspace
```

(`--path` works against a local clone; a `--git https://github.com/FelixMiddelhoff/drift` install works the same way once this is pushed. Not published to crates.io — see [docs/rule-catalog.md](docs/rule-catalog.md) for why, and `cargo dylint`'s own docs for the full CLI.)

## C# / Unity quickstart

Reference the built `Drift.Analyzers.dll` as an `Analyzer` item in your `.csproj`:

```xml
<ItemGroup>
  <Analyzer Include="path/to/Drift.Analyzers.dll" />
</ItemGroup>
```

(A NuGet package isn't published yet — `dotnet pack bindings/csharp/Drift.Analyzers` builds one locally in the correct `analyzers/dotnet/cs/` layout that VS/Rider/VS Code auto-load on install.)

## Unreal quickstart

Generate a `compile_commands.json` with UnrealBuildTool, then run `drift-unreal-lint` against it:

```bash
"<EnginePath>/Engine/Binaries/DotNET/UnrealBuildTool/UnrealBuildTool.exe" \
  -Mode=GenerateClangDatabase -Project="<YourProject>.uproject" <Target> Win64 Development

LIBCLANG_PATH="<path to your LLVM install>/bin" \
  cargo run --manifest-path bindings/unreal/drift-unreal-lint/Cargo.toml -- \
  compile_commands.json [config.toml]
```

`unseeded_rng` runs unconditionally. `float_outside_fixed_step` is opt-in — it does nothing until `config.toml` sets `tick_reachable_roots`:

```toml
tick_reachable_roots = ["AMyPawn::Tick"]
fixed_step_functions = ["UMyIntegrator::Step"]
```

Uses stock LLVM/Clang directly (the `clang` crate over libclang) — no forked compiler, no custom clang-tidy check to build. See [docs/rule-catalog.md](docs/rule-catalog.md) for both rules' known limitations.

## License

Licensed under either of

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
