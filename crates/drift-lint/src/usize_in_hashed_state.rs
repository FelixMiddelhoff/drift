use clippy_utils::diagnostics::span_lint_and_help;
use rustc_hir::attrs::AttributeKind;
use rustc_hir::{Item, ItemKind, find_attr};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::ty::TyKind;
use rustc_session::{declare_lint, declare_lint_pass};

declare_lint! {
    /// ### What it does
    /// Flags `usize`/`isize` fields on a struct that also derives `Hash`.
    ///
    /// ### Why is this bad?
    /// `usize`/`isize` are pointer-width — 32 bits on a 32-bit target, 64
    /// on a 64-bit one. A struct hashed for a cross-peer sync check (the
    /// same role Foldback's `FoldbackHash`-style derive plays) that
    /// contains one will hash differently on a 32-bit vs. 64-bit build of
    /// the same logical state, producing a false desync report between
    /// two otherwise-correct peers on different architectures.
    ///
    /// ### Known problems
    /// Flags any `#[derive(Hash)]`, not specifically a sync/rollback
    /// hashing derive — `std::hash::Hash` is a reasonable, real-world
    /// proxy for "this struct's bytes matter for equality/hashing
    /// somewhere," but produces a false positive on a struct hashed only
    /// for something width-insensitive (e.g. a `HashMap` key never
    /// compared across processes). Suppress with
    /// `#[allow(drift_usize_in_hashed_state)]` in that case.
    ///
    /// Deliberately **not** scoped by `dylint.toml`'s
    /// `tick_reachable_roots` (see `crate::reachability`), unlike this
    /// crate's other rules: reachability is a call-graph-from-a-function
    /// concept, and this rule flags a *struct field definition*, not code
    /// inside a function — the struct could be constructed and hashed
    /// from a reachable function regardless of where it's declared, so
    /// scoping by call-graph reachability wouldn't make sense here.
    ///
    /// ### Example
    /// ```rust
    /// #[derive(Hash)]
    /// struct Unit {
    ///     id: usize, // flagged — use u32/u64 instead
    /// }
    /// ```
    pub DRIFT_USIZE_IN_HASHED_STATE,
    Warn,
    "usize/isize field on a struct deriving Hash — width varies 32 vs 64-bit builds"
}

declare_lint_pass!(UsizeInHashedState => [DRIFT_USIZE_IN_HASHED_STATE]);

impl<'tcx> LateLintPass<'tcx> for UsizeInHashedState {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        // A `#[derive(Hash)]` lowers to a compiler-generated
        // `impl Hash for X { .. }` tagged `#[automatically_derived]` —
        // that's the reliable place to detect it, not the original
        // struct's own attribute list (derive attributes don't survive
        // there the way a hand-written attribute would).
        let ItemKind::Impl(impl_) = item.kind else {
            return;
        };
        if !find_attr!(
            cx.tcx,
            item.owner_id.def_id,
            AttributeKind::AutomaticallyDerived
        ) {
            return;
        }
        let Some(trait_ref) = impl_.of_trait else {
            return;
        };
        let Some(trait_def_id) = trait_ref.trait_ref.path.res.opt_def_id() else {
            return;
        };
        let Some(hash_def_id) = cx.tcx.get_diagnostic_item(rustc_span::sym::Hash) else {
            return;
        };
        if trait_def_id != hash_def_id {
            return;
        }
        let self_ty = cx
            .tcx
            .type_of(item.owner_id.def_id)
            .instantiate_identity()
            .skip_normalization();
        let Some(adt_def) = self_ty.ty_adt_def() else {
            return;
        };
        for field in adt_def.all_fields() {
            let field_ty = cx
                .tcx
                .type_of(field.did)
                .instantiate_identity()
                .skip_normalization();
            if let TyKind::Uint(rustc_middle::ty::UintTy::Usize)
            | TyKind::Int(rustc_middle::ty::IntTy::Isize) = field_ty.kind()
            {
                span_lint_and_help(
                    cx,
                    DRIFT_USIZE_IN_HASHED_STATE,
                    cx.tcx.def_span(field.did),
                    "usize/isize field on a struct deriving Hash — width varies across platforms",
                    None,
                    "use a fixed-width integer type (u32/u64/i32/i64) instead",
                );
            }
        }
    }
}
