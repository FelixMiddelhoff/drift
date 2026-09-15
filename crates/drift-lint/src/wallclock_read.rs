use crate::reachability::{Config, Reachable, load_config};
use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::res::{MaybeDef, MaybeQPath};
use rustc_hir::{Expr, ExprKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_session::{declare_lint, impl_lint_pass};

declare_lint! {
    /// ### What it does
    /// Flags calls to `std::time::SystemTime::now()` and
    /// `std::time::Instant::now()`.
    ///
    /// ### Why is this bad?
    /// Two peers in a lockstep/rollback simulation read different wall-clock
    /// values by construction (different machines, different boot times,
    /// different scheduling jitter) — using either to drive anything that
    /// affects simulated state desyncs immediately. This is fine for
    /// profiling/logging/UI, which is exactly why this lint is noisy by
    /// design (see Known problems) rather than trying to guess intent.
    ///
    /// ### Known problems
    /// Unconditional unless a `dylint.toml` configures
    /// `tick_reachable_roots` (see `crate::reachability`) — with it, only
    /// code reachable from those roots fires. Without it, suppress with
    /// `#[allow(drift_wallclock_read)]` at legitimate profiling/logging
    /// call sites.
    ///
    /// ### Example
    /// ```rust
    /// use std::time::Instant;
    /// let start = Instant::now();
    /// ```
    /// If this value affects simulated state, replace it with your
    /// simulation's own deterministic tick counter/timestamp instead.
    pub DRIFT_WALLCLOCK_READ,
    Warn,
    "wall-clock read (SystemTime::now/Instant::now) inside code reachable from simulation state"
}

pub struct WallclockRead {
    config: Config,
    reachable: Reachable,
}

impl WallclockRead {
    pub fn new() -> Self {
        Self {
            config: load_config(),
            reachable: Reachable::default(),
        }
    }
}

impl_lint_pass!(WallclockRead => [DRIFT_WALLCLOCK_READ]);

const FLAGGED_PATHS: &[&str] = &["std::time::SystemTime::now", "std::time::Instant::now"];

impl<'tcx> LateLintPass<'tcx> for WallclockRead {
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
            DRIFT_WALLCLOCK_READ,
            expr.span,
            "wall-clock read — not guaranteed the same across peers",
            None,
            "use your simulation's own deterministic tick counter if this feeds simulated state",
        );
    }
}
