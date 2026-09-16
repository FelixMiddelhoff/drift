# C# / Unity quickstart

Reference the built `Drift.Analyzers.dll` as an `Analyzer` item in your `.csproj`:

```xml
<ItemGroup>
  <Analyzer Include="path/to/Drift.Analyzers.dll" />
</ItemGroup>
```

A NuGet package isn't published yet — `dotnet pack bindings/csharp/Drift.Analyzers` builds one locally in the correct `analyzers/dotnet/cs/` layout that VS/Rider/VS Code auto-load on install.

5 rules (DRIFT0001–0005) — works in any C# project; Unity-specific rules are scoped to `UnityEngine.*` types. `float_outside_fixed_step` was never ported to C#/Unity — see the [rule catalog](./rule-catalog.md) for the full per-rule breakdown.
