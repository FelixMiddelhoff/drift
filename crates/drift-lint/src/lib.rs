#![feature(rustc_private)]
#![warn(unused_extern_crates)]

extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;

mod hashmap_iter;
mod unordered_parallelism;
mod unseeded_rng;
mod usize_in_hashed_state;
mod wallclock_read;

dylint_linting::dylint_library!();

#[expect(clippy::no_mangle_with_rust_abi)]
#[unsafe(no_mangle)]
pub fn register_lints(sess: &rustc_session::Session, lint_store: &mut rustc_lint::LintStore) {
    dylint_linting::init_config(sess);
    lint_store.register_lints(&[
        hashmap_iter::DRIFT_HASHMAP_ITER,
        unseeded_rng::DRIFT_UNSEEDED_RNG,
        wallclock_read::DRIFT_WALLCLOCK_READ,
        unordered_parallelism::DRIFT_UNORDERED_PARALLELISM,
        usize_in_hashed_state::DRIFT_USIZE_IN_HASHED_STATE,
    ]);
    lint_store.register_late_lint_pass(Box::new(|_| Box::new(hashmap_iter::HashmapIter)));
    lint_store.register_late_lint_pass(Box::new(|_| Box::new(unseeded_rng::UnseededRng)));
    lint_store.register_late_lint_pass(Box::new(|_| Box::new(wallclock_read::WallclockRead)));
    lint_store.register_late_lint_pass(Box::new(|_| {
        Box::new(unordered_parallelism::UnorderedParallelism)
    }));
    lint_store.register_late_lint_pass(Box::new(|_| {
        Box::new(usize_in_hashed_state::UsizeInHashedState)
    }));
}

#[test]
fn hashmap_iter_ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui/hashmap_iter");
}

#[test]
fn unseeded_rng_ui() {
    dylint_testing::ui_test_example(env!("CARGO_PKG_NAME"), "unseeded_rng");
}

#[test]
fn wallclock_read_ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui/wallclock_read");
}

#[test]
fn unordered_parallelism_ui() {
    dylint_testing::ui_test_example(env!("CARGO_PKG_NAME"), "unordered_parallelism");
}

#[test]
fn usize_in_hashed_state_ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui/usize_in_hashed_state");
}
