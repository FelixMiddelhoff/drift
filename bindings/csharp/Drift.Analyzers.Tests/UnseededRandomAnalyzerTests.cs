using System.Threading.Tasks;
using Xunit;

namespace Drift.Analyzers.Tests;

// UnityEngine.Random is stubbed in the same compilation (see
// AnalyzerTestHelper's docs) — no real UnityEngine.dll available here, and
// the analyzer resolves it purely by namespace/type name, so a same-shape
// stand-in type resolves identically for symbol lookup purposes.
public class UnseededRandomAnalyzerTests
{
    private const string UnityRandomStub = """
        namespace UnityEngine
        {
            public static class Random
            {
                public static float value => 0f;
                public static void InitState(int seed) { }
                public static System.Random state => null!;
            }
        }
        """;

    [Fact]
    public async Task Flags_parameterless_new_Random()
    {
        const string source = """
            class C
            {
                void M()
                {
                    var r = new System.Random();
                }
            }
            """;

        var result = await AnalyzerTestHelper.GetDiagnosticsAsync(new UnseededRandomAnalyzer(), source);

        var diagnostic = Assert.Single(result.Diagnostics);
        Assert.Equal(UnseededRandomAnalyzer.DiagnosticId, diagnostic.Id);
    }

    [Fact]
    public async Task Does_not_flag_seeded_new_Random()
    {
        const string source = """
            class C
            {
                void M()
                {
                    var r = new System.Random(42);
                }
            }
            """;

        var result = await AnalyzerTestHelper.GetDiagnosticsAsync(new UnseededRandomAnalyzer(), source);

        Assert.Empty(result.Diagnostics);
    }

    [Fact]
    public async Task Flags_UnityEngine_Random_value()
    {
        var source = UnityRandomStub + """
            class C
            {
                void M()
                {
                    var x = UnityEngine.Random.value;
                }
            }
            """;

        var result = await AnalyzerTestHelper.GetDiagnosticsAsync(new UnseededRandomAnalyzer(), source);

        Assert.Single(result.Diagnostics);
    }

    [Fact]
    public async Task Does_not_flag_UnityEngine_Random_InitState()
    {
        var source = UnityRandomStub + """
            class C
            {
                void M()
                {
                    UnityEngine.Random.InitState(42);
                }
            }
            """;

        var result = await AnalyzerTestHelper.GetDiagnosticsAsync(new UnseededRandomAnalyzer(), source);

        Assert.Empty(result.Diagnostics);
    }
}
