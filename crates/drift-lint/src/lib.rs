#![feature(rustc_private)]
#![warn(unused_extern_crates)]

extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_session;
extern crate rustc_span;

mod hashmap_iter;
mod unseeded_rng;

dylint_linting::dylint_library!();

#[expect(clippy::no_mangle_with_rust_abi)]
#[unsafe(no_mangle)]
pub fn register_lints(sess: &rustc_session::Session, lint_store: &mut rustc_lint::LintStore) {
    dylint_linting::init_config(sess);
    lint_store.register_lints(&[
        hashmap_iter::DRIFT_HASHMAP_ITER,
        unseeded_rng::DRIFT_UNSEEDED_RNG,
    ]);
    lint_store.register_late_lint_pass(Box::new(|_| Box::new(hashmap_iter::HashmapIter)));
    lint_store.register_late_lint_pass(Box::new(|_| Box::new(unseeded_rng::UnseededRng)));
}

#[test]
fn hashmap_iter_ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui/hashmap_iter");
}

#[test]
fn unseeded_rng_ui() {
    dylint_testing::ui_test_example(env!("CARGO_PKG_NAME"), "unseeded_rng");
}
