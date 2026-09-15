using System.Threading.Tasks;
using Xunit;

namespace Drift.Analyzers.Tests;

public class UnorderedParallelismAnalyzerTests
{
    [Fact]
    public async Task Flags_AsParallel()
    {
        const string source = """
            using System.Collections.Generic;
            using System.Linq;
            class C
            {
                void M()
                {
                    var units = new List<int>();
                    var q = units.AsParallel().Select(u => u * 2);
                }
            }
            """;

        var result = await AnalyzerTestHelper.GetDiagnosticsAsync(new UnorderedParallelismAnalyzer(), source);

        var diagnostic = Assert.Single(result.Diagnostics);
        Assert.Equal(UnorderedParallelismAnalyzer.DiagnosticId, diagnostic.Id);
    }

    [Fact]
    public async Task Flags_Parallel_ForEach()
    {
        const string source = """
            using System.Collections.Generic;
            using System.Threading.Tasks;
            class C
            {
                void M()
                {
                    var units = new List<int>();
                    Parallel.ForEach(units, u => { });
                }
            }
            """;

        var result = await AnalyzerTestHelper.GetDiagnosticsAsync(new UnorderedParallelismAnalyzer(), source);

        Assert.Single(result.Diagnostics);
    }

    [Fact]
    public async Task Does_not_flag_plain_Select()
    {
        const string source = """
            using System.Collections.Generic;
            using System.Linq;
            class C
            {
                void M()
                {
                    var units = new List<int>();
                    var q = units.Select(u => u * 2);
                }
            }
            """;

        var result = await AnalyzerTestHelper.GetDiagnosticsAsync(new UnorderedParallelismAnalyzer(), source);

        Assert.Empty(result.Diagnostics);
    }
}
