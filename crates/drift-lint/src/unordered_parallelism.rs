use clippy_utils::diagnostics::span_lint_and_help;
use rustc_hir::{Expr, ExprKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_session::{declare_lint, declare_lint_pass};

declare_lint! {
    /// ### What it does
    /// Flags `.par_iter()`, `.par_iter_mut()`, `.into_par_iter()`, and
    /// `.par_bridge()` calls (rayon's parallel-iterator entry points).
    ///
    /// ### Why is this bad?
    /// Work items processed via rayon complete in scheduler-dependent
    /// order, not input order. A reduction into simulated state that
    /// isn't commutative (spawn order, first-writer-wins, an ordered
    /// event log) will differ between two peers whose thread pools happen
    /// to schedule differently — a desync that reproduces on one machine
    /// and not another, which is exactly the kind of bug this project
    /// exists to catch before it ships.
    ///
    /// ### Known problems
    /// Doesn't verify whether the chain actually ends in a commutative
    /// reduction (`sum`, `min`/`max` with a total order) — that requires
    /// following the whole iterator-adapter chain to its terminal
    /// operation, not implemented by this prototype. A `par_iter()` whose
    /// result is genuinely order-independent is a false positive; suppress
    /// with `#[allow(drift::unordered_parallelism)]` once confirmed.
    ///
    /// ### Example
    /// ```rust,ignore
    /// use rayon::prelude::*;
    /// units.par_iter().for_each(|u| apply_damage(u)); // order not guaranteed
    /// ```
    pub DRIFT_UNORDERED_PARALLELISM,
    Warn,
    "rayon parallel iteration in code reachable from simulation state"
}

declare_lint_pass!(UnorderedParallelism => [DRIFT_UNORDERED_PARALLELISM]);

const FLAGGED_METHODS: &[&str] = &["par_iter", "par_iter_mut", "into_par_iter", "par_bridge"];

impl<'tcx> LateLintPass<'tcx> for UnorderedParallelism {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        let ExprKind::MethodCall(segment, _receiver, _args, _span) = expr.kind else {
            return;
        };
        if !FLAGGED_METHODS.contains(&segment.ident.name.as_str()) {
            return;
        }
        // Best-effort scoping to rayon's own methods: check the resolved
        // method's definition path starts with "rayon::" rather than
        // flagging any crate's unrelated method sharing this name.
        let Some(method_def_id) = cx.typeck_results().type_dependent_def_id(expr.hir_id) else {
            return;
        };
        let def_path = cx.tcx.def_path_str(method_def_id);
        if !def_path.starts_with("rayon::") {
            return;
        }
        span_lint_and_help(
            cx,
            DRIFT_UNORDERED_PARALLELISM,
            expr.span,
            "rayon parallel iteration — result order is scheduler-dependent",
            None,
            "confirm the terminal reduction is order-independent (commutative), or suppress if already confirmed",
        );
    }
}
