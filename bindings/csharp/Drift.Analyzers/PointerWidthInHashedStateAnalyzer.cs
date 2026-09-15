using System.Collections.Immutable;
using System.Linq;
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
        title: "nint/nuint member used in hashing — width varies across platforms",
        messageFormat: "'{0}' is nint/nuint (or IntPtr/UIntPtr) and used in hashing — width varies across platforms",
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
        context.RegisterSyntaxNodeAction(AnalyzeHandWrittenGetHashCode, SyntaxKind.MethodDeclaration);
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
        if (type is null || !IsPointerWidth(type))
        {
            return;
        }

        context.ReportDiagnostic(Diagnostic.Create(Rule, typeSyntax.GetLocation(), memberName));
    }

    // Closes a real, documented gap: a plain class/struct with a
    // hand-written `GetHashCode()` override isn't a record, so the check
    // above never sees it. This doesn't attempt real data-flow analysis
    // (was a referenced field actually folded into the returned hash, or
    // just read for an unrelated reason?) — it flags any nint/nuint
    // field or property of the containing type that's *referenced
    // anywhere in the method body* of a `GetHashCode` override, which is
    // a real, syntactic, conservative signal: a hash method touching a
    // pointer-width member at all is worth a second look, even if this
    // specific check can't prove the reference feeds the return value.
    private static void AnalyzeHandWrittenGetHashCode(SyntaxNodeAnalysisContext context)
    {
        var method = (MethodDeclarationSyntax)context.Node;
        if (method.Identifier.Text != "GetHashCode" || method.ParameterList.Parameters.Count != 0)
        {
            return;
        }

        var methodSymbol = context.SemanticModel.GetDeclaredSymbol(method, context.CancellationToken);
        if (methodSymbol is not { IsOverride: true, ContainingType: { } containingType })
        {
            return;
        }

        // Records are handled by AnalyzeRecordDeclaration above (which
        // also covers a record's own explicitly-written GetHashCode,
        // since it still checks the record's declared members directly)
        // — skip here to avoid reporting the same field twice.
        if (containingType.IsRecord)
        {
            return;
        }

        // Handles both a block body (`{ ... }`) and an expression body
        // (`=> ...`) — a real miss on the first attempt here: the fixture
        // test below uses `=> Id.GetHashCode()`, which has a null `Body`
        // and only populates `ExpressionBody`, so an earlier version of
        // this check (body-only) silently found nothing and the "flags"
        // test failed for real before this was added.
        SyntaxNode? searchRoot = method.Body is { } body ? body : method.ExpressionBody?.Expression;
        if (searchRoot is null)
        {
            return;
        }

        foreach (var identifier in searchRoot.DescendantNodesAndSelf().OfType<IdentifierNameSyntax>())
        {
            var symbol = context.SemanticModel.GetSymbolInfo(identifier, context.CancellationToken).Symbol;
            var (memberType, memberName) = symbol switch
            {
                IFieldSymbol field when SymbolEqualityComparer.Default.Equals(field.ContainingType, containingType) => (field.Type, field.Name),
                IPropertySymbol property when SymbolEqualityComparer.Default.Equals(property.ContainingType, containingType) => (property.Type, property.Name),
                _ => (null, null),
            };
            if (memberType is null || !IsPointerWidth(memberType))
            {
                continue;
            }
            context.ReportDiagnostic(Diagnostic.Create(Rule, identifier.GetLocation(), memberName));
        }
    }

    private static bool IsPointerWidth(ITypeSymbol type) => type.SpecialType is SpecialType.System_IntPtr or SpecialType.System_UIntPtr;
}
