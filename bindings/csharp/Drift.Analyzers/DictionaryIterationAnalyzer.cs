using System.Collections.Immutable;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.CSharp.Syntax;
using Microsoft.CodeAnalysis.Diagnostics;

namespace Drift.Analyzers;

// Mirrors drift-lint's drift::hashmap_iter (crates/drift-lint/src/hashmap_iter.rs):
// Dictionary<TKey, TValue>/HashSet<T> iteration order is not guaranteed stable
// across peers, platforms, or runs. See docs/rule-catalog.md for the full
// rationale and known false-positive shared with the Rust rule.
[DiagnosticAnalyzer(LanguageNames.CSharp)]
public sealed class DictionaryIterationAnalyzer : DiagnosticAnalyzer
{
    public const string DiagnosticId = "DRIFT0001";

    private static readonly DiagnosticDescriptor Rule = new(
        DiagnosticId,
        title: "Dictionary/HashSet iteration order is not guaranteed stable across peers",
        messageFormat: "Iterating a {0} — order is not guaranteed stable across peers",
        category: "Determinism",
        defaultSeverity: DiagnosticSeverity.Warning,
        isEnabledByDefault: true,
        description: "Use a SortedDictionary/SortedSet, or sort keys before iterating, if this feeds simulated state.");

    public override ImmutableArray<DiagnosticDescriptor> SupportedDiagnostics => ImmutableArray.Create(Rule);

    public override void Initialize(AnalysisContext context)
    {
        context.ConfigureGeneratedCodeAnalysis(GeneratedCodeAnalysisFlags.None);
        context.EnableConcurrentExecution();
        context.RegisterSyntaxNodeAction(AnalyzeForEach, SyntaxKind.ForEachStatement);
    }

    private static void AnalyzeForEach(SyntaxNodeAnalysisContext context)
    {
        var forEach = (ForEachStatementSyntax)context.Node;
        var typeInfo = context.SemanticModel.GetTypeInfo(forEach.Expression, context.CancellationToken);
        if (typeInfo.Type is not INamedTypeSymbol namedType)
        {
            return;
        }

        if (IsFlaggedType(namedType, out var displayName))
        {
            context.ReportDiagnostic(Diagnostic.Create(Rule, forEach.Expression.GetLocation(), displayName));
        }
    }

    private static bool IsFlaggedType(INamedTypeSymbol type, out string displayName)
    {
        // Dictionary<TKey,TValue>/HashSet<T> themselves, and their nested
        // KeyCollection/ValueCollection (Dictionary.Keys/.Values) — those
        // inherit the same unordered iteration from their containing
        // Dictionary, so `dict.Keys` is just as unordered as `dict` itself.
        var candidate = type;
        var containing = candidate.ContainingType;
        if (containing is not null && IsDictionaryOrHashSet(containing))
        {
            displayName = candidate.Name;
            return true;
        }

        if (IsDictionaryOrHashSet(candidate))
        {
            displayName = candidate.OriginalDefinition.Name;
            return true;
        }

        displayName = string.Empty;
        return false;
    }

    private static bool IsDictionaryOrHashSet(INamedTypeSymbol type)
    {
        var original = type.OriginalDefinition;
        var ns = original.ContainingNamespace?.ToDisplayString();
        if (ns != "System.Collections.Generic")
        {
            return false;
        }

        return original.Name is "Dictionary" or "HashSet";
    }
}
