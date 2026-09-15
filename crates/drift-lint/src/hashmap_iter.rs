use crate::reachability::{Config, Reachable, load_config};
use clippy_utils::diagnostics::span_lint_and_help;
use rustc_hir::def::Res;
use rustc_hir::{Block, Expr, ExprKind, HirId, PatKind, QPath, StmtKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_session::{declare_lint, impl_lint_pass};
use rustc_span::sym;
use std::collections::HashSet;

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
    /// traversed, a documented under-approximation.
    ///
    /// Recognizes one specific safe pattern and doesn't fire on it: a
    /// `let` binding whose initializer ends in `.collect()` off a flagged
    /// call, immediately followed (next statement, same block) by a
    /// `.sort()`/`.sort_by()`/`.sort_by_key()`/`.sort_unstable()`/
    /// `.sort_unstable_by()`/`.sort_unstable_by_key()` call on that same
    /// binding — exactly the pattern found dogfooding against Foldback's
    /// `hashable.rs` (see docs/rule-catalog.md). Anything less direct
    /// (sorting a few statements later, sorting a field the collected
    /// value was moved into, sorting through a helper function) still
    /// fires — this is pattern-matching one known shape, not real
    /// dataflow analysis. Suppress with `#[allow(drift_hashmap_iter)]`
    /// for those.
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
    /// `HirId`s of flagged-method call sites already proven safe by the
    /// collect-then-sort pattern (see `check_block`) — populated before
    /// the corresponding `check_expr` call fires, since the enclosing
    /// block is visited before its statements' sub-expressions in the
    /// normal top-down HIR walk.
    safe_calls: HashSet<HirId>,
}

impl HashmapIter {
    pub fn new() -> Self {
        Self {
            config: load_config(),
            reachable: Reachable::default(),
            safe_calls: HashSet::new(),
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

const SORT_METHODS: &[&str] = &[
    "sort",
    "sort_by",
    "sort_by_key",
    "sort_unstable",
    "sort_unstable_by",
    "sort_unstable_by_key",
];

fn is_flagged_call<'tcx>(cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) -> bool {
    let ExprKind::MethodCall(segment, receiver, _args, _span) = expr.kind else {
        return false;
    };
    if !FLAGGED_METHODS.contains(&segment.ident.name.as_str()) {
        return false;
    }
    let receiver_ty = cx.typeck_results().expr_ty_adjusted(receiver).peel_refs();
    let Some(adt_def) = receiver_ty.ty_adt_def() else {
        return false;
    };
    let def_id = adt_def.did();
    cx.tcx.is_diagnostic_item(sym::HashMap, def_id)
        || cx.tcx.is_diagnostic_item(sym::HashSet, def_id)
}

/// Walks a method-call receiver chain (`x.foo().bar().baz()`) looking for
/// a flagged call anywhere in it, returning that call's `HirId`.
fn find_flagged_call_in_chain<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Option<HirId> {
    if is_flagged_call(cx, expr) {
        return Some(expr.hir_id);
    }
    let ExprKind::MethodCall(_, receiver, _, _) = expr.kind else {
        return None;
    };
    find_flagged_call_in_chain(cx, receiver)
}

fn local_binding_id(pat_kind: &rustc_hir::PatKind<'_>) -> Option<HirId> {
    if let PatKind::Binding(_, hir_id, _, _) = pat_kind {
        Some(*hir_id)
    } else {
        None
    }
}

fn sort_call_target(expr: &Expr<'_>) -> Option<HirId> {
    let ExprKind::MethodCall(segment, receiver, _args, _span) = expr.kind else {
        return None;
    };
    let name = segment.ident.name.as_str();
    if !SORT_METHODS.contains(&name) {
        return None;
    }
    let ExprKind::Path(QPath::Resolved(_, path)) = receiver.kind else {
        return None;
    };
    let Res::Local(hir_id) = path.res else {
        return None;
    };
    let _ = name;
    Some(hir_id)
}

impl<'tcx> LateLintPass<'tcx> for HashmapIter {
    fn check_crate(&mut self, cx: &LateContext<'tcx>) {
        self.reachable = Reachable::compute(cx, &self.config);
    }

    fn check_block(&mut self, cx: &LateContext<'tcx>, block: &'tcx Block<'tcx>) {
        for pair in block.stmts.windows(2) {
            let [first, second] = pair else { continue };
            let StmtKind::Let(local) = first.kind else {
                continue;
            };
            let Some(binding_id) = local_binding_id(&local.pat.kind) else {
                continue;
            };
            let Some(init) = local.init else {
                continue;
            };
            // The binding's initializer must itself end in `.collect()`
            // for this to be the pattern we recognize (collect, then
            // sort) — a bare `.iter()` binding immediately followed by a
            // `.sort()` on something else entirely shouldn't match.
            let ExprKind::MethodCall(collect_segment, collect_receiver, _, _) = init.kind else {
                continue;
            };
            if collect_segment.ident.name.as_str() != "collect" {
                continue;
            }
            let (StmtKind::Semi(second_expr) | StmtKind::Expr(second_expr)) = second.kind else {
                continue;
            };
            let Some(sorted_id) = sort_call_target(second_expr) else {
                continue;
            };
            if sorted_id != binding_id {
                continue;
            }
            if let Some(flagged_id) = find_flagged_call_in_chain(cx, collect_receiver) {
                self.safe_calls.insert(flagged_id);
            }
        }
    }

    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if !is_flagged_call(cx, expr) {
            return;
        }
        if self.safe_calls.contains(&expr.hir_id) {
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
