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
fn opt_in_only_no_config_means_no_findings() {
    let out = run(None);
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
}

#[test]
fn float_chain_flagged_once_per_statement_reachable_functions_only() {
    let out = run(Some("drift-unreal.toml"));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "stdout was: {stdout}");

    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "expected exactly 2 findings, got: {stdout}");
    assert!(lines[0].contains("pawn.cpp:26:16"));
    assert!(lines[1].contains("pawn.cpp:27:16"));
    for line in &lines {
        assert!(line.contains("[drift-unreal::float_outside_fixed_step]"));
    }

    // Same float-chain shape as Simulate, but never called from Tick —
    // reachability scoping must not flag it.
    assert!(!stdout.contains("pawn.cpp:34"));
}

#[test]
fn fixed_step_functions_exemption_suppresses_the_hit() {
    let out = run(Some("drift-unreal-exempt.toml"));
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
}
