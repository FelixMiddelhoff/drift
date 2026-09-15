using System.Threading.Tasks;
using Xunit;

namespace Drift.Analyzers.Tests;

public class WallclockReadAnalyzerTests
{
    private const string UnityTimeStub = """
        namespace UnityEngine
        {
            public static class Time
            {
                public static float realtimeSinceStartup => 0f;
                public static float deltaTime => 0f;
            }
        }
        """;

    [Fact]
    public async Task Flags_DateTime_Now()
    {
        const string source = """
            class C
            {
                void M()
                {
                    var t = System.DateTime.Now;
                }
            }
            """;

        var result = await AnalyzerTestHelper.GetDiagnosticsAsync(new WallclockReadAnalyzer(), source);

        var diagnostic = Assert.Single(result.Diagnostics);
        Assert.Equal(WallclockReadAnalyzer.DiagnosticId, diagnostic.Id);
    }

    [Fact]
    public async Task Flags_Environment_TickCount()
    {
        const string source = """
            class C
            {
                void M()
                {
                    var t = System.Environment.TickCount;
                }
            }
            """;

        var result = await AnalyzerTestHelper.GetDiagnosticsAsync(new WallclockReadAnalyzer(), source);

        Assert.Single(result.Diagnostics);
    }

    [Fact]
    public async Task Flags_UnityEngine_Time_realtimeSinceStartup()
    {
        var source = UnityTimeStub + """
            class C
            {
                void M()
                {
                    var t = UnityEngine.Time.realtimeSinceStartup;
                }
            }
            """;

        var result = await AnalyzerTestHelper.GetDiagnosticsAsync(new WallclockReadAnalyzer(), source);

        Assert.Single(result.Diagnostics);
    }

    [Fact]
    public async Task Does_not_flag_UnityEngine_Time_deltaTime()
    {
        var source = UnityTimeStub + """
            class C
            {
                void M()
                {
                    var t = UnityEngine.Time.deltaTime;
                }
            }
            """;

        var result = await AnalyzerTestHelper.GetDiagnosticsAsync(new WallclockReadAnalyzer(), source);

        Assert.Empty(result.Diagnostics);
    }

    [Fact]
    public async Task Does_not_flag_unrelated_member_access()
    {
        const string source = """
            class C
            {
                void M()
                {
                    var s = "hello";
                    var len = s.Length;
                }
            }
            """;

        var result = await AnalyzerTestHelper.GetDiagnosticsAsync(new WallclockReadAnalyzer(), source);

        Assert.Empty(result.Diagnostics);
    }
}
