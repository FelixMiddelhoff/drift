# C# / Unity quickstart

```bash
dotnet add package GameDeterminism.Analyzers
```

Published as `GameDeterminism.Analyzers`, not `Drift.Analyzers` — `Drift` is a NuGet ID prefix reserved by an unrelated company, so the underlying project (`bindings/csharp/Drift.Analyzers`) and the published package name differ.

Or reference the built `Drift.Analyzers.dll` directly as an `Analyzer` item in your `.csproj`:

```xml
<ItemGroup>
  <Analyzer Include="path/to/Drift.Analyzers.dll" />
</ItemGroup>
```

`dotnet pack bindings/csharp/Drift.Analyzers` builds the package locally in the correct `analyzers/dotnet/cs/` layout that VS/Rider/VS Code auto-load on install.

5 rules (DRIFT0001–0005) — works in any C# project; Unity-specific rules are scoped to `UnityEngine.*` types. `float_outside_fixed_step` was never ported to C#/Unity — see the [rule catalog](./rule-catalog.md) for the full per-rule breakdown.
