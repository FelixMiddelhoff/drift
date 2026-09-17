use crate::reachability::{Config, Reachable, load_config, resolve_named_functions};
use clippy_utils::diagnostics::span_lint_and_help;
use rustc_hir::def_id::LocalDefId;
use rustc_hir::{BinOpKind, Expr, ExprKind, Node};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::ty::TyKind;
use rustc_session::{declare_lint, impl_lint_pass};
use std::collections::HashSet;

declare_lint! {
    /// ### What it does
    /// Flags non-associative float arithmetic (`+`, `-`, `*`, `/`, `%`)
    /// in code reachable from a configured `tick_reachable_roots` entry,
    /// unless the enclosing function is listed in `fixed_step_functions`.
    ///
    /// ### Why is this bad?
    /// `(a + b) + c` isn't guaranteed to equal `a + (b + c)` in
    /// floating-point — operation order, SIMD width, and compiler
    /// optimization level can all change the result by an ULP or more.
    /// Outside a controlled fixed-timestep/fixed-point discipline, that's
    /// enough to desync two peers whose builds compiled the same source
    /// slightly differently.
    ///
    /// ### Known problems
    /// **Opt-in only** — unlike this crate's other rules, this one does
    /// **nothing** unless `dylint.toml` configures `tick_reachable_roots`
    /// (see `crate::reachability`). Deferred from this project's original
    /// v1 catalog for exactly this reason: without reachability scoping,
    /// this rule would flag nearly every float operation in a typical
    /// game codebase — worse than not having it. Even scoped, it doesn't
    /// distinguish arithmetic that's genuinely fixed-step-safe (integer-
    /// like float usage, a value that never varies build-to-build) from
    /// arithmetic that really is fragile — `fixed_step_functions` is a
    /// blunt, whole-function exemption, not per-expression judgment.
    ///
    /// ### Example
    /// ```toml
    /// # dylint.toml
    /// [drift-lint]
    /// tick_reachable_roots = ["tick"]
    /// fixed_step_functions = ["physics::integrate"]
    /// ```
    /// ```rust,ignore
    /// fn tick() {
    ///     let x = a + b + c; // flagged unless tick() is fixed-step-exempt
    /// }
    /// ```
    pub DRIFT_FLOAT_OUTSIDE_FIXED_STEP,
    Warn,
    "non-associative float arithmetic in code reachable from simulation state, outside a fixed-step-exempt function"
}

pub struct FloatOutsideFixedStep {
    config: Config,
    reachable: Reachable,
    exempt: HashSet<LocalDefId>,
}

impl FloatOutsideFixedStep {
    pub fn new() -> Self {
        Self {
            config: load_config(),
            reachable: Reachable::default(),
            exempt: HashSet::new(),
        }
    }
}

impl_lint_pass!(FloatOutsideFixedStep => [DRIFT_FLOAT_OUTSIDE_FIXED_STEP]);

const FLAGGED_OPS: &[BinOpKind] = &[
    BinOpKind::Add,
    BinOpKind::Sub,
    BinOpKind::Mul,
    BinOpKind::Div,
    BinOpKind::Rem,
];

impl<'tcx> LateLintPass<'tcx> for FloatOutsideFixedStep {
    fn check_crate(&mut self, cx: &LateContext<'tcx>) {
        // Opt-in only (see Known problems above) — an empty config means
        // "do nothing," not "flag everywhere" like this crate's other
        // rules. Skip the (otherwise pointless) reachability computation
        // entirely when unconfigured.
        if self.config.tick_reachable_roots.is_empty() {
            return;
        }
        self.reachable = Reachable::compute(cx, &self.config);
        self.exempt = resolve_named_functions(cx, &self.config.fixed_step_functions);
    }

    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if self.config.tick_reachable_roots.is_empty() {
            return;
        }
        // A compound assignment (`total += delta`) is a distinct HIR node
        // (`AssignOp`, not `Binary`) carrying the same `BinOp` — a real gap
        // found dogfooding drift-godot-lint against a real project
        // (Orama-Interactive/Pixelorama's own `_process` accumulating a
        // timer via `+=`): the original Binary-only check silently missed
        // it here too, confirmed by grepping this file for `AssignOp`
        // before this fix (no match). `is_compound_assign` distinguishes
        // it from `Binary` below since AssignOp can't be a chain operand
        // (its type is `()`, so it's never nested inside another
        // arithmetic expr) and is always its own standalone statement.
        let (op, lhs, is_compound_assign) = match expr.kind {
            ExprKind::Binary(op, lhs, _rhs) => (op.node, lhs, false),
            // `AssignOp` carries its own `AssignOpKind` (`AddAssign`,
            // `SubAssign`, ...), not `BinOpKind` — convert via rustc's own
            // `From<AssignOpKind> for BinOpKind` so `FLAGGED_OPS` below
            // covers both node kinds with one list.
            ExprKind::AssignOp(op, lhs, _rhs) => (BinOpKind::from(op.node), lhs, true),
            _ => return,
        };
        if !FLAGGED_OPS.contains(&op) {
            return;
        }
        let lhs_ty = cx.typeck_results().expr_ty(lhs);
        if !matches!(lhs_ty.kind(), TyKind::Float(_)) {
            return;
        }
        // A chain like `a + b + c` is two nested Binary exprs over the
        // same non-associativity concern — report only the outermost,
        // not once per operator, or a chain of N terms produces N-1
        // near-duplicate warnings on the same line (confirmed for real:
        // an unguarded version of this check emitted two warnings for
        // one `a + b + c` in the fixture below before this was added).
        // Doesn't apply to AssignOp — never a chain operand, see above.
        if !is_compound_assign
            && let Node::Expr(parent) = cx.tcx.parent_hir_node(expr.hir_id)
            && let ExprKind::Binary(parent_op, parent_lhs, _) = parent.kind
            && FLAGGED_OPS.contains(&parent_op.node)
            && matches!(
                cx.typeck_results().expr_ty(parent_lhs).kind(),
                TyKind::Float(_)
            )
        {
            return;
        }
        let enclosing = cx.tcx.hir_enclosing_body_owner(expr.hir_id);
        if self.exempt.contains(&enclosing) {
            return;
        }
        if !self.reachable.includes(enclosing) {
            return;
        }
        span_lint_and_help(
            cx,
            DRIFT_FLOAT_OUTSIDE_FIXED_STEP,
            expr.span,
            "non-associative float arithmetic reachable from simulation state",
            None,
            "confirm this runs under a fixed-step/fixed-point discipline, or list the enclosing function in dylint.toml's fixed_step_functions",
        );
    }
}
