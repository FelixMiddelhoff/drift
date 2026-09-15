using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.Diagnostics;

namespace Drift.Analyzers.Tests;

// Hand-rolled rather than pulling in Microsoft.CodeAnalysis.Testing (a much
// heavier, less commonly cached package set) — compiles the given source
// in-memory against the running runtime's core reference assemblies, plus
// a same-compilation stand-in `UnityEngine.Random` (see UnseededRandomAnalyzer
// tests) since no real UnityEngine.dll is available in this environment.
internal static class AnalyzerTestHelper
{
    public static async Task<ImmutableArrayWrapper> GetDiagnosticsAsync(DiagnosticAnalyzer analyzer, string source)
    {
        var syntaxTree = CSharpSyntaxTree.ParseText(source);
        var references = new List<MetadataReference>();
        var trustedAssemblies = ((string)AppDomain.CurrentDomain.GetData("TRUSTED_PLATFORM_ASSEMBLIES")!).Split(System.IO.Path.PathSeparator);
        foreach (var path in trustedAssemblies)
        {
            references.Add(MetadataReference.CreateFromFile(path));
        }

        var compilation = CSharpCompilation.Create(
            "TestAssembly",
            new[] { syntaxTree },
            references,
            new CSharpCompilationOptions(OutputKind.DynamicallyLinkedLibrary));

        var withAnalyzers = compilation.WithAnalyzers(System.Collections.Immutable.ImmutableArray.Create(analyzer));
        var diagnostics = await withAnalyzers.GetAnalyzerDiagnosticsAsync();
        return new ImmutableArrayWrapper(diagnostics.Where(d => d.Id.StartsWith("DRIFT")).ToList());
    }

    internal sealed class ImmutableArrayWrapper
    {
        public ImmutableArrayWrapper(List<Diagnostic> diagnostics) => Diagnostics = diagnostics;

        public List<Diagnostic> Diagnostics { get; }
    }
}
