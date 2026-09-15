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

### `drift::unseeded_rng`

Flags `rand::thread_rng()` and `rand::random()`.

**Why**: an RNG seeded from OS entropy differs per peer and per run by construction — any simulation decision that reads from it desyncs immediately.

Fix: use an RNG constructed from an explicit, tracked seed (e.g. `StdRng::seed_from_u64(tick_seed)`) fed by your simulation's own deterministic seed source. Suppress with `#[allow(drift_unseeded_rng)]` for genuinely cosmetic randomness (particle effects, UI flourish).

### `drift::wallclock_read`

Flags `std::time::SystemTime::now()` and `std::time::Instant::now()`.

**Why**: two peers read different wall-clock values by construction. Fine for profiling/logging, not fine for anything that affects simulated state.

Fix: use your simulation's own deterministic tick counter. Suppress with `#[allow(drift_wallclock_read)]` at legitimate profiling/logging call sites.

### `drift::unordered_parallelism`

Flags `.par_iter()`, `.par_iter_mut()`, `.into_par_iter()`, `.par_bridge()` (rayon).

**Why**: work items complete in scheduler-dependent order. A reduction into simulated state that isn't commutative differs between peers whose thread pools schedule differently — a desync that reproduces on one machine and not another.

**Known problem**: doesn't verify the chain actually ends in a non-commutative reduction — a `par_iter()` whose result is genuinely order-independent (e.g. `.sum()`) is a false positive. Suppress with `#[allow(drift_unordered_parallelism)]` once confirmed order-independent.

### `drift::usize_in_hashed_state`

Flags `usize`/`isize` fields on a struct that also derives `Hash`.

**Why**: `usize`/`isize` are pointer-width (32 vs. 64 bits). A struct hashed for a cross-peer sync check that contains one hashes differently on a 32-bit vs. 64-bit build of the same logical state — a false desync report between two otherwise-correct peers on different architectures.

Fix: use a fixed-width integer type (`u32`/`u64`/`i32`/`i64`) instead. Suppress with `#[allow(drift_usize_in_hashed_state)]` if the struct is only ever hashed for something width-insensitive (e.g. a `HashMap` key never compared across processes). Not scoped by reachability (see above) — it flags a field definition, not code inside a function.

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

## Suppressing a rule

Every rule is a plain rustc lint under the hood — suppress the normal way:

```rust,ignore
#[allow(drift_hashmap_iter)]
fn ui_only_function() { /* ... */ }
```

## C#/Unity (`Drift.Analyzers`)

A Roslyn analyzer, `bindings/csharp/Drift.Analyzers` — works in any C# project, not just Unity (Unity-specific rules are simply scoped to `UnityEngine.*` types and still just ordinary C# analysis). Install by referencing the built `Drift.Analyzers.dll` as an `Analyzer` item, or via the NuGet package once published (not yet).

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

Build: `cargo build` inside `bindings/unreal/drift-unreal-lint`, with `LIBCLANG_PATH` pointing at your LLVM install's `bin` directory (e.g. `C:\Program Files\LLVM\bin`). Run: `drift-unreal-lint <compile_commands.json> [config.toml]`.

### Reachability scoping (required — this rule is opt-in only)

Same rationale as the Rust side's `dylint.toml`: unscoped, this would blanket-flag nearly every float operation in a typical Unreal codebase. Without a config file (or an empty one), the tool does nothing and exits 0.

```toml
tick_reachable_roots = ["ALyraWeaponSpawner::Tick"]
fixed_step_functions = ["UMyIntegrator::Step"]
```

`tick_reachable_roots` seeds a reachable-function set built from a real, cross-translation-unit call graph (functions matched by `Namespace::Class::Method`-style qualified name, edges from every `CallExpr` in every parsed TU resolved by USR). Reachability stops at any function whose definition isn't in one of the TUs actually parsed — real Engine-internals calls (a TU outside the target project's own `compile_commands.json`) don't extend the graph further, a real, disclosed limitation, not a silent one.

### `drift-unreal::float_outside_fixed_step`

Flags non-associative float/double arithmetic (`+`, `-`, `*`, `/`) reachable from a `tick_reachable_roots` entry, unless the enclosing function is listed in `fixed_step_functions`. Same rationale as the Rust/C# rule of the same name. A chain like `a + b + c` is deduped to one warning on the outermost expression, not one per operator — verified with a real positive/negative fixture (`tests/fixture/pawn.cpp`): a `Tick`→`Simulate` call chain with two real float-chain statements fires exactly twice (once per statement, not once per operator), and a same-shaped `NotReached` function never called from `Tick` fires zero times.

**Known limitation, real not assumed**: `UPROPERTY`/`UFUNCTION`/`USTRUCT`/etc. specifiers (`EditAnywhere`, `Replicated`, `Category`, ...) are completely invisible to this tool — confirmed by reading `ObjectMacros.h` directly, they're `#define X(...)` (expand to nothing) under normal compilation, only read by UnrealHeaderTool separately. This rule doesn't need that metadata (it only looks at arithmetic and call graphs), so it's unaffected — but a future rule that needed to know "is this field actually replicated" would need a different approach (parsing `.generated.h` output, or a real UE-aware fork like RedpointGames') than this tool's own stock-Clang path.

**Dogfood result**: run against a real, freshly-downloaded Lyra Starter Game (UE 5.8) — all 388 translation units in its `compile_commands.json` parsed successfully (via full response-file expansion), 3087 real function/method definitions found, 554 call edges, and a real root (`ALyraWeaponSpawner::Tick`) resolved correctly. Zero findings from that specific root — real, not a bug: `Tick` there only calls `Super::Tick`, and reachability correctly stops at the Engine-internals boundary (see limitation above), so no false positive and no crash across a real, large, non-trivial codebase.
