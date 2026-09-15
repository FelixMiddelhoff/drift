using System.Collections.Immutable;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.CSharp.Syntax;
using Microsoft.CodeAnalysis.Diagnostics;

namespace Drift.Analyzers;

// Mirrors drift-lint's drift::unordered_parallelism, but targets .NET's own
// parallelism primitives rather than rayon: PLINQ's AsParallel() and
// System.Threading.Tasks.Parallel.ForEach/.For. Same result-order-is-
// scheduler-dependent rationale.
[DiagnosticAnalyzer(LanguageNames.CSharp)]
public sealed class UnorderedParallelismAnalyzer : DiagnosticAnalyzer
{
    public const string DiagnosticId = "DRIFT0004";

    private static readonly DiagnosticDescriptor Rule = new(
        DiagnosticId,
        title: "Parallel iteration result order is scheduler-dependent",
        messageFormat: "{0} — result order is scheduler-dependent",
        category: "Determinism",
        defaultSeverity: DiagnosticSeverity.Warning,
        isEnabledByDefault: true,
        description: "Confirm the terminal reduction is order-independent (commutative), or suppress if already confirmed.");

    public override ImmutableArray<DiagnosticDescriptor> SupportedDiagnostics => ImmutableArray.Create(Rule);

    public override void Initialize(AnalysisContext context)
    {
        context.ConfigureGeneratedCodeAnalysis(GeneratedCodeAnalysisFlags.None);
        context.EnableConcurrentExecution();
        context.RegisterSyntaxNodeAction(AnalyzeInvocation, SyntaxKind.InvocationExpression);
    }

    private static void AnalyzeInvocation(SyntaxNodeAnalysisContext context)
    {
        var invocation = (InvocationExpressionSyntax)context.Node;
        var symbolInfo = context.SemanticModel.GetSymbolInfo(invocation, context.CancellationToken);
        var method = symbolInfo.Symbol as IMethodSymbol;
        var containingType = method?.ContainingType;
        if (containingType is null)
        {
            return;
        }

        var ns = containingType.ContainingNamespace?.ToDisplayString();

        // PLINQ: `.AsParallel()` extension method on ParallelEnumerable.
        if (ns == "System.Linq" && containingType.Name == "ParallelEnumerable" && method!.Name == "AsParallel")
        {
            context.ReportDiagnostic(Diagnostic.Create(Rule, invocation.GetLocation(), "AsParallel()"));
            return;
        }

        // System.Threading.Tasks.Parallel.ForEach / .For.
        if (ns == "System.Threading.Tasks" && containingType.Name == "Parallel" && method!.Name is "ForEach" or "For")
        {
            context.ReportDiagnostic(Diagnostic.Create(Rule, invocation.GetLocation(), $"Parallel.{method.Name}"));
        }
    }
}
