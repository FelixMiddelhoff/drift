# Unreal walkthrough: adding drift to a project

Full step-by-step example: taking a real Unreal C++ project from zero to a `drift-unreal-lint` run wired into CI, using a small `AMyPawn`-style class with a `Tick` function as the running example. Every command below is real — this is the same path used to dogfood this tool against Lyra (see the [rule catalog](./rule-catalog.md) for those results).

## 1. Prerequisites

- An Unreal Engine C++ project (Epic Games Launcher build or source build) with a `.uproject` file.
- LLVM/Clang installed separately from the engine's own bundled compiler — `drift-unreal-lint` uses stock `libclang` directly, not UBT's compiler. On Windows, `winget install LLVM.LLVM` is enough; you only need `libclang.dll`/`libclang.lib`, not a full LLVM toolchain build.
- A Rust toolchain (`rustup`) to build `drift-unreal-lint` itself — it isn't published as a prebuilt binary yet.

Clone drift alongside (or inside) your engine workspace:

```bash
git clone https://github.com/FelixMiddelhoff/drift
```

## 2. Generate a compile database

`drift-unreal-lint` reads a standard [JSON Compilation Database](https://clang.llvm.org/docs/JSONCompilationDatabase.html) — the same format clangd/clang-tidy use. UnrealBuildTool generates one directly:

```bash
"<EnginePath>/Engine/Binaries/DotNET/UnrealBuildTool/UnrealBuildTool.exe" \
  -Mode=GenerateClangDatabase -Project="<YourProject>.uproject" <Target> Win64 Development
```

Replace `<Target>` with your project's editor target (e.g. `MyProjectEditor`). This writes `compile_commands.json` at your project root, one entry per translation unit UBT knows how to build.

## 3. Build drift-unreal-lint

```bash
cd drift/bindings/unreal/drift-unreal-lint
cargo build --release
```

Point `LIBCLANG_PATH` at your LLVM install's `bin` directory so the `clang` crate can find `libclang.dll`/`.so`/`.dylib` — both for this build step (not strictly required to build, only to run) and for every run below:

```bash
export LIBCLANG_PATH="C:/Program Files/LLVM/bin"   # adjust for your platform/install
```

## 4. First run: unconditional rules only

Five of the six rules — `unseeded_rng`, `wallclock_read`, `hashmap_iter`, `unordered_parallelism`, `usize_in_hashed_state` — need no configuration and run the moment you point the tool at a compile database:

```bash
./target/release/drift-unreal-lint <YourProject>/compile_commands.json
```

Example output against a project with a raw `FMath::FRand()` call and a `TMap` iterated in a range-based for:

```
Source/MyProject/MyPawn.cpp:42:9: warning: FRand() reads Unreal's global RNG, which is not seeded deterministically by default; differs per peer/run [drift-unreal::unseeded_rng]
Source/MyProject/InventoryComponent.cpp:88:5: warning: iterating a TMap/TSet — order is not guaranteed stable across peers [drift-unreal::hashmap_iter]
```

The process exits `1` if there are any findings, `0` if clean — plug it straight into a CI gate on that alone.

## 5. Fixing a real finding

Take the `unseeded_rng` hit above. `FMath::FRand()` reads Unreal's process-global RNG, seeded from OS entropy by default — two peers in a lockstep session get different values from the same call. Fix: use an explicitly-seeded `FRandomStream` that's part of your replicated/synchronized simulation state instead:

```cpp
// Before — flagged
float Roll = FMath::FRand();

// After — deterministic, seed is part of simulation state
float Roll = SimRandomStream.FRand();
```

Re-run `drift-unreal-lint` — that finding is gone; `SimRandomStream.FRand()` isn't one of the flagged call spellings (`FMath::*`), because it's no longer reading the shared global state.

## 6. Enabling float_outside_fixed_step

The sixth rule, `float_outside_fixed_step`, is opt-in: it flags float/double arithmetic (`+ - * /`) reachable from your simulation's tick functions, because IEEE-754 float addition/multiplication isn't associative — the same expression can produce a different bit pattern on different platforms/compilers depending on evaluation order, a real desync source in a lockstep sim. Unscoped, this would flag nearly every float operation in a typical Unreal codebase, so it does nothing until you tell it where your simulation's fixed-tick entry points are.

Create `config.toml` next to (or anywhere, and pass its path explicitly) your compile database:

```toml
tick_reachable_roots = ["AMyPawn::Tick"]
fixed_step_functions = ["UMyIntegrator::Step"]
```

- `tick_reachable_roots`: the functions where your fixed-step simulation begins. `drift-unreal-lint` builds a call graph from every translation unit and does a reachability walk from these roots — any float arithmetic in a function reachable from here gets flagged.
- `fixed_step_functions`: an exemption list — functions you've already verified use a deterministic, fixed-order integrator (e.g. a `Step(float FixedDelta)` you've audited) and don't want re-flagged.

