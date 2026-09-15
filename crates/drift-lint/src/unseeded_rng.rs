use crate::reachability::{Config, Reachable, load_config};
use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::res::{MaybeDef, MaybeQPath};
use rustc_hir::{Expr, ExprKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_session::{declare_lint, impl_lint_pass};

declare_lint! {
    /// ### What it does
    /// Flags calls to `rand::thread_rng()` and `rand::random()` — RNG
    /// sources seeded from OS entropy rather than an explicit, tracked
    /// seed.
    ///
    /// ### Why is this bad?
    /// Two peers in a lockstep/rollback simulation must see the same
    /// random sequence to stay in sync. An RNG seeded from OS entropy is
    /// different per peer (and per run) by construction — any simulation
    /// decision that reads from it desyncs immediately.
    ///
    /// ### Known problems
    /// Unconditional unless a `dylint.toml` configures
    /// `tick_reachable_roots` (see `crate::reachability`) — with it, only
    /// code reachable from those roots fires. Without it (or for
    /// genuinely cosmetic randomness — particle effects, UI flourish),
    /// suppress with `#[allow(drift_unseeded_rng)]` at the call site.
    ///
    /// ### Example
    /// ```rust
    /// let x: u32 = rand::random();
    /// ```
    /// Use instead an RNG constructed from an explicit, tracked seed
    /// (e.g. `rand::rngs::StdRng::seed_from_u64(tick_seed)`), fed by your
    /// simulation's own deterministic seed source.
    pub DRIFT_UNSEEDED_RNG,
    Warn,
    "RNG seeded from OS entropy inside code reachable from simulation state"
}

pub struct UnseededRng {
    config: Config,
    reachable: Reachable,
}

impl UnseededRng {
    pub fn new() -> Self {
        Self {
            config: load_config(),
            reachable: Reachable::default(),
        }
    }
}

impl_lint_pass!(UnseededRng => [DRIFT_UNSEEDED_RNG]);

const FLAGGED_PATHS: &[&str] = &["rand::thread_rng", "rand::random"];

impl<'tcx> LateLintPass<'tcx> for UnseededRng {
    fn check_crate(&mut self, cx: &LateContext<'tcx>) {
        self.reachable = Reachable::compute(cx, &self.config);
    }

    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        let ExprKind::Call(callee, _args) = expr.kind else {
            return;
        };
        let Some(def_id) = callee.res(cx).opt_def_id() else {
            return;
        };
        let def_path = cx.tcx.def_path_str(def_id);
        if !FLAGGED_PATHS.contains(&def_path.as_str()) {
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
            DRIFT_UNSEEDED_RNG,
            expr.span,
            "call to an OS-entropy-seeded RNG source",
            None,
            "use an explicit, tracked seed (e.g. StdRng::seed_from_u64) fed by your simulation's deterministic seed",
        );
    }
}
