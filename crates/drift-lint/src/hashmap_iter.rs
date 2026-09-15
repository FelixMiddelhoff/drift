use clippy_utils::diagnostics::span_lint_and_help;
use rustc_hir::{Expr, ExprKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_session::{declare_lint, declare_lint_pass};
use rustc_span::sym;

declare_lint! {
    /// ### What it does
    /// Flags iteration over a `HashMap`/`HashSet` (`.iter()`, `.keys()`,
    /// `.values()`, `.drain()`, `.into_iter()`, and their `_mut` variants).
    ///
    /// ### Why is this bad?
    /// `HashMap`/`HashSet` iteration order is not guaranteed stable across
    /// processes, platforms, or even separate runs with the same input —
    /// using it to drive anything that affects simulated state (spawn
    /// order, damage application order, event dispatch order) is a classic
    /// source of a rollback-netcode desync between peers whose hash tables
    /// happened to land in a different bucket order.
    ///
    /// ### Known problems
    /// This is a blunt, unconditional check — it doesn't yet know whether
    /// the iteration site is reachable from tagged simulation code
    /// (drift-planning/drift-plan.md §5's `#[drift::tick_reachable]` tiered
    /// integration model is not implemented by this prototype), so it will
    /// flag legitimate non-deterministic uses (UI, logging, debug tooling)
    /// too. Suppress with `#[allow(drift::hashmap_iter)]` at those sites
    /// until reachability scoping exists.
    ///
    /// ### Example
    /// ```rust
    /// # use std::collections::HashMap;
    /// # let units: HashMap<u32, u32> = HashMap::new();
    /// for (id, _unit) in units.iter() {
    ///     // order of `id` here is not guaranteed the same on every peer
    /// }
    /// ```
    /// Use instead a `BTreeMap`, or sort the keys before iterating:
    /// ```rust
    /// # use std::collections::HashMap;
    /// # let units: HashMap<u32, u32> = HashMap::new();
    /// let mut ids: Vec<_> = units.keys().collect();
    /// ids.sort();
    /// for id in ids {
    ///     // deterministic order
    /// }
    /// ```
    pub DRIFT_HASHMAP_ITER,
    Warn,
    "iterating a HashMap/HashSet, whose order is not guaranteed stable across peers"
}

declare_lint_pass!(HashmapIter => [DRIFT_HASHMAP_ITER]);

const FLAGGED_METHODS: &[&str] = &[
    "iter",
    "iter_mut",
    "into_iter",
    "keys",
    "values",
    "values_mut",
    "drain",
];

impl<'tcx> LateLintPass<'tcx> for HashmapIter {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        let ExprKind::MethodCall(segment, receiver, _args, _span) = expr.kind else {
            return;
        };
        if !FLAGGED_METHODS.contains(&segment.ident.name.as_str()) {
            return;
        }
        let receiver_ty = cx.typeck_results().expr_ty_adjusted(receiver).peel_refs();
        let Some(adt_def) = receiver_ty.ty_adt_def() else {
            return;
        };
        let def_id = adt_def.did();
        let is_hash_container = cx.tcx.is_diagnostic_item(sym::HashMap, def_id)
            || cx.tcx.is_diagnostic_item(sym::HashSet, def_id);
        if !is_hash_container {
            return;
        }
        span_lint_and_help(
            cx,
            DRIFT_HASHMAP_ITER,
            expr.span,
            "iterating a HashMap/HashSet — order is not guaranteed stable across peers",
            None,
            "use a BTreeMap/BTreeSet, or sort the keys before iterating, if this feeds simulated state",
        );
    }
}
