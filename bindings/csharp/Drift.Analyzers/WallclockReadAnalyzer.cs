using System.Collections.Immutable;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.CSharp.Syntax;
using Microsoft.CodeAnalysis.Diagnostics;

namespace Drift.Analyzers;

// Mirrors drift-lint's drift::wallclock_read (crates/drift-lint/src/wallclock_read.rs):
// DateTime.Now/UtcNow and Environment.TickCount read OS wall-clock/uptime
// state that two peers see differently by construction.
[DiagnosticAnalyzer(LanguageNames.CSharp)]
public sealed class WallclockReadAnalyzer : DiagnosticAnalyzer
{
    public const string DiagnosticId = "DRIFT0003";

    private static readonly DiagnosticDescriptor Rule = new(
        DiagnosticId,
        title: "Wall-clock read is not guaranteed the same across peers",
        messageFormat: "{0} is not guaranteed the same across peers",
        category: "Determinism",
        defaultSeverity: DiagnosticSeverity.Warning,
        isEnabledByDefault: true,
        description: "Use your simulation's own deterministic tick counter if this feeds simulated state.");

    public override ImmutableArray<DiagnosticDescriptor> SupportedDiagnostics => ImmutableArray.Create(Rule);

    public override void Initialize(AnalysisContext context)
    {
        context.ConfigureGeneratedCodeAnalysis(GeneratedCodeAnalysisFlags.None);
        context.EnableConcurrentExecution();
        context.RegisterSyntaxNodeAction(AnalyzeMemberAccess, SyntaxKind.SimpleMemberAccessExpression);
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

        var ns = containingType.ContainingNamespace?.ToDisplayString();
        var memberName = access.Name.Identifier.Text;

        var isDateTimeNow = ns == "System" && containingType.Name == "DateTime" && memberName is "Now" or "UtcNow";
        var isTickCount = ns == "System" && containingType.Name == "Environment" && memberName == "TickCount";
        var isUnityRealtime = ns == "UnityEngine" && containingType.Name == "Time" && memberName == "realtimeSinceStartup";

        if (isDateTimeNow || isTickCount || isUnityRealtime)
        {
            context.ReportDiagnostic(Diagnostic.Create(Rule, access.GetLocation(), $"{containingType.Name}.{memberName}"));
        }
    }
}
