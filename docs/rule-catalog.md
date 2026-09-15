# Rule catalog

One entry per lint. Each rule is `warn`-by-default, not `deny` — see [Suppressing a rule](#suppressing-a-rule).

## Rust (`drift-lint`, via [dylint](https://github.com/trailofbits/dylint))

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

**Known false positive** (found dogfooding against [Foldback](https://github.com/FelixMiddelhoff/foldback)'s `hashable.rs`): code that collects into a `Vec` and sorts it *immediately after* the flagged call is already safe, but the rule fires on the `.iter()` call itself without looking ahead — it doesn't yet follow the value to see if it gets sorted before use. Real example that currently false-positives:

```rust
# use std::collections::HashMap;
# let self_map: HashMap<u32, u32> = HashMap::new();
let mut entries: Vec<_> = self_map.iter().collect(); // flagged
entries.sort_by(|a, b| a.0.cmp(b.0));                // ...but this makes it fine
```

Suppress with `#[allow(drift::hashmap_iter)]` at sites like this until the rule learns to look ahead for a following sort.

### `drift::unseeded_rng`

Flags `rand::thread_rng()` and `rand::random()`.

**Why**: an RNG seeded from OS entropy differs per peer and per run by construction — any simulation decision that reads from it desyncs immediately.

Fix: use an RNG constructed from an explicit, tracked seed (e.g. `StdRng::seed_from_u64(tick_seed)`) fed by your simulation's own deterministic seed source. Suppress with `#[allow(drift::unseeded_rng)]` for genuinely cosmetic randomness (particle effects, UI flourish).

### `drift::wallclock_read`

Flags `std::time::SystemTime::now()` and `std::time::Instant::now()`.

**Why**: two peers read different wall-clock values by construction. Fine for profiling/logging, not fine for anything that affects simulated state.

Fix: use your simulation's own deterministic tick counter. Suppress with `#[allow(drift::wallclock_read)]` at legitimate profiling/logging call sites.

### `drift::unordered_parallelism`

Flags `.par_iter()`, `.par_iter_mut()`, `.into_par_iter()`, `.par_bridge()` (rayon).

**Why**: work items complete in scheduler-dependent order. A reduction into simulated state that isn't commutative differs between peers whose thread pools schedule differently — a desync that reproduces on one machine and not another.

**Known problem**: doesn't verify the chain actually ends in a non-commutative reduction — a `par_iter()` whose result is genuinely order-independent (e.g. `.sum()`) is a false positive. Suppress with `#[allow(drift::unordered_parallelism)]` once confirmed order-independent.

### `drift::usize_in_hashed_state`

Flags `usize`/`isize` fields on a struct that also derives `Hash`.

**Why**: `usize`/`isize` are pointer-width (32 vs. 64 bits). A struct hashed for a cross-peer sync check that contains one hashes differently on a 32-bit vs. 64-bit build of the same logical state — a false desync report between two otherwise-correct peers on different architectures.

Fix: use a fixed-width integer type (`u32`/`u64`/`i32`/`i64`) instead. Suppress with `#[allow(drift::usize_in_hashed_state)]` if the struct is only ever hashed for something width-insensitive (e.g. a `HashMap` key never compared across processes).

### Deferred: `float_outside_fixed_step`

In the original design ([drift-planning/drift-plan.md §4](../../drift-planning/drift-plan.md#4-rule-catalog-v1-rustdylint)) but not implemented — correctly detecting "float arithmetic reachable from simulation code, not itself running under a fixed-timestep discipline" needs the `#[drift::tick_reachable]` call-graph reachability tagging from §5, which this prototype doesn't have either. Shipping it without that scoping would blanket-flag nearly every float operation in a typical game codebase — worse than not having the rule. Revisit once reachability tagging exists.

## Suppressing a rule

Every rule is a plain rustc lint under the hood — suppress the normal way:

```rust,ignore
#[allow(drift::hashmap_iter)]
fn ui_only_function() { /* ... */ }
```

## C#/Unity (`Drift.Analyzers`)

A Roslyn analyzer, `bindings/csharp/Drift.Analyzers` — works in any C# project, not just Unity (Unity-specific rules are simply scoped to `UnityEngine.*` types and still just ordinary C# analysis). Install by referencing the built `Drift.Analyzers.dll` as an `Analyzer` item, or via the NuGet package once published (not yet).

### `DRIFT0001` — Dictionary/HashSet iteration order

Flags `foreach` over a `Dictionary<TKey, TValue>`/`HashSet<T>` (including their `.Keys`/`.Values` collections).

Same rationale as `drift::hashmap_iter`. Fix: use a `SortedDictionary`/`SortedSet`, or sort before iterating.

**Known limitation**: only recognizes the generic `System.Collections.Generic.Dictionary`/`HashSet` by name — doesn't yet follow the non-generic `System.Collections.IDictionary`/`ICollection` interfaces (found for real dogfooding against Foldback's Unity binding: `FoldbackReflection.cs` has an `if (value is IDictionary dict)` branch this rule doesn't see through). Worth closing in a follow-up.

### `DRIFT0002` — RNG seeded from OS/engine entropy

Flags `new System.Random()` (parameterless — seeded from `Environment.TickCount`) and any `UnityEngine.Random.*` member access except `InitState`/`state` (the deterministic-seeding mechanism itself, not a violation).

Same rationale as `drift::unseeded_rng`. Fix: `new Random(seed)`, or `UnityEngine.Random.InitState(seed)`, fed by your simulation's deterministic seed.

### `DRIFT0003` — Wall-clock read

Flags `DateTime.Now`/`DateTime.UtcNow`, `Environment.TickCount`, and `UnityEngine.Time.realtimeSinceStartup`.

Same rationale as `drift::wallclock_read`. Fix: use your simulation's own deterministic tick counter.

### Suppressing a C# rule

```csharp
#pragma warning disable DRIFT0001
// ...
#pragma warning restore DRIFT0001
```

### Dogfood result

Run against Foldback's real Unity binding (`bindings/unity/Runtime/FoldbackReflection.cs`, `bindings/unity/Tests~/FoldbackSys.Tests/Program.cs`): zero hits — neither file actually `foreach`es over a generic `Dictionary`/`HashSet` or constructs an unseeded RNG (all `HashSet`/`Dictionary` uses there are construction or non-`foreach` access). The one real gap this surfaced is the `IDictionary` limitation noted above, not a false positive.
