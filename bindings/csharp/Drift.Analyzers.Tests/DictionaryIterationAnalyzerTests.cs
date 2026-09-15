using System.Threading.Tasks;
using Xunit;

namespace Drift.Analyzers.Tests;

public class DictionaryIterationAnalyzerTests
{
    [Fact]
    public async Task Flags_foreach_over_Dictionary()
    {
        const string source = """
            using System.Collections.Generic;
            class C
            {
                void M()
                {
                    var units = new Dictionary<int, int>();
                    foreach (var kv in units)
                    {
                    }
                }
            }
            """;

        var result = await AnalyzerTestHelper.GetDiagnosticsAsync(new DictionaryIterationAnalyzer(), source);

        var diagnostic = Assert.Single(result.Diagnostics);
        Assert.Equal(DictionaryIterationAnalyzer.DiagnosticId, diagnostic.Id);
    }

    [Fact]
    public async Task Flags_foreach_over_HashSet()
    {
        const string source = """
            using System.Collections.Generic;
            class C
            {
                void M()
                {
                    var ids = new HashSet<int>();
                    foreach (var id in ids)
                    {
                    }
                }
            }
            """;

        var result = await AnalyzerTestHelper.GetDiagnosticsAsync(new DictionaryIterationAnalyzer(), source);

        Assert.Single(result.Diagnostics);
    }

    [Fact]
    public async Task Does_not_flag_foreach_over_SortedDictionary()
    {
        const string source = """
            using System.Collections.Generic;
            class C
            {
                void M()
                {
                    var units = new SortedDictionary<int, int>();
                    foreach (var kv in units)
                    {
                    }
                }
            }
            """;

        var result = await AnalyzerTestHelper.GetDiagnosticsAsync(new DictionaryIterationAnalyzer(), source);

        Assert.Empty(result.Diagnostics);
    }

    [Fact]
    public async Task Flags_foreach_over_non_generic_IDictionary()
    {
        const string source = """
            using System.Collections;
            using System.Collections.Generic;
            class C
            {
                void M(object value)
                {
                    if (value is IDictionary dict)
                    {
                        foreach (DictionaryEntry entry in dict)
                        {
                        }
                    }
                }
            }
            """;

        var result = await AnalyzerTestHelper.GetDiagnosticsAsync(new DictionaryIterationAnalyzer(), source);

        Assert.Single(result.Diagnostics);
    }

    [Fact]
    public async Task Does_not_flag_foreach_over_generic_IDictionary_interface()
    {
        // Deliberately not extended to the generic interface (see the
        // analyzer's own comment): SortedDictionary<K,V> also implements
        // IDictionary<K,V>, so flagging the interface itself would be a
        // new false positive on ordered code reached only through it.
        const string source = """
            using System.Collections.Generic;
            class C
            {
                void M(IDictionary<int, int> map)
                {
                    foreach (var kv in map)
                    {
                    }
                }
            }
            """;

        var result = await AnalyzerTestHelper.GetDiagnosticsAsync(new DictionaryIterationAnalyzer(), source);

        Assert.Empty(result.Diagnostics);
    }

    [Fact]
    public async Task Does_not_flag_foreach_over_List()
    {
        const string source = """
            using System.Collections.Generic;
            class C
            {
                void M()
                {
                    var units = new List<int>();
                    foreach (var u in units)
                    {
                    }
                }
            }
            """;

        var result = await AnalyzerTestHelper.GetDiagnosticsAsync(new DictionaryIterationAnalyzer(), source);

        Assert.Empty(result.Diagnostics);
    }
}
