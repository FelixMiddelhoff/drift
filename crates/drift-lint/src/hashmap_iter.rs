use crate::reachability::{Config, Reachable, load_config};
use clippy_utils::diagnostics::span_lint_and_help;
use rustc_hir::{Expr, ExprKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_session::{declare_lint, impl_lint_pass};
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
    /// Unconditional unless a `dylint.toml` configures
    /// `tick_reachable_roots` (see `crate::reachability`), in which case
    /// only sites reachable from those roots via a direct, intra-crate
    /// call graph fire — `dyn Trait`/fn-pointer call targets aren't
    /// traversed, a documented under-approximation. Also known to
    /// false-positive on code that collects into a `Vec` and sorts it
    /// immediately after the flagged call (see docs/rule-catalog.md for a
    /// real example found dogfooding against Foldback) — the rule doesn't
    /// look ahead for a following sort. Suppress with
    /// `#[allow(drift_hashmap_iter)]` at either kind of site.
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

pub struct HashmapIter {
    config: Config,
    reachable: Reachable,
}

impl HashmapIter {
    pub fn new() -> Self {
        Self {
            config: load_config(),
            reachable: Reachable::default(),
        }
    }
}

impl_lint_pass!(HashmapIter => [DRIFT_HASHMAP_ITER]);

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
    fn check_crate(&mut self, cx: &LateContext<'tcx>) {
        self.reachable = Reachable::compute(cx, &self.config);
    }

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
        if !self
            .reachable
            .includes(cx.tcx.hir_enclosing_body_owner(expr.hir_id))
        {
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
