//! Optional call-graph reachability scoping (drift-planning/drift-plan.md
//! §5's `#[drift::tick_reachable]` design, reworked after research this
//! session found the original attribute-based design isn't viable: a
//! third-party dylint tool can't register `drift` as a real rustc "tool"
//! namespace without every *consumer* crate opting into
//! `#![feature(register_tool)]` on nightly — unacceptable for a lint meant
//! to run against arbitrary stable-Rust game code. `dylint.toml` config
//! (a mechanism `dylint_linting` already ships, see its own crate docs)
//! is the real, stable-compatible equivalent: no consumer toolchain or
//! dependency changes needed, just a config file.
//!
//! # Usage
//! ```toml
//! # dylint.toml, in the target workspace root
//! [drift-lint]
//! tick_reachable_roots = ["simulation::tick"]
//! ```
//! Root paths are matched against `TyCtxt::def_path_str`, which is
//! crate-*relative* and does not include the current crate's own name —
//! confirmed for real (a first attempt using a `my_crate::`-prefixed path
//! silently matched nothing, no error, just zero function ever counted
//! as a root — caught by adding a debug print of every candidate path
//! before trusting the fixture test below). A plain top-level function is
//! just its own name (`"tick"`); a nested one is `module::path::to::fn`.
//! When configured, every rule in this crate only fires on code reachable
//! (via a direct, intra-crate call graph — see [`Config`]'s docs for the
//! real, documented under-approximation this implies) from one of the
//! listed root functions. When *not* configured (no `dylint.toml`, or no
//! `tick_reachable_roots` key), every rule fires unconditionally, exactly
//! the prototype's original behavior — this is opt-in, not a breaking
//! change to existing usage.

use clippy_utils::res::MaybeDef;
use rustc_hir::def_id::LocalDefId;
use rustc_hir::intravisit::{Visitor, walk_expr};
use rustc_hir::{Expr, ExprKind};
use rustc_lint::LateContext;
use rustc_middle::hir::nested_filter;
use serde::Deserialize;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub tick_reachable_roots: Vec<String>,
    /// Functions exempt from `drift::float_outside_fixed_step` even when
    /// reachable — the fixed-timestep integrator's own internals, where
    /// float arithmetic is the whole point. Matched the same way as
    /// `tick_reachable_roots`: against `TyCtxt::def_path_str`, crate-
    /// relative, no crate-name prefix.
    #[serde(default)]
    pub fixed_step_functions: Vec<String>,
}

pub fn load_config() -> Config {
    dylint_linting::config_or_default(env!("CARGO_PKG_NAME"))
}

/// `None` means "not configured" — every call site should treat this as
/// "flag everywhere," the pre-reachability-scoping behavior.
#[derive(Default)]
pub struct Reachable(Option<HashSet<LocalDefId>>);

