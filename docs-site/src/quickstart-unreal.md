# Unreal quickstart

Generate a `compile_commands.json` with UnrealBuildTool, then run `drift-unreal-lint` against it:

```bash
"<EnginePath>/Engine/Binaries/DotNET/UnrealBuildTool/UnrealBuildTool.exe" \
  -Mode=GenerateClangDatabase -Project="<YourProject>.uproject" <Target> Win64 Development

LIBCLANG_PATH="<path to your LLVM install>/bin" \
  cargo run --manifest-path bindings/unreal/drift-unreal-lint/Cargo.toml -- \
  compile_commands.json [config.toml]
```

`unseeded_rng`, `wallclock_read`, `hashmap_iter`, `unordered_parallelism`, and `usize_in_hashed_state` run unconditionally. `float_outside_fixed_step` is opt-in — it does nothing until `config.toml` sets `tick_reachable_roots`:

```toml
tick_reachable_roots = ["AMyPawn::Tick"]
fixed_step_functions = ["UMyIntegrator::Step"]
```

Uses stock LLVM/Clang directly (the `clang` crate over libclang) — no forked compiler, no custom clang-tidy check to build.

## Performance

Each translation unit is parsed in its own worker subprocess (crash isolation — a libclang OOM/segfault on one file costs only that file's findings, not the whole run). Workers run concurrently on a thread pool sized to `std::thread::available_parallelism()`; override with `DRIFT_UNREAL_JOBS=<n>` if you need to cap concurrency (e.g. to control peak memory on a large codebase).

See the [rule catalog](./rule-catalog.md) for each rule's own known limitations, and the [Unreal walkthrough](./unreal-walkthrough.md) for a full step-by-step example against a real project.
