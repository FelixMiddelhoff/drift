using System.Collections.Immutable;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.CSharp.Syntax;
using Microsoft.CodeAnalysis.Diagnostics;

namespace Drift.Analyzers;

// Mirrors drift-lint's drift::unseeded_rng (crates/drift-lint/src/unseeded_rng.rs):
// `new System.Random()` seeds from Environment.TickCount (OS/process-clock
// derived), and `UnityEngine.Random` is Unity's global generator, seeded from
// entropy unless InitState is called explicitly. Either desyncs a lockstep
// simulation that reads from it.
[DiagnosticAnalyzer(LanguageNames.CSharp)]
public sealed class UnseededRandomAnalyzer : DiagnosticAnalyzer
{
    public const string DiagnosticId = "DRIFT0002";

    private static readonly DiagnosticDescriptor Rule = new(
        DiagnosticId,
        title: "RNG seeded from OS/engine entropy inside code reachable from simulation state",
        messageFormat: "{0} is not seeded deterministically",
        category: "Determinism",
        defaultSeverity: DiagnosticSeverity.Warning,
        isEnabledByDefault: true,
        description: "Use an explicit, tracked seed (e.g. new Random(tickSeed) or UnityEngine.Random.InitState) fed by your simulation's deterministic seed.");

    public override ImmutableArray<DiagnosticDescriptor> SupportedDiagnostics => ImmutableArray.Create(Rule);

    public override void Initialize(AnalysisContext context)
    {
        context.ConfigureGeneratedCodeAnalysis(GeneratedCodeAnalysisFlags.None);
        context.EnableConcurrentExecution();
        context.RegisterSyntaxNodeAction(AnalyzeObjectCreation, SyntaxKind.ObjectCreationExpression);
        context.RegisterSyntaxNodeAction(AnalyzeMemberAccess, SyntaxKind.SimpleMemberAccessExpression);
    }

    private static void AnalyzeObjectCreation(SyntaxNodeAnalysisContext context)
    {
        var creation = (ObjectCreationExpressionSyntax)context.Node;
        // Only the parameterless `new Random()` is entropy-seeded — `new
        // Random(seed)` is exactly the fix this rule asks for, so it must
        // not be flagged.
        if (creation.ArgumentList is { Arguments.Count: > 0 })
        {
            return;
        }

        var typeInfo = context.SemanticModel.GetTypeInfo(creation, context.CancellationToken);
        if (typeInfo.Type is { } type && type.ContainingNamespace?.ToDisplayString() == "System" && type.Name == "Random")
        {
            context.ReportDiagnostic(Diagnostic.Create(Rule, creation.GetLocation(), "new Random()"));
        }
    }

    private static void AnalyzeMemberAccess(SyntaxNodeAnalysisContext context)
    {
        var access = (MemberAccessExpressionSyntax)context.Node;
        var symbolInfo = context.SemanticModel.GetSymbolInfo(access, context.CancellationToken);
        var containingType = symbolInfo.Symbol?.ContainingType;
        if (containingType is null)
        {
            return;
        }

        if (containingType.ContainingNamespace?.ToDisplayString() != "UnityEngine" || containingType.Name != "Random")
        {
            return;
        }

        // InitState (and the `state` property, used to save/restore a
        // stream) are the deterministic-seeding mechanism itself, not a
        // violation — flagging them would tell people not to use the fix.
        var memberName = access.Name.Identifier.Text;
        if (memberName is "InitState" or "state")
        {
            return;
        }

        context.ReportDiagnostic(Diagnostic.Create(Rule, access.GetLocation(), "UnityEngine.Random"));
    }
}