Run again with the config:

```bash
./target/release/drift-unreal-lint <YourProject>/compile_commands.json config.toml
```

```
Source/MyProject/MyPawn.cpp:57:21: warning: float arithmetic reachable from tick-reachable code; non-associative reordering can desync across platforms [drift-unreal::float_outside_fixed_step]
```

That's `AMyPawn::Tick` calling into a helper that does `NewPosition = Position + Velocity * DeltaTime` — worth checking whether `DeltaTime` here is your fixed simulation step or the engine's variable frame delta (a common real bug: mixing `DeltaSeconds` from `Tick(float DeltaSeconds)` into simulated state instead of a fixed timestep accumulator).

## 7. Performance on a full codebase

Each translation unit parses in its own worker subprocess — real cost: a full 388-file Lyra sweep hit a genuine libclang out-of-memory crash on one file before this existed, which used to take the whole run down with it (see the [rule catalog](./rule-catalog.md) for the full story). Now a crashed worker only costs that one file's findings.

Workers run concurrently by default (sized to your CPU's `available_parallelism`). On a memory-constrained machine, cap it:

```bash
DRIFT_UNREAL_JOBS=4 ./target/release/drift-unreal-lint <YourProject>/compile_commands.json config.toml
```

Set `DRIFT_UNREAL_STATS=1` to print a summary (translation units parsed, call edges, float candidates found) to stderr — useful to confirm a full sweep actually completed before trusting a "zero findings" result.

## 8. Wiring into CI

A minimal GitHub Actions job — adjust the LLVM install step for your runner OS:

```yaml
jobs:
  drift-unreal:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Install LLVM
        run: choco install llvm -y
      - name: Build drift-unreal-lint
        run: cargo build --release --manifest-path drift/bindings/unreal/drift-unreal-lint/Cargo.toml
      - name: Generate compile database
        run: |
          "<EnginePath>/Engine/Binaries/DotNET/UnrealBuildTool/UnrealBuildTool.exe" `
            -Mode=GenerateClangDatabase -Project="MyProject.uproject" MyProjectEditor Win64 Development
      - name: Run drift-unreal-lint
        env:
          LIBCLANG_PATH: "C:/Program Files/LLVM/bin"
        run: drift/bindings/unreal/drift-unreal-lint/target/release/drift-unreal-lint.exe compile_commands.json drift-config.toml
```

The non-zero exit code on any finding fails the job — no extra plumbing needed. Commit `drift-config.toml` (your `tick_reachable_roots`/`fixed_step_functions`) to your project's own repo so it evolves alongside your simulation code.

## Known limitations

See the [rule catalog](./rule-catalog.md#unreal-c-bindingsunrealdrift-unreal-lint) for the full, honest list per rule — worth reading before relying on a clean run as proof of correctness. The short version: call-site matching is syntactic (spelling-based, not full symbol resolution) for `unseeded_rng`/`wallclock_read`/`unordered_parallelism`, and the reachability call graph is built per-translation-unit then merged, which can miss edges through virtual dispatch or function pointers.
