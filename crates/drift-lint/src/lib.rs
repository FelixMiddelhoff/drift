#![feature(rustc_private)]
#![warn(unused_extern_crates)]

extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;

mod float_outside_fixed_step;
mod hashmap_iter;
mod reachability;
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
        float_outside_fixed_step::DRIFT_FLOAT_OUTSIDE_FIXED_STEP,
    ]);
    lint_store.register_late_lint_pass(Box::new(|_| Box::new(hashmap_iter::HashmapIter::new())));
    lint_store.register_late_lint_pass(Box::new(|_| Box::new(unseeded_rng::UnseededRng::new())));
    lint_store
        .register_late_lint_pass(Box::new(|_| Box::new(wallclock_read::WallclockRead::new())));
    lint_store.register_late_lint_pass(Box::new(|_| {
        Box::new(unordered_parallelism::UnorderedParallelism::new())
    }));
    lint_store.register_late_lint_pass(Box::new(|_| {
        Box::new(usize_in_hashed_state::UsizeInHashedState)
    }));
    lint_store.register_late_lint_pass(Box::new(|_| {
        Box::new(float_outside_fixed_step::FloatOutsideFixedStep::new())
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

/// Not a `ui_test`/`ui_test_example` fixture like the others — proves the
/// `dylint.toml`-based reachability scoping (crate::reachability) itself
/// actually scopes, and that `drift::float_outside_fixed_step` (opt-in
/// only, and its own outermost-expression-only dedup for a chain like
/// `a + b + c`) does too. `tests/reachability_fixture` has:
/// - a `HashMap::iter()` and a float-arithmetic chain inside `helper()`,
///   reachable from the configured `tick` root — both should fire, the
///   float one exactly once despite being a 2-operator chain.
/// - the same float arithmetic inside `physics_integrate()`, which
///   `dylint.toml` lists in `fixed_step_functions` — should NOT fire.
/// - the same `HashMap::iter()` and a float op inside `unreachable_fn()`,
///   not reachable from `tick` at all — neither should fire.
///
/// Shells out to a real `cargo build` + `cargo dylint --lib-path`, the
/// same invocation this session used by hand to first prove reachability
/// scoping actually worked (an earlier version of that feature's own
/// manual check silently produced zero warnings at all — a real bug: the
/// root-path config format didn't match what `TyCtxt::def_path_str`
/// actually returns for a local item — caught only by that manual check,
/// which is why this exists as a permanent test rather than trusting the
/// implementation on faith).
#[test]
fn reachability_scoping() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");

    let build_status = std::process::Command::new("cargo")
        .arg("build")
        .current_dir(manifest_dir)
        .status()
        .expect("cargo build failed to run");
    assert!(
        build_status.success(),
        "cargo build of drift-lint itself failed"
    );

    let libs_json =
        dylint_testing::dylint_libs(env!("CARGO_PKG_NAME")).expect("dylint_libs failed");
    let libs: Vec<String> =
        serde_json::from_str(&libs_json).expect("dylint_libs did not return a JSON array");
    let lib_path = libs.first().expect("dylint_libs returned an empty list");

    let fixture_dir = std::path::Path::new(manifest_dir)
        .join("tests")
        .join("reachability_fixture");
    let output = std::process::Command::new("cargo")
        .args(["dylint", "--lib-path", lib_path])
        .current_dir(&fixture_dir)
        .output()
        .expect("cargo dylint failed to run");
    let stderr = String::from_utf8_lossy(&output.stderr);

    let hashmap_hits = stderr.matches("iterating a HashMap/HashSet").count();
    assert_eq!(
        hashmap_hits, 1,
        "expected exactly one reachability-scoped hashmap_iter hit, got:\n{stderr}"
    );

    let float_hits = stderr.matches("non-associative float arithmetic").count();
    assert_eq!(
        float_hits, 1,
        "expected exactly one float_outside_fixed_step hit (the reachable, non-exempt chain, deduped to its outermost expression), got:\n{stderr}"
    );
}
