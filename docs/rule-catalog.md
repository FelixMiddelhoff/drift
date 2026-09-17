# Rule catalog

One entry per lint. Each rule is `warn`-by-default, not `deny` — see [Suppressing a rule](#suppressing-a-rule).

## Rust (`drift-lint`, via [dylint](https://github.com/trailofbits/dylint))

### Reachability scoping (optional)

By default every rule below fires unconditionally, repo-wide. Add a `dylint.toml` in the target workspace root to scope rules to code actually reachable from your simulation's tick function:

```toml
[drift-lint]
tick_reachable_roots = ["tick"]        # crate-relative, no crate-name prefix — see below
fixed_step_functions = ["physics::integrate"]  # drift::float_outside_fixed_step only
```

`tick_reachable_roots` seeds a reachable-function set built from a direct, intra-crate call graph (function bodies only — `dyn Trait`/fn-pointer call targets aren't traversed, a deliberate under-approximation: a missed rule firing is a smaller problem for a lint tool's adoption than a wrong one). Every rule except `usize_in_hashed_state` (which flags a field definition, not code inside a function — reachability doesn't apply) only fires on code inside a reachable function once this is set.

Root paths are matched against `TyCtxt::def_path_str`, which does **not** include the current crate's own name — `"tick"` for a top-level function, `"module::path::to::fn"` for a nested one, never `"my_crate::tick"`.

### `drift::hashmap_iter`

Flags iterating a `HashMap`/`HashSet` (`.iter()`, `.iter_mut()`, `.into_iter()`, `.keys()`, `.values()`, `.values_mut()`, `.drain()`).

**Why**: iteration order isn't guaranteed stable across peers/platforms/runs. Feeding it into anything that affects simulated state (spawn order, damage application order, event dispatch) is a classic desync source.

```rust
# use std::collections::HashMap;
# let units: HashMap<u32, u32> = HashMap::new();
for (id, _unit) in units.iter() {
    // order of `id` here is not guaranteed the same on every peer
}
```

Fix: use a `BTreeMap`/`BTreeSet`, or collect and sort before iterating:

```rust
# use std::collections::HashMap;
# let units: HashMap<u32, u32> = HashMap::new();
let mut ids: Vec<_> = units.keys().collect();
ids.sort();
```

**Recognizes the collect-then-sort pattern** (found as a real false positive dogfooding against [Foldback](https://github.com/FelixMiddelhoff/foldback)'s `hashable.rs`, now fixed and reverified against that same file — zero hits): a `let` binding whose initializer ends in `.collect()` off a flagged call, immediately followed by a `.sort()`/`.sort_by()`/`.sort_by_key()`/`.sort_unstable()`/`.sort_unstable_by()`/`.sort_unstable_by_key()` on that same binding, doesn't fire:

```rust
# use std::collections::HashMap;
# let self_map: HashMap<u32, u32> = HashMap::new();
let mut entries: Vec<_> = self_map.iter().collect(); // not flagged — sorted right after
entries.sort_by(|a, b| a.0.cmp(b.0));
```

This is pattern-matching one known shape (same statement's binding, very next statement, same block), not real dataflow analysis — sorting a few statements later, through a helper function, or after the value moves into a field still fires. Suppress those with `#[allow(drift_hashmap_iter)]`.

**Dogfood result, real**: run against [`veloren/veloren`](https://github.com/veloren/veloren)'s `common` crate group (a real, shipped open-source multiplayer voxel RPG — 277 `.rs` files across 10 workspace crates: `veloren-common`, `-base`, `-ecs`, `-net`, `-state`, `-systems`, etc.) — 5 real findings, e.g. `common/systems/src/aura.rs:176` and `:180`, both real `HashMap`/`HashSet` iteration inside an ECS aura-application system.

### `drift::unseeded_rng`

Flags `rand::thread_rng()` and `rand::random()`.

**Why**: an RNG seeded from OS entropy differs per peer and per run by construction — any simulation decision that reads from it desyncs immediately.

Fix: use an RNG constructed from an explicit, tracked seed (e.g. `StdRng::seed_from_u64(tick_seed)`) fed by your simulation's own deterministic seed source. Suppress with `#[allow(drift_unseeded_rng)]` for genuinely cosmetic randomness (particle effects, UI flourish).

**Dogfood result, real**: run against `veloren/veloren`'s `common` crate group — 26 real findings, all `rand::random()`, spread across real combat-state files (`leap_explosion_shockwave.rs`, `leap_shockwave.rs`, `rapid_melee.rs`, `shockwave.rs`) — real gameplay-simulation code, not test/example files.

### `drift::wallclock_read`

Flags `std::time::SystemTime::now()` and `std::time::Instant::now()`.

**Why**: two peers read different wall-clock values by construction. Fine for profiling/logging, not fine for anything that affects simulated state.

Fix: use your simulation's own deterministic tick counter. Suppress with `#[allow(drift_wallclock_read)]` at legitimate profiling/logging call sites.

**Dogfood result, real**: run against `veloren/veloren`'s `common` crate group — 18 real findings, including `common/state/src/state.rs:966` (`start: Instant::now()`), a real timing field inside the simulation's own `State` struct.

### `drift::unordered_parallelism`

Flags `.par_iter()`, `.par_iter_mut()`, `.into_par_iter()`, `.par_bridge()` (rayon).

**Why**: work items complete in scheduler-dependent order. A reduction into simulated state that isn't commutative differs between peers whose thread pools schedule differently — a desync that reproduces on one machine and not another.

**Known problem**: doesn't verify the chain actually ends in a non-commutative reduction — a `par_iter()` whose result is genuinely order-independent (e.g. `.sum()`) is a false positive. Suppress with `#[allow(drift_unordered_parallelism)]` once confirmed order-independent.

**Dogfood result, real**: run against `veloren/veloren`'s `common` crate group — 1 real finding, `common/src/store.rs:93` (`self.items.par_iter_mut()`).

### `drift::usize_in_hashed_state`

Flags `usize`/`isize` fields on a struct that also derives `Hash`.

**Why**: `usize`/`isize` are pointer-width (32 vs. 64 bits). A struct hashed for a cross-peer sync check that contains one hashes differently on a 32-bit vs. 64-bit build of the same logical state — a false desync report between two otherwise-correct peers on different architectures.

Fix: use a fixed-width integer type (`u32`/`u64`/`i32`/`i64`) instead. Suppress with `#[allow(drift_usize_in_hashed_state)]` if the struct is only ever hashed for something width-insensitive (e.g. a `HashMap` key never compared across processes). Not scoped by reachability (see above) — it flags a field definition, not code inside a function.

**Dogfood result, real**: run against `veloren/veloren`'s `common` crate group — 2 real findings: `common/src/trade.rs:209` (`pub struct TradeId(usize);`), and `common/src/comp/body/plugin.rs:58` (`pub species: Species` on a `#[derive(Hash)]` struct) — the second one confirms this rule resolves real type aliases through rustc's own type information, not just textual matching: `Species` is `pub type Species = usize;`, only visible by asking the compiler what the field's real type is, not by reading the field declaration's own spelling.

### `drift::float_outside_fixed_step`

Flags non-associative float arithmetic (`+`, `-`, `*`, `/`, `%`) reachable from a `tick_reachable_roots` entry, unless the enclosing function is listed in `fixed_step_functions`.

**Opt-in only, unlike every other rule here** — does nothing at all unless `dylint.toml` configures `tick_reachable_roots`. Without that scoping this rule would blanket-flag nearly every float operation in a typical game codebase, which is exactly why it wasn't in the original v1 catalog — implemented once reachability scoping existed to make it viable, not before.

**Why**: `(a + b) + c` isn't guaranteed to equal `a + (b + c)` in floating-point — operation order, SIMD width, and compiler optimization level can all change the result. Fine inside a controlled fixed-timestep/fixed-point integrator (that's the whole point of `fixed_step_functions`), not fine reachable from simulation state outside one.

A chain like `a + b + c` is deduped to a single warning on the outermost expression, not one per operator.

```toml
[drift-lint]
tick_reachable_roots = ["tick"]
fixed_step_functions = ["physics::integrate"]
```

**Known problem**: `fixed_step_functions` is a blunt, whole-function exemption — it doesn't distinguish genuinely fixed-step-safe arithmetic from fragile arithmetic within the same function, or vice versa. Suppress a specific expression with `#[allow(drift_float_outside_fixed_step)]`.

**Real gap found dogfooding the Godot binding, fixed here too**: compound-assignment accumulation (`total += delta`) is `ExprKind::AssignOp`, a distinct HIR node from `ExprKind::Binary` — the original scan only matched `Binary`, so float drift accumulated via `+=`/`-=`/`*=`/`/=` inside a tick-reachable function went uncaught. Found via a real project (`Orama-Interactive/Pixelorama`)'s own `_marching_ants_time_elapsed += delta` inside a Godot `_process`; confirmed the same gap existed here and in the Unreal binding, fixed in all three. `AssignOp` is matched via `BinOpKind::from(AssignOpKind)` and is never subject to the chain-dedup check above (it can't be a chain operand — its type is `()`).

**Not exercised in the `veloren/veloren` dogfood above** — this rule is opt-in and no `dylint.toml`/`tick_reachable_roots` was configured for that external target (`veloren/veloren` doesn't ship a `dylint.toml`, and adding one to a third-party project's own simulation roots is out of scope for a dogfood run), so it correctly found zero hits, not evidence of anything either way. `crates/drift-lint/tests/reachability_fixture` (a real `cargo dylint` invocation, not a `ui_test`) is this rule's own positive/negative-verified regression test instead.

## Suppressing a rule

Every rule is a plain rustc lint under the hood — suppress the normal way:

```rust,ignore
#[allow(drift_hashmap_iter)]
fn ui_only_function() { /* ... */ }
```

## C#/Unity (`Drift.Analyzers`)

A Roslyn analyzer, `bindings/csharp/Drift.Analyzers` — works in any C# project, not just Unity (Unity-specific rules are simply scoped to `UnityEngine.*` types and still just ordinary C# analysis). Install via `dotnet add package GameDeterminism.Analyzers` (published under that name, not `Drift.Analyzers` — `Drift` is a NuGet ID prefix reserved by an unrelated company), or by referencing the built `Drift.Analyzers.dll` directly as an `Analyzer` item.

### `DRIFT0001` — Dictionary/HashSet iteration order

Flags `foreach` over a `Dictionary<TKey, TValue>`/`HashSet<T>` (including their `.Keys`/`.Values` collections).

Same rationale as `drift::hashmap_iter`. Fix: use a `SortedDictionary`/`SortedSet`, or sort before iterating.

Also recognizes the non-generic `System.Collections.IDictionary` interface (a real gap once suspected dogfooding against Foldback's Unity binding — its `FoldbackReflection.cs` pattern-matches `is IDictionary dict` — but re-verified by reading the actual code, not just grepping for the type name: that code never bare-`foreach`es the dictionary, it sorts `dict.Keys` by string first, the same safe pattern `drift::hashmap_iter`'s own collect-then-sort recognition covers on the Rust side — so this was never a real false negative there, just a plausible-looking one from an incomplete first read). Deliberately **not** extended to the generic `IDictionary<TKey,TValue>` interface — `SortedDictionary<TKey,TValue>` also implements it, so flagging the interface itself would be a new false positive on genuinely ordered code reached only through it.

### `DRIFT0002` — RNG seeded from OS/engine entropy

Flags `new System.Random()` (parameterless — seeded from `Environment.TickCount`) and any `UnityEngine.Random.*` member access except `InitState`/`state` (the deterministic-seeding mechanism itself, not a violation).

Same rationale as `drift::unseeded_rng`. Fix: `new Random(seed)`, or `UnityEngine.Random.InitState(seed)`, fed by your simulation's deterministic seed.

### `DRIFT0003` — Wall-clock read

Flags `DateTime.Now`/`DateTime.UtcNow`, `Environment.TickCount`, and `UnityEngine.Time.realtimeSinceStartup`.

Same rationale as `drift::wallclock_read`. Fix: use your simulation's own deterministic tick counter.

### `DRIFT0004` — Parallel iteration result order

Flags `.AsParallel()` (PLINQ) and `Parallel.ForEach`/`Parallel.For` (`System.Threading.Tasks`).

Same rationale as `drift::unordered_parallelism`, targeting .NET's own parallelism primitives instead of rayon. Same known problem: doesn't verify the terminal reduction is actually commutative.

### `DRIFT0005` — pointer-width member used in hashing

Flags `nint`/`nuint` (and the older `IntPtr`/`UIntPtr` spellings) members two ways:
1. Declared on a `record`/`record struct` — positional parameters, fields, and properties. A C# `record`'s compiler-generated `GetHashCode`/`Equals` are derived from every member, the closest real analogue to Rust's `#[derive(Hash)]`.
2. Referenced anywhere in the body of a hand-written `GetHashCode()` override on a plain class/struct — not full data-flow analysis (it doesn't prove the reference actually feeds the returned hash, just that a hash method touches the field at all), but real and syntactic, not guessed at. Both a block body (`{ ... }`) and an expression body (`=> ...`) are checked.

Same rationale as `drift::usize_in_hashed_state`: `nint`/`nuint` are pointer-width (32 vs. 64 bits), so hashing one produces a different result on a 32-bit vs. 64-bit build of the same logical state.

**Known limitation**: the hand-written-`GetHashCode` check is syntactic reference detection, not real data-flow — a field merely *read* inside `GetHashCode` (logged, asserted, whatever) but not actually folded into the hash gets flagged too. Suppress with `#pragma warning disable DRIFT0005` at specific false positives.

### Suppressing a C# rule

```csharp
#pragma warning disable DRIFT0001
// ...
#pragma warning restore DRIFT0001
```

### Dogfood result

Run against Foldback's real Unity binding (`bindings/unity/Runtime/FoldbackReflection.cs`, `bindings/unity/Tests~/FoldbackSys.Tests/Program.cs`) with all 5 rules: zero hits. Genuinely clean, re-verified by reading the code, not just grepping type names — see DRIFT0001 above for the one case that looked like a gap on a first pass and wasn't on a closer read.

## Unreal C++ (`bindings/unreal/drift-unreal-lint`)

A standalone Rust binary using [libclang](https://clang.llvm.org/docs/Tooling.html) directly (the `clang` crate) against a project's `compile_commands.json` — stock LLVM/Clang, no engine fork, no custom clang-tidy check compiled into LLVM (see [drift-godot-unreal-plan.md §6](https://github.com/FelixMiddelhoff/drift) for why: the prebuilt LLVM package ships `clang-c` headers + `libclang.lib` only, not the full LibTooling headers a real clang-tidy check needs). Real UnrealBuildTool `compile_commands.json` entries are `clang-cl.exe @file.rsp`, and `file.rsp` itself nests a second `@...Shared.rsp` — both response files are expanded recursively, and `--driver-mode=cl` is added automatically when the original compiler was `clang-cl` so its MSVC-style flags (`/FI`, `/Fo`, `/clang:...`) parse correctly.

Build: `cargo build` inside `bindings/unreal/drift-unreal-lint`, with `LIBCLANG_PATH` pointing at your LLVM install's `bin` directory (e.g. `C:\Program Files\LLVM\bin`). Run: `drift-unreal-lint <compile_commands.json> [config.toml]` — `unseeded_rng`, `wallclock_read`, `hashmap_iter`, `unordered_parallelism`, and `usize_in_hashed_state` always run; `float_outside_fixed_step` only runs once `config.toml` sets `tick_reachable_roots`.

**Real, disclosed gap**: unlike the Rust side's `#[allow(drift_x)]`, there is no suppression mechanism here — no inline comment, no config-level allowlist. A confirmed false positive today means restructuring the code or not failing CI on that finding.

**Two more real bugs found dogfooding against real Lyra source, past what the initial spike caught**: (1) the JSON Compilation Database spec requires relative paths inside `arguments`/`command` to resolve against that entry's own `directory` field, not the caller's cwd — real UBT response files use relative `-I../Plugins/...` include paths meant to resolve against `Engine/Source`. Without `chdir`-ing into `directory` before each parse, every file transitively including a plugin header (e.g. `Abilities/GameplayAbility.h`) hit a **fatal** "file not found" partway through, silently truncating that TU's AST to whatever came before the failure — this was the reason an earlier version of this doc claimed a clean-but-empty dogfood result; the truncation was real, the "clean" part wasn't. (2) once real headers actually resolved, this LLVM install's own AVX512 intrinsic headers reference builtins this exact clang frontend doesn't implement (a handful of real but irrelevant errors confined to system headers) — hitting clang's default `-ferror-limit=20` and aborting the whole parse before ever reaching the target file's own code. Fixed by passing `-ferror-limit=0`, the standard fix for exactly this in static-analysis tooling: keep going past irrelevant header noise instead of giving up on the whole TU.

**Real, disclosed cost, now measured to a real conclusion, and fixed**: once headers actually resolve, parsing real Unreal C++ per translation unit is genuinely expensive without a precompiled header (well known — this is also why full UE rebuilds are slow) — a single real file's own full include tree was ~20,000 function/method definitions. A full-codebase sweep of Lyra's 388 translation units was run to completion this session (two `tick_reachable_roots` configured together: `ULyraRangedWeaponInstance::Tick`, `ALyraWeaponSpawner::Tick`) and, on the first attempt, turned out **not just slow, it crashed**: ran 22m27s, then a real libclang fatal error (`LLVM ERROR: out of memory` / `Buffer allocation failed`) while parsing `Plugins/UIExtension/Source/Private/Widgets/UIExtensionPointWidget.cpp`, followed by a process-level segmentation fault (not a clean non-zero exit) rather than the crash-recovery path (`Err(_) => continue`, already in `run()`) absorbing it and moving on. Because findings were only printed after every TU finished (no streaming), the crash produced **zero output** despite most of the 388 files likely having parsed successfully before it.

**Real fix**: `run()` no longer parses every TU in one shared process. It now spawns itself once per `compile_commands.json` entry (`--worker <index> <compile_commands.json> [config.toml]`, a hidden mode), and each worker parses exactly one TU in its own fresh `Clang`/`Index`, printing its findings — plus, when `tick_reachable_roots` is configured, its own call edges and every one of its own functions' float-chain findings (tagged by owning function, unfiltered by reachability, since a single TU can't know the whole program's call graph) — as JSON to stdout. The orchestrator aggregates every worker's output, computes the cross-TU reachable set from the aggregated edges exactly as before, and only then filters the float-chain candidates by reachability. A worker that crashes (real OOM, a genuine segfault, anything) now only costs its own file's findings — reported to stderr, not silently dropped — and the orchestrator, in a separate untouched process, always reaches the end and prints everything every other worker found.

**Real result, re-run after the fix**: same full 388-file sweep, same two roots — completed cleanly this time, **388/388 translation units parsed, zero worker failures** (the file that previously triggered the OOM no longer appears in any crash/failure report). 250 real findings across 5 of the 6 rules (`usize_in_hashed_state` found zero real hits this run — a real, disclosed gap, see its own entry above, not evidence of a bug): 15 `float_outside_fixed_step`, 136 `hashmap_iter`, 4 `unordered_parallelism`, 13 `unseeded_rng`, 82 `wallclock_read` — spot-checked, not just counted: `float_outside_fixed_step` fired on both configured roots' real code (`LyraRangedWeaponInstance.cpp`, and a new hit in `LyraWeaponSpawner.cpp:88` not seen in the earlier single-root isolated dogfood), `unordered_parallelism` fired on real header-internal `ParallelFor`/`UE::Tasks::Launch` usage. **Real, disclosed trade-off, since fixed**: on the first (sequential) fix, wall-clock went from 22m27s-before-crashing to 70m3s to a real completion — one worker process (and its own LLVM/libclang initialization) per file, run one at a time, is real overhead the old single-shared-process design didn't pay. `run()`'s orchestrator now runs a work-stealing thread pool of concurrent worker processes instead of spawning them one at a time — pool size defaults to `std::thread::available_parallelism()`, overridable via `DRIFT_UNREAL_JOBS`. Crash isolation is unaffected (each worker is still its own OS process; a pool just controls how many run at once), and result aggregation is order-independent (findings are sorted/deduped after collection regardless of which worker finishes first). The per-file dogfood results throughout this doc's "Unreal C++" section remain real, complete, and unaffected by any of this — each ran against an isolated single- or few-file compile database, never anywhere near this memory pressure.

### Reachability scoping (required — `float_outside_fixed_step` only)

Same rationale as the Rust side's `dylint.toml`: unscoped, this would blanket-flag nearly every float operation in a typical Unreal codebase. Without a config file (or an empty one), only `unseeded_rng` runs.

```toml
tick_reachable_roots = ["ALyraWeaponSpawner::Tick"]
fixed_step_functions = ["UMyIntegrator::Step"]
```

`tick_reachable_roots` seeds a reachable-function set built from a real, cross-translation-unit call graph (functions matched by `Namespace::Class::Method`-style qualified name, edges from every `CallExpr` in every parsed TU). Reachability stops at any function whose definition isn't in one of the TUs actually parsed — real Engine-internals calls (a TU outside the target project's own `compile_commands.json`) don't extend the graph further, a real, disclosed limitation, not a silent one.

### `drift-unreal::unseeded_rng`

Flags calls to Unreal's global RNG (`FMath::Rand`, `RandRange`, `RandHelper`, `FRand`, `FRandRange`, `RandBool`, `VRand`, `VRandCone`, `VRandCone2D`), seeded from OS/engine entropy unless the project explicitly tracks a seed. Same rationale as `drift::unseeded_rng`. **Fires unconditionally**, no reachability scoping needed — unlike float arithmetic, a raw call to one of these is always worth flagging.

Matched against the call's **own source spelling**, not the resolved declaration's qualified name — a real bug hit building this: `FMath` (`Engine/.../UnrealMathUtility.h`) is `struct FMath : public FPlatformMath`, and `Rand`/`FRand` are actually declared on the base (a platform-specific typedef of `FGenericPlatformMath`), so the resolved declaration's qualified name is *not* `FMath::...`. Tokenizing the call site directly (up to the opening `(`) sidesteps the inheritance chain entirely.

**Known limitation**: scans the whole translation unit, headers included — a call inside a header (e.g. Unreal's own `FRandomStream::FRand()` implementation) fires too, not just application code. No attempt is made to distinguish "your project's code" from "included engine code," unlike the Rust rule which naturally only sees the current crate.

**Dogfood result, real**: run against `LyraGameplayAbility_RangedWeapon.cpp` (a real file, isolated from the full 388-entry compile database for a fast, complete-headers dogfood run once the two bugs above were fixed) — found exactly the 3 real call sites confirmed by `grep` beforehand (`FMath::FRand()` ×2, `FMath::Rand()` ×1) at their exact real line numbers, plus one real header-internal hit inside `RandomStream.h` (see limitation above, not a bug).

### `drift-unreal::wallclock_read`

Flags calls to `FPlatformTime::Seconds`/`Cycles`/`Cycles64` and `FDateTime::Now`/`UtcNow` — a value that differs per peer/run must never feed simulated state. Same rationale as `drift::wallclock_read`/`DRIFT0003`. **Fires unconditionally**, no reachability scoping needed, same as `unseeded_rng`.

Matched against the call's own source spelling, not the resolved declaration — verified, not assumed, before building: `FPlatformTime` (`HAL/PlatformTime.h`) is a platform `typedef` (e.g. `typedef FWindowsPlatformTime FPlatformTime;` on Windows), and `FWindowsPlatformTime::Seconds`/`Cycles`/`Cycles64` are declared directly on that platform struct rather than inherited — so `FPlatformTime::Seconds()`'s resolved declaration has a different qualified name (`FWindowsPlatformTime::Seconds`) than the call-site text, same shape as `unseeded_rng`'s own `FMath` base-class split. `FDateTime::Now`/`UtcNow` don't have this split (`FDateTime` declares them directly), but are matched the same way for consistency.

**Known limitation**: same as `unseeded_rng` — scans the whole translation unit, headers included, no attempt to distinguish project code from included engine code.

**Real bug found dogfooding, fixed**: a macro-expanded argument re-evaluates the same call-site AST node more than once at the identical `file:line:column` — confirmed against real Lyra source, `LyraAssetManagerStartupJob.cpp`'s `UE_LOG(..., FPlatformTime::Seconds() - JobStartTime)` reported the same location 3 times before a fix. All findings are now deduped by exact `(file, line, column, rule)` before printing — collapses only true duplicates, leaves genuinely distinct call sites (different columns) untouched.

**Dogfood result, real**: run against `LyraAssetManagerStartupJob.cpp`/`.h` (isolated single-file compile database) — found exactly the 3 real `FPlatformTime::Seconds()` call sites `grep` had confirmed beforehand (`.cpp` lines 9 and 22, `.h` line 38), plus real header-internal hits inside `RandomStream.h`, `AutomationEvent.h`, `StatsSystemTypes.h` (see limitation above, not a bug).

### `drift-unreal::hashmap_iter`

Flags a range-based-for or `.CreateIterator()`/`.CreateConstIterator()` call over a `TMap`/`TSet` — both are hash-backed (`Containers/Map.h`/`Set.h`, confirmed by reading the real headers, which select between `TSparseSet`/`TCompactSet` internally), not insertion-ordered by any engine guarantee. Same rationale as `drift::hashmap_iter`/`DRIFT0001`. **Fires unconditionally**, no reachability scoping needed. Deliberately excludes `TSortedMap`/`TSortedSet`/`TMultiMap` — matches the plan's own taxonomy entry, not every hash-adjacent container.

**Detected by type, not call-site spelling** — a real, deliberate difference from the other two unconditional rules here: a range-based-for's compiler-desugared AST (confirmed with a real `-ast-dump`, not assumed) exposes the synthesized `auto&& __range = <container>;` binding one level below the `ForRangeStmt`'s own immediate children, so its type — the container's own type — is checked there rather than at the `ForRangeStmt` cursor directly. `.CreateIterator()`/`.CreateConstIterator()` calls are matched by their receiver's type the same way.

**A real consequence of type-based over spelling-based detection**: this rule naturally avoids the collect-then-sort false positive the Rust side's own `drift::hashmap_iter` had to special-case (a `TArray` built from a map's keys/values, sorted immediately after, then iterated) — the sorted result is a `TArray`, a different type than `TMap`/`TSet`, so it never matches in the first place. Verified in the fixture (`tests/fixture/pawn.cpp`'s `IterateSortedArray`), not just assumed to follow from the design.

**Known limitation**: same as `unseeded_rng`/`wallclock_read` — scans the whole translation unit, headers included, no attempt to distinguish project code from included engine code (real UE headers like `Containers/LruCache.h` fire too, expected).

**Dogfood result, real**: run against `LyraGamePhaseSubsystem.cpp` and `LyraTeamSubsystem.cpp` (isolated two-file compile database) — found exactly the 3 real range-based-for-over-`TMap` call sites `grep` had confirmed beforehand (`ActivePhaseMap` at lines 124 and 147, `TeamMap` at line 392), plus real header-internal hits (see limitation above, not a bug).

### `drift-unreal::unordered_parallelism`

Flags `ParallelFor`, `ParallelForWithTaskContext`, and `UE::Tasks::Launch` — parallel dispatch whose results, folded into simulated state without a deterministic reduction, aren't proven commutative. Same known-problem caveat as `drift::unordered_parallelism`/`DRIFT0004`, inherited rather than re-litigated. Matched by call-site spelling, same shape as `unseeded_rng`/`wallclock_read`. **Fires unconditionally**, no reachability scoping needed.

**Deliberately excludes `AsyncTask`** — a real decision backed by evidence, not the taxonomy-symmetry default the plan warned against: since Lyra itself has zero confirmed call sites for any of these (a real, disclosed gap — see below), real non-Lyra Engine source was checked instead. Every real `AsyncTask` call site found (`AndroidPlatformMemory.cpp`'s memory warning, `ConfigContext.cpp`'s deprecation message, `IPlatformFileManagedStorageWrapper.h`'s background file op) was UI/logging/IO work, not simulated state — including it in v1 would flag far more non-hazards than `ParallelFor` would. Deferred the same way `float_outside_fixed_step` was itself once deferred: on real evidence against inclusion, not a guess. Revisit if a real project surfaces an `AsyncTask` call site that actually touches simulated state.

**Real, disclosed gap**: unlike the other three unconditional rules, Lyra's own source has zero confirmed call sites for any of `ParallelFor`/`ParallelForWithTaskContext`/`UE::Tasks::Launch` — not evidence the rule is unneeded (these are standard, documented UE parallelism primitives used throughout the wider engine and in larger real projects), just evidence Lyra isn't the dogfood target that proves this one out. Validated instead with a real positive/negative fixture (`tests/fixture/pawn.cpp`): `RunParallel` (calls `ParallelFor`) fires, `LogAsync` (calls the deliberately-excluded `AsyncTask`) does not.

### `drift-unreal::usize_in_hashed_state`

Flags `SIZE_T`/`size_t`/`uintptr_t`/`intptr_t`-typed struct/class fields referenced inside a `GetTypeHash` overload for that struct — pointer-width types are 32-bit on Win32, 64-bit on Win64/most platforms, exactly the Rust `usize`/C# `nint`/`nuint` hazard `drift::usize_in_hashed_state`/`DRIFT0005` already flag. **Fires unconditionally**, no reachability scoping needed.

**Implementation mirrors C#'s `DRIFT0005` more than the Rust side's own struct-derive-attribute scan**, since Unreal has no `#[derive(Hash)]`/`record` equivalent — hashing is always a hand-written free function `GetTypeHash(const T&)`, found via Argument-Dependent Lookup: (1) find pointer-width-typed fields, keyed by their containing struct's own qualified name; (2) for each `GetTypeHash` overload with exactly one parameter, resolve that parameter's pointee type's declaration to the same lookup key, and scan the function body for a member reference to one of that struct's flagged fields. Same syntactic, conservative reference-detection C#'s own version already uses (not real dataflow) — a field merely read, not actually folded into the returned hash, still gets flagged, a known, inherited limitation.

**Real, disclosed gap**: same as `unordered_parallelism` — Lyra's own `LyraGame` source has zero confirmed `SIZE_T`/`GetTypeHash` references (a gameplay sample isn't the kind of project that hand-rolls hash functions on pointer-width fields). The hazard is real in principle, just unconfirmed against this specific dogfood target. Validated instead with a real positive/negative fixture (`tests/fixture/pawn.cpp`): `FUnitId::Handle` (`SIZE_T`, referenced inside its own `GetTypeHash` overload) fires; `FUnusedHandle::RawHandle` (`SIZE_T`, but never referenced inside any `GetTypeHash`) does not.

### `drift-unreal::float_outside_fixed_step`

Flags non-associative float/double arithmetic (`+`, `-`, `*`, `/`) reachable from a `tick_reachable_roots` entry, unless the enclosing function is listed in `fixed_step_functions`. Same rationale as the Rust/C# rule of the same name. A chain like `a + b + c` is deduped to one warning on the outermost expression, not one per operator — verified with a real positive/negative fixture (`tests/fixture/pawn.cpp`): a `Tick`→`Simulate` call chain with two real float-chain statements fires exactly twice (once per statement, not once per operator), and a same-shaped `NotReached` function never called from `Tick` fires zero times.

**Known limitation, real not assumed**: `UPROPERTY`/`UFUNCTION`/`USTRUCT`/etc. specifiers (`EditAnywhere`, `Replicated`, `Category`, ...) are completely invisible to this tool — confirmed by reading `ObjectMacros.h` directly, they're `#define X(...)` (expand to nothing) under normal compilation, only read by UnrealHeaderTool separately. This rule doesn't need that metadata (it only looks at arithmetic and call graphs), so it's unaffected — but a future rule that needed to know "is this field actually replicated" would need a different approach (parsing `.generated.h` output, or a real UE-aware fork like RedpointGames') than this tool's own stock-Clang path.

**Dogfood result, real, against a root that actually reaches float arithmetic**: `ULyraRangedWeaponInstance::Tick` (isolated single-file compile database, `tick_reachable_roots = ["ULyraRangedWeaponInstance::Tick"]`) — `Tick` calls `UpdateSpread`/`UpdateMultipliers`, both real, non-synthetic float arithmetic (spread-cooldown decay, movement/crouch/jump spread multipliers smoothly interpolated every tick). Found 4 real findings in `LyraRangedWeaponInstance.cpp` at lines 155, 177, 181, 212 (e.g. `CurrentHeat - (CooldownRate * DeltaSeconds)`), plus one real header-internal hit inside `World.h` (see the header-scanning limitation the other rules share, not repeated here). Closes the gap the earlier `ALyraWeaponSpawner::Tick` dogfood left open (that root legitimately found zero, since it only calls `Super::Tick` — a correct-but-unconvincing result on its own).

**Real gap found dogfooding the Godot binding, fixed here too**: compound-assignment accumulation (`CurrentValue += Delta`) is libclang's `EntityKind::CompoundAssignOperator`, a distinct node from `EntityKind::BinaryOperator` — the original scan only walked `BinaryOperator`. Fixed by also matching `CompoundAssignOperator` against a `COMPOUND_ARITHMETIC_OPS` set (`+=`, `-=`, `*=`, `/=`). See the Rust rule's own entry above for the real, non-synthetic example that surfaced this (`Orama-Interactive/Pixelorama`).

## Godot GDScript (`bindings/godot/drift-godot-lint`)

A standalone Rust binary using [`gdck-syntax`](https://crates.io/crates/gdck-syntax) (a real, lossless, pure-Rust GDScript 4 parser — no engine dependency, no `.gdextension`) directly against a project's `.gd` files. Foundation validated by a real feasibility spike first (see [drift-godot-unreal-plan.md §9](https://github.com/FelixMiddelhoff/drift) for the full spike write-up): 468/470 files of a real, substantial Godot 4 game (`SlayHorizon/godot-tiny-mmo`) parsed clean. A second, unrelated real project — [`Orama-Interactive/Pixelorama`](https://github.com/Orama-Interactive/Pixelorama), a shipped pixel-art editor (250 `.gd` files, ~59.5k lines, Godot 4.7) — parsed 250/250 clean, 0 parse errors, run under `DRIFT_GODOT_STATS=1` to confirm rather than assume.

Build: `cargo build` inside `bindings/godot/drift-godot-lint` — no external toolchain needed (pure Rust, unlike the Unreal binding's libclang dependency). Run: `drift-godot-lint <file.gd | project directory>` — `unseeded_rng`, `wallclock_read`, and `unordered_parallelism` always run; `float_outside_fixed_step` runs automatically wherever `_process`/`_physics_process` is defined, no config file needed (see its own entry below for why). A directory is walked recursively, skipping `.godot` (Godot's own editor cache, never real project source).

**v1 scope, checked against the shared taxonomy, not copy-pasted**: 4 of the 6 rules port. `hashmap_iter` does **not** port — Godot's own `Dictionary` is insertion-ordered by engine guarantee (confirmed independently by Foldback's own reflective-hashing work), so it isn't the hazard Rust's `HashMap`/C#'s `Dictionary`/Unreal's `TMap`/`TSet` are. `usize_in_hashed_state` is **N/A**, not deferred — GDScript's `int` is always 64-bit, no fixed/pointer-width distinction exists to have a bug in.

**Real, disclosed gap**: same as the Unreal binding — no suppression mechanism exists here either, no inline comment, no config-level allowlist.

### `drift-godot::unseeded_rng`

Flags Godot's global RNG functions (`randi`, `randf`, `randi_range`, `randf_range`, `randfn`, `randomize`) — auto-seeded from OS entropy unless a project explicitly tracks a seed via `seed(value)`. `randomize()` itself is included: it explicitly reseeds *from* OS entropy, the opposite of a tracked seed, so calling it is exactly as real a hazard as reading the RNG directly. Same rationale as `drift::unseeded_rng`/`DRIFT0002`/`drift-unreal::unseeded_rng`. **Fires unconditionally**, no reachability scoping needed.

Matched as a **bare** `NameRef` callee, not any call whose method name matches — deliberately excludes a member-call shape like `generator.randi()`, which is what a project's own deterministic RNG wrapper looks like at the call site. This isn't a hypothetical distinction: the exact addon named in the plan's own demand check (`§1`) ships a `NetworkRandomNumberGenerator` class that wraps a seedable `RandomNumberGenerator` instance precisely to solve this hazard — its own internal `generator.randi()` call must not be flagged, and isn't, because it's a member call, not a bare one.

**Real bug found building this**: `gdck-syntax`'s `NameRef`/`AttributeExpr` nodes carry their own leading whitespace as a child token (confirmed with a real tree dump, not assumed) — `.text()` on the callee node returned `" randi_range"`, not `"randi_range"`, so every call silently failed to match until `.trim()` was added. Caught before trusting a "0 findings" result, the same discipline `drift-unreal-lint`'s own near-miss UI tests established.

**Dogfood result, real**: run against `SlayHorizon/godot-tiny-mmo` (the same real, 470-file Godot 4 game the feasibility spike used) — 20 real findings, including the exact 6 call sites the spike itself had already found by hand (`gateway.gd`, `local_player.gd`, `dungeon_service.gd`, `weapon.gd`), plus more once the full `RNG_FUNCS` list (the spike's own throwaway visitor only checked `randi`/`randf`/`randi_range`/`randf_range`) was applied for real.

**Second dogfood target, a different genre entirely**: run against [`Orama-Interactive/Pixelorama`](https://github.com/Orama-Interactive/Pixelorama) (a real, shipped pixel-art editor, 250 `.gd` files, ~59.5k lines, Godot 4.7) — 16 real findings, spot-checked: `BaseDraw.gd:218` (`randi() % _brush.random.size()`, picking a random brush variant) and `VanishingPoint.gd:7` (`Color(randf(), randf(), randf(), 1)`, a randomized debug-handle color) are both genuine unseeded global-RNG reads, correctly located down to the column for multiple calls on the same line.

### `drift-godot::wallclock_read`

Flags `OS.get_ticks_msec`, `OS.get_ticks_usec`, `Time.get_ticks_msec`, `Time.get_ticks_usec`, `Time.get_unix_time_from_system` — a value that differs per peer/run must never feed simulated state. Same rationale as `drift::wallclock_read`/`DRIFT0003`/`drift-unreal::wallclock_read`. Matched as a `Type.method`-shaped `AttributeExpr` callee, same shape as Unreal's own `FPlatformTime::Seconds`-style matching — and the same trailing-whitespace fix above applies here too. **Fires unconditionally**, no reachability scoping needed.

**Known limitation**: same as the other unconditional rules in this tool — a locally-defined method that happens to share a name with a flagged one (e.g. a project's own `get_ticks_msec()`) is distinguished correctly (matched by the full `Type.method` spelling, not the bare method name), but a project that assigns `Time` or `OS` to a local alias (`var T := Time`) and calls `T.get_ticks_msec()` would not be caught — a real, syntactic-matching limitation inherited from the same design every other spelling-based rule in this project already carries.

**Dogfood result, real**: run against `SlayHorizon/godot-tiny-mmo` — 151 real findings across client, server, and shared code (`Time.get_ticks_msec`/`get_ticks_usec` for perf/sync timing, `Time.get_unix_time_from_system` for chat/mail/leaderboard timestamps), a real superset of the spike's own 41 hand-found hits once `get_ticks_usec` was added to the real tool's list.

**Second dogfood target**: `Orama-Interactive/Pixelorama` — 5 real findings: `Time.get_unix_time_from_system()` (autosave/crash-recovery timestamps in `Global.gd`/`OpenSave.gd`, a debounce check in `GradientEdit.gd`) and one `Time.get_ticks_msec()` (`Main.gd:244`). Correctly distinguishes real `Time.*` calls from the codebase's own many unrelated `_delta`/timer-parameter uses of the word "time" — no false positives on those.

### `drift-godot::unordered_parallelism`

Flags `WorkerThreadPool.add_task`/`WorkerThreadPool.add_group_task` — Godot's own thread-pool dispatch, whose completion order isn't guaranteed. Same known-problem caveat as `drift::unordered_parallelism`/`DRIFT0004`/`drift-unreal::unordered_parallelism`, inherited rather than re-litigated. Matched as a `Type.method` `AttributeExpr` callee, same shape as `wallclock_read`. **Fires unconditionally**, no reachability scoping needed.

**Real, disclosed gap**: both `SlayHorizon/godot-tiny-mmo` and `Orama-Interactive/Pixelorama` have zero confirmed call sites for either function — not evidence the rule is unneeded (`WorkerThreadPool` is a standard, documented Godot 4 parallelism primitive), just evidence neither real corpus checked so far uses it, same situation `drift-unreal-lint`'s own `unordered_parallelism` was in against Lyra. Validated instead with a real positive/negative fixture: a bare `WorkerThreadPool.add_task(...)` call fires; an unrelated class's own same-named `add_task` method doesn't.

### `drift-godot::float_outside_fixed_step`

Flags non-associative float arithmetic (`+`, `-`, `*`, `/`) reachable from `_process` or `_physics_process` — Godot's own fixed, well-known per-frame/per-physics-step entry points. Same rationale as the Rust/C#/Unreal rule of the same name, but with a real Godot-specific simplification: unlike the Rust/Unreal sides' own `tick_reachable_roots` config, **no config file is needed here at all** — every real Godot project hooks into the engine's own tick by overriding these exact method names, so reachability roots are auto-detected by name.

**Real, necessary limitation, disclosed rather than hidden**: GDScript is dynamically typed by default, and `gdck-syntax` is a syntax-only parser with no type checker — unlike the Rust/C#/Unreal sides, which all have a real compiler to ask "is this expression a float?", this tool has no way to know in general. v1's proxy is narrow and syntactic: an operand counts as float-ish only if it's (1) a `Float` literal (`9.8`), or (2) a bare name referring to a parameter or local variable *in that same function* explicitly typed `: float`. Pure untyped-variable arithmetic (`var a = 1; var b = 2; return a + b`, neither side typed or literal) is invisible to this rule — a real gap, not a silently-accepted one. Typed GDScript (Godot's own recommended style for anything simulation-relevant) is exactly what this catches.

**A second, separate real limitation**: the reachability call graph is flat and name-only (no symbol table to resolve `self.foo()`/`obj.foo()` to a specific class) — two unrelated methods sharing a name collide into one call-graph node. This widens reachability (a real false-positive risk) rather than silently dropping a real edge, the opposite trade-off from a missed edge. **Real bug found dogfooding, fixed**: this collision also caused the identical finding to be printed twice when two colliding names both resolved to the same underlying function (`toaster.gd:154:22`, confirmed in real output) — fixed with a dedup pass on `(file, line, column, rule)` before printing, same fix shape `drift-unreal-lint` needed for its own real `UE_LOG` macro-duplication bug.

**Dogfood result, real**: run against `SlayHorizon/godot-tiny-mmo` — 861 of 1,999 total functions resolved reachable from `_process`/`_physics_process`, 84 real findings. Spot-checked, not just counted: `local_player.gd:382` (`velocity = input_direction * move_speed`, where `move_speed` is a local explicitly typed `: float`) fires — a real confirmation the local-type-tracking design catches real, non-synthetic code, not just the synthetic fixture. Ran in well under a second even with the whole-project call-graph construction (1,999 functions, no libclang-style parse cost here).

**Second dogfood target, and a real miss it surfaced, now fixed**: `Orama-Interactive/Pixelorama` — first run found 0 findings, but investigating *why* (rather than trusting a clean run) found a real false negative: `Selection.gd`'s `_process(delta: float)` does `_marching_ants_time_elapsed += delta`, a genuine float accumulation reachable from a real tick function, invisible to this rule because `+=` is a distinct AST shape (`AssignStmt`, sharing a node kind with plain `x = y` and every other compound-assignment operator) from the plain `BinaryExpr` this tool walked. Fixed by also matching `AssignStmt` against a `COMPOUND_ARITHMETIC_OPS` token set (`PlusEq`, `MinusEq`, `StarEq`, `SlashEq` — deliberately excluding `StarStarEq`/`**=` and every non-arithmetic compound op, same scope as `ARITHMETIC_OPS`). Re-running against the same file after the fix confirms it now fires at `Selection.gd:41:2`. Confirmed the same gap existed in the Rust and Unreal implementations too — see `drift::float_outside_fixed_step`'s own entry for the cross-binding note.

**A second, more serious real bug the same investigation surfaced, now fixed**: at full-project scale (250 files), the fix above still produced 0 findings project-wide despite firing correctly against `Selection.gd` in isolation. Root cause: `collect_functions_and_edges`'s whole-project function table keyed by bare name in a single `HashMap<String, FuncInfo>` — with `_process`/`_physics_process` being about as common a name as exists in a real multi-scene Godot project, every file's own definition overwrote the previous one (`.insert()`), so only the *last* one (in sorted file order) ever got scanned as a root; every other file's `_process`/`_physics_process` body was silently skipped entirely, not just its `+=` accumulations. `edges` (the call graph itself) was never affected — it already unioned callees per name via `.entry().or_default().extend()`, not overwrite — only the root/target function table was. This was a materially worse instance of the already-disclosed "flat, name-only call graph" limitation below: that entry describes the risk only for **callees** (widens reachability, a false-positive risk), but for the **roots themselves** (the one case a real project is guaranteed to have many same-named instances of), the bug was a silent false-negative gap across nearly the whole project. Confirmed via `DRIFT_GODOT_STATS=1`: only 3 functions reported reachable from `_process`/`_physics_process` across all 250 files pre-fix, and a same-file single-target run found the hit a whole-project run missed. **Fixed** by changing `funcs` to `HashMap<String, Vec<FuncInfo>>` — every function sharing a name is now scanned, not just one; a re-run after the fix finds `Selection.gd:41:2` project-wide (22 findings total, up from 21). Locked in with a real regression fixture (`tests/fixture/collision/{a,b}.gd`, two files each defining their own `_physics_process`) proving same-named functions across files no longer collide.

**A third real bug found verifying the two fixes above, now fixed**: writing this catalog's own walkthrough, a two-statement reproduction (a plain float chain followed by a `+=` accumulation, both directly in one `_physics_process` body — not delegated to separate functions the way this catalog's own fixture originally did) surfaced a real location bug: the compound-assignment finding was reported on the *wrong line* — the line of whichever statement preceded it, not its own. Root cause: `gdck-syntax`'s checkpoint-based `AssignStmt` construction (it opens `ExprStmt`, parses the lhs, then — only once it sees `=`/`+=`/etc. — retroactively reopens that span as `AssignStmt` at a checkpoint taken *before* the lhs was parsed). For any statement after the first one in a block, the `Newline`/`Whitespace` separating it from the previous statement is lexed as the *next* token's own leading trivia (the same "`NameRef`/`AttributeExpr` carry their own leading whitespace" quirk already documented above), and the checkpoint predates that trivia too — so a retroactively-built `AssignStmt`'s own `.range()` silently swallows the previous statement's trailing newline. Confirmed directly against a real tree dump, not assumed. This only manifested for compound assignments specifically: a plain `=` assignment's own `AssignStmt` never qualifies as a finding site in the first place (`Eq` isn't in `COMPOUND_ARITHMETIC_OPS`), so the code always reported the location of its *inner* `BinaryExpr` instead, whose own range was never affected — only a compound assignment's finding, reported at the `AssignStmt`'s own range, hit this. **Fixed** with a `first_real_token_start` helper that walks the leftmost path to the first non-trivia token and reports that instead of the node's raw range start — applied to every qualifying node, not just `AssignStmt`, which also corrected a smaller pre-existing one-column-off inaccuracy on ordinary `BinaryExpr` findings (they were pointing at the leading-whitespace character before the first identifier, not the identifier itself). Locked in with a new fixture function (`combined_in_one_function`, `tests/fixture/player.gd`) reproducing the exact two-statement shape that surfaced this.

### Fixture

`tests/fixture/player.gd` — synthetic (small, self-contained): a bare `randi_range()` call fires, a member call on a project-owned `RandomNumberGenerator` instance doesn't; a bare `Time.get_ticks_msec()` call fires, a locally-defined method of the same name (and a call to it) doesn't; a bare `WorkerThreadPool.add_task(...)` call fires, an unrelated class's own same-named method doesn't; a `_physics_process` reachable float chain (`velocity_y + gravity * delta`, deduped to one warning) fires, a same-shaped but never-called function doesn't, and pure untyped-variable arithmetic reachable from `_physics_process` doesn't either; a compound-assignment accumulation (`elapsed += delta`) fires as its own hit; and (`combined_in_one_function`) a plain float chain followed by a compound assignment *in the same function* both fire, each at their own correct line — the exact shape that surfaced the location bug above. `tests/fixture/collision/{a,b}.gd` — two files each defining their own `_physics_process` with distinct float arithmetic, both must fire, proving same-named functions across files don't collide. Two `cargo test` integration tests, exact-diff assertions on all cases.
