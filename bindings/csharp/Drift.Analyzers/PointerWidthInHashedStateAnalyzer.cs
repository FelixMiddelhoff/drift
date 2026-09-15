using System.Collections.Immutable;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.CSharp.Syntax;
using Microsoft.CodeAnalysis.Diagnostics;

namespace Drift.Analyzers;

// Mirrors drift-lint's drift::usize_in_hashed_state, but for C#'s
// closest equivalent to Rust's `#[derive(Hash)]`: a `record`/`record struct`,
// whose compiler-generated GetHashCode/Equals are derived from every field
// exactly the way `#[derive(Hash)]` is. `nint`/`nuint` (and their older
// IntPtr/UIntPtr spellings) are the pointer-width types here.
[DiagnosticAnalyzer(LanguageNames.CSharp)]
public sealed class PointerWidthInHashedStateAnalyzer : DiagnosticAnalyzer
{
    public const string DiagnosticId = "DRIFT0005";

    private static readonly DiagnosticDescriptor Rule = new(
        DiagnosticId,
        title: "nint/nuint member on a record — width varies across platforms",
        messageFormat: "'{0}' is nint/nuint (or IntPtr/UIntPtr) on a record — width varies across platforms",
        category: "Determinism",
        defaultSeverity: DiagnosticSeverity.Warning,
        isEnabledByDefault: true,
        description: "Use a fixed-width integer type (int/long/uint/ulong) instead.");

    public override ImmutableArray<DiagnosticDescriptor> SupportedDiagnostics => ImmutableArray.Create(Rule);

    public override void Initialize(AnalysisContext context)
    {
        context.ConfigureGeneratedCodeAnalysis(GeneratedCodeAnalysisFlags.None);
        context.EnableConcurrentExecution();
        context.RegisterSyntaxNodeAction(AnalyzeRecordDeclaration, SyntaxKind.RecordDeclaration, SyntaxKind.RecordStructDeclaration);
    }

    private static void AnalyzeRecordDeclaration(SyntaxNodeAnalysisContext context)
    {
        var record = (RecordDeclarationSyntax)context.Node;

        // Positional-record parameters (`record Unit(nint Id)`).
        if (record.ParameterList is { } parameterList)
        {
            foreach (var parameter in parameterList.Parameters)
            {
                CheckTypeSyntax(context, parameter.Type, parameter.Identifier.Text);
            }
        }

        // Regular field/property members (`record Unit { public nint Id; }`).
        foreach (var member in record.Members)
        {
            switch (member)
            {
                case FieldDeclarationSyntax field:
                    foreach (var variable in field.Declaration.Variables)
                    {
                        CheckTypeSyntax(context, field.Declaration.Type, variable.Identifier.Text);
                    }
                    break;
                case PropertyDeclarationSyntax property:
                    CheckTypeSyntax(context, property.Type, property.Identifier.Text);
                    break;
            }
        }
    }

    private static void CheckTypeSyntax(SyntaxNodeAnalysisContext context, TypeSyntax? typeSyntax, string memberName)
    {
        if (typeSyntax is null)
        {
            return;
        }

        var type = context.SemanticModel.GetTypeInfo(typeSyntax, context.CancellationToken).Type;
        if (type is null)
        {
            return;
        }

        var isPointerWidth = type.SpecialType is SpecialType.System_IntPtr or SpecialType.System_UIntPtr;
        if (!isPointerWidth)
        {
            return;
        }

        context.ReportDiagnostic(Diagnostic.Create(Rule, typeSyntax.GetLocation(), memberName));
    }
}