impl Reachable {
    pub fn compute(cx: &LateContext<'_>, config: &Config) -> Self {
        if config.tick_reachable_roots.is_empty() {
            return Self(None);
        }

        // Direct, intra-crate call graph only: `dyn Trait`/fn-pointer call
        // targets aren't resolvable via a plain HIR walk, so they simply
        // don't add an edge — a real, documented under-approximation (the
        // reachable set can miss real paths through virtual dispatch),
        // the opposite of the plan's original "over-approximate, flag
        // anything possibly reachable" sketch. Chosen deliberately: an
        // under-approximation produces false negatives (a missed rule
        // firing), not false positives (a wrong rule firing on code that
        // provably isn't reachable) — the latter is what actually erodes
        // trust in a lint tool (see drift-plan.md §8), so err toward it.
        let mut graph: HashMap<LocalDefId, Vec<LocalDefId>> = HashMap::new();
        for owner in cx.tcx.hir_body_owners() {
            // Not every body owner has typeck results usable via
            // `cx.typeck_results()` — that relies on the driver having
            // set the *current* enclosing body during normal lint-pass
            // callbacks (check_fn/check_body/etc.), which manually
            // walking every body here from `check_crate` bypasses.
            // `cx.tcx.typeck(owner)` is the same underlying query,
            // fetched directly, independent of that cached state —
            // confirmed for real: the naive `cx.typeck_results()`
            // version panicked ("called outside of body") on this
            // fixture's `main` on the very first attempt.
            let typeck_results = cx.tcx.typeck(owner);
            let body = cx.tcx.hir_body_owned_by(owner);
            let mut collector = CallCollector {
                cx,
                typeck_results,
                callees: Vec::new(),
            };
            collector.visit_expr(body.value);
            graph.insert(owner, collector.callees);
        }

        let root_paths: HashSet<&str> = config
            .tick_reachable_roots
            .iter()
            .map(String::as_str)
            .collect();
        let mut frontier: VecDeque<LocalDefId> = graph
            .keys()
            .filter(|def_id| root_paths.contains(cx.tcx.def_path_str(def_id.to_def_id()).as_str()))
            .copied()
            .collect();

        let mut reached: HashSet<LocalDefId> = frontier.iter().copied().collect();
        while let Some(def_id) = frontier.pop_front() {
            let Some(callees) = graph.get(&def_id) else {
                continue;
            };
            for &callee in callees {
                if reached.insert(callee) {
                    frontier.push_back(callee);
                }
            }
        }

        Self(Some(reached))
    }

    /// Should a lint fire on something inside this function?
    pub fn includes(&self, def_id: LocalDefId) -> bool {
        match &self.0 {
            None => true,
            Some(reached) => reached.contains(&def_id),
        }
    }
}

struct CallCollector<'a, 'tcx> {
    cx: &'a LateContext<'tcx>,
    // Deliberately not `cx.typeck_results()` — see the comment above this
    // struct's only construction site (in `Reachable::compute`) for why
    // that panics when called outside the driver's normal per-body lint
    // callbacks, which manually walking every body here bypasses.
    typeck_results: &'tcx rustc_middle::ty::TypeckResults<'tcx>,
    callees: Vec<LocalDefId>,
}

impl<'tcx> Visitor<'tcx> for CallCollector<'_, 'tcx> {
    type NestedFilter = nested_filter::OnlyBodies;

    fn maybe_tcx(&mut self) -> Self::MaybeTyCtxt {
        self.cx.tcx
    }

    fn visit_expr(&mut self, ex: &'tcx Expr<'tcx>) {
        let def_id = match ex.kind {
            ExprKind::Call(callee, _) => resolve_call_target(callee, self.typeck_results),
            ExprKind::MethodCall(..) => self.typeck_results.type_dependent_def_id(ex.hir_id),
            _ => None,
        };
        if let Some(def_id) = def_id
            && let Some(local) = def_id.as_local()
        {
            self.callees.push(local);
        }
        walk_expr(self, ex);
    }
}

/// Resolves a list of `tick_reachable_roots`/`fixed_step_functions`-style
/// config paths to the actual `LocalDefId`s of same-named functions in
/// this crate — shared by `Reachable::compute` and
/// `float_outside_fixed_step`'s exemption list, same matching rule (see
/// the module docs above for the crate-relative `def_path_str` gotcha).
pub fn resolve_named_functions(cx: &LateContext<'_>, names: &[String]) -> HashSet<LocalDefId> {
    let wanted: HashSet<&str> = names.iter().map(String::as_str).collect();
    cx.tcx
        .hir_body_owners()
        .filter(|def_id| wanted.contains(cx.tcx.def_path_str(def_id.to_def_id()).as_str()))
        .collect()
}

fn resolve_call_target(
    callee: &Expr<'_>,
    typeck_results: &rustc_middle::ty::TypeckResults<'_>,
) -> Option<rustc_hir::def_id::DefId> {
    let ExprKind::Path(qpath) = &callee.kind else {
        return None;
    };
    match qpath {
        rustc_hir::QPath::Resolved(_, path) => path.res.opt_def_id(),
        rustc_hir::QPath::TypeRelative(..) => typeck_results.type_dependent_def_id(callee.hir_id),
    }
}
