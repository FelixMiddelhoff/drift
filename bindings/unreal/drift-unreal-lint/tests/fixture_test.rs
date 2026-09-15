//! Turns this session's manual CI-worthiness checks into a real,
//! permanent regression suite — same reasoning as drift-lint's own UI
//! tests: verify an exact diff, not just "the tool ran and didn't crash".
//! Needs `LIBCLANG_PATH` set to a real LLVM install's `bin` directory.

use std::process::{Command, Output};

fn fixture_dir() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixture")
}

fn run(config: Option<&str>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_drift-unreal-lint"));
    cmd.current_dir(fixture_dir()).arg("compile_commands.json");
    if let Some(config) = config {
        cmd.arg(config);
    }
    cmd.output().expect("failed to run drift-unreal-lint")
}

#[test]
fn unseeded_rng_fires_unconditionally_with_no_config() {
    let out = run(None);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "stdout was: {stdout}");

    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1, "expected exactly 1 finding, got: {stdout}");
    assert!(lines[0].contains("pawn.cpp:48:12"));
    assert!(lines[0].contains("[drift-unreal::unseeded_rng]"));
}

#[test]
fn float_chain_flagged_once_per_statement_reachable_functions_only() {
    let out = run(Some("drift-unreal.toml"));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "stdout was: {stdout}");

    // unseeded_rng still fires (unconditional) alongside the opt-in float
    // rule once a config file is present.
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3, "expected exactly 3 findings, got: {stdout}");
    assert!(lines[0].contains("pawn.cpp:33:16"));
    assert!(lines[0].contains("[drift-unreal::float_outside_fixed_step]"));
    assert!(lines[1].contains("pawn.cpp:34:16"));
    assert!(lines[1].contains("[drift-unreal::float_outside_fixed_step]"));
    assert!(lines[2].contains("pawn.cpp:48:12"));
    assert!(lines[2].contains("[drift-unreal::unseeded_rng]"));

    // Same float-chain shape as Simulate, but never called from Tick —
    // reachability scoping must not flag it.
    assert!(!stdout.contains("pawn.cpp:41"));
}

#[test]
fn fixed_step_functions_exemption_suppresses_only_the_float_hit() {
    let out = run(Some("drift-unreal-exempt.toml"));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "stdout was: {stdout}");

    // The float chain in the exempted function is gone; unseeded_rng
    // (unrelated to fixed_step_functions) still fires.
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1, "expected exactly 1 finding, got: {stdout}");
    assert!(lines[0].contains("[drift-unreal::unseeded_rng]"));
}
