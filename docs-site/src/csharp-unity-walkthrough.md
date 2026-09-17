# C# / Unity walkthrough: adding drift to a project

Full step-by-step example: taking a real C# or Unity project from zero to `GameDeterminism.Analyzers` flagging real code, using a small multiplayer-relevant class as the running example.

## 1. Prerequisites

- Any C# project (`.csproj`) targeting a framework the analyzer supports — it's a plain Roslyn analyzer, so it works identically in a Unity project or a standalone .NET project.
- .NET SDK (for building/testing) — Unity users don't need this separately, Unity ships its own Roslyn pipeline.

## 2. Install

```bash
dotnet add package GameDeterminism.Analyzers
```

Published as `GameDeterminism.Analyzers`, not `Drift.Analyzers` — see the [C# quickstart](./quickstart-csharp.md) for why. In Unity, add the same `<PackageReference>` to your `.csproj` (via NuGetForUnity, or manually if you manage `.csproj` files directly), or reference the built DLL:

```xml
<ItemGroup>
  <Analyzer Include="path/to/Drift.Analyzers.dll" />
</ItemGroup>
```

No project restart needed — Roslyn analyzers run inline as you build (and in most IDEs, as you type).

## 3. First build: every rule fires unconditionally

Unlike the Rust/Unreal sides, none of the 5 C# rules need reachability configuration — there's no opt-in step here at all, and no `float_outside_fixed_step` equivalent (never ported to C#/Unity). Build your project and real findings show up as ordinary build warnings:

```csharp
Dictionary<int, PlayerState> players = new();
foreach (var kvp in players) // DRIFT0001
{
    ApplyState(kvp.Value);
}
```

```
DRIFT0001: Iterating a Dictionary<int, PlayerState> — order is not guaranteed stable across peers
```

## 4. Fixing a real finding

Two peers running this same loop can process `players` in a different order — if `ApplyState` has any order-dependent side effect (accumulating into shared state, picking a "first" match), that's a desync waiting to happen.

```csharp
// Before — flagged
foreach (var kvp in players) { ApplyState(kvp.Value); }

// After — deterministic order
foreach (var kvp in players.OrderBy(p => p.Key)) { ApplyState(kvp.Value); }
```

Sorting by key before iterating is recognized as the fix — same rationale as the Rust/Unreal/Godot sides' own `hashmap_iter`.

## 5. The other four rules, with real examples

```csharp
var roll = UnityEngine.Random.Range(1, 7); // DRIFT0002: OS/engine-entropy-seeded RNG
var now = DateTime.Now;                    // DRIFT0003: wall-clock read
Parallel.ForEach(units, u => u.Tick());    // DRIFT0004: parallel iteration, result order not guaranteed
```

```csharp
public record PlayerId(nint Handle); // DRIFT0005: nint is pointer-width — 32 vs 64 bits
```

Fixes, in order: `UnityEngine.Random.InitState(seed)` fed by a deterministic seed (or a tracked `System.Random(seed)` outside Unity); replace the wall-clock read with your simulation's own tick counter; confirm `Parallel.ForEach`'s reduction is actually order-independent, or don't parallelize that step; replace `nint`/`nuint` in anything hashed with a fixed-width `int`/`long`.

## 6. Suppressing a specific finding

```csharp
#pragma warning disable DRIFT0001
foreach (var kvp in players) { /* verified safe, e.g. read-only debug logging */ }
#pragma warning restore DRIFT0001
```

## 7. Wiring into CI

Nothing special needed — analyzer warnings surface on any normal `dotnet build`/`msbuild`. To make them **fail** the build (rather than just show as warnings) in CI, promote them in your `.csproj` or `Directory.Build.props`:

```xml
<PropertyGroup>
  <WarningsAsErrors>DRIFT0001;DRIFT0002;DRIFT0003;DRIFT0004;DRIFT0005</WarningsAsErrors>
</PropertyGroup>
```

```yaml
jobs:
  drift-analyzers:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-dotnet@v4
      - run: dotnet build YourProject.csproj
```

## Known limitations

See the [rule catalog](./rule-catalog.md#cunity-driftanalyzers) for the full per-rule breakdown. The short version: `DRIFT0001` doesn't extend to the generic `IDictionary<TKey,TValue>` interface (deliberate — `SortedDictionary<TKey,TValue>` also implements it, so flagging the interface would be a new false positive on genuinely ordered code), and `DRIFT0005`'s hand-written-`GetHashCode` check is syntactic reference detection, not real data-flow — a field merely read inside `GetHashCode` but not actually folded into the hash still gets flagged.
