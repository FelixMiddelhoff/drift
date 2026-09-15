using System.Threading.Tasks;
using Xunit;

namespace Drift.Analyzers.Tests;

public class PointerWidthInHashedStateAnalyzerTests
{
    [Fact]
    public async Task Flags_nint_positional_record_parameter()
    {
        const string source = """
            record Unit(nint Id);
            """;

        var result = await AnalyzerTestHelper.GetDiagnosticsAsync(new PointerWidthInHashedStateAnalyzer(), source);

        var diagnostic = Assert.Single(result.Diagnostics);
        Assert.Equal(PointerWidthInHashedStateAnalyzer.DiagnosticId, diagnostic.Id);
    }

    [Fact]
    public async Task Flags_nuint_record_struct_property()
    {
        const string source = """
            record struct Unit
            {
                public nuint Id { get; set; }
            }
            """;

        var result = await AnalyzerTestHelper.GetDiagnosticsAsync(new PointerWidthInHashedStateAnalyzer(), source);

        Assert.Single(result.Diagnostics);
    }

    [Fact]
    public async Task Does_not_flag_int_record_parameter()
    {
        const string source = """
            record Unit(int Id);
            """;

        var result = await AnalyzerTestHelper.GetDiagnosticsAsync(new PointerWidthInHashedStateAnalyzer(), source);

        Assert.Empty(result.Diagnostics);
    }

    [Fact]
    public async Task Does_not_flag_nint_on_plain_class()
    {
        const string source = """
            class Unit
            {
                public nint Id;
            }
            """;

        var result = await AnalyzerTestHelper.GetDiagnosticsAsync(new PointerWidthInHashedStateAnalyzer(), source);

        Assert.Empty(result.Diagnostics);
    }
}
