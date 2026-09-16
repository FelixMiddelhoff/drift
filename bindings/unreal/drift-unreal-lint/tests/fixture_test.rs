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
fn unconditional_rules_fire_with_no_config() {
    let out = run(None);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "stdout was: {stdout}");

    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 6, "expected exactly 6 findings, got: {stdout}");
    assert!(lines[0].contains("pawn.cpp:104:12"));
    assert!(lines[0].contains("[drift-unreal::unseeded_rng]"));
    assert!(lines[1].contains("pawn.cpp:119:12"));
    assert!(lines[1].contains("[drift-unreal::wallclock_read]"));
    assert!(lines[2].contains("pawn.cpp:125:5"));
    assert!(lines[2].contains("[drift-unreal::hashmap_iter]"));
    assert!(lines[3].contains("pawn.cpp:135:15"));
    assert!(lines[3].contains("[drift-unreal::hashmap_iter]"));
    assert!(lines[4].contains("pawn.cpp:166:5"));
    assert!(lines[4].contains("[drift-unreal::unordered_parallelism]"));
    assert!(lines[5].contains("pawn.cpp:193:34"));
    assert!(lines[5].contains("[drift-unreal::usize_in_hashed_state]"));

    // Iterating a TArray, even one built to hold a map's own values and
    // sorted immediately before, must never fire — hashmap_iter's
    // type-based detection only matches TMap/TSet, a different type than
    // whatever a project sorts a map's contents into.
    assert!(!stdout.contains("pawn.cpp:146"));
    // AsyncTask is deliberately excluded from unordered_parallelism (see
    // main.rs's UNORDERED_PARALLELISM_FUNCS comment) — must never fire.
    assert!(!stdout.contains("AsyncTask() dispatches"));
    // RawHandle is SIZE_T but never referenced inside any GetTypeHash
    // overload — must never fire.
    assert!(!stdout.contains("RawHandle"));
}

#[test]
fn float_chain_flagged_once_per_statement_reachable_functions_only() {
    let out = run(Some("drift-unreal.toml"));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "stdout was: {stdout}");

    // unseeded_rng, wallclock_read, hashmap_iter, unordered_parallelism,
    // usize_in_hashed_state still fire (unconditional) alongside the
    // opt-in float rule once a config file is present.
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 8, "expected exactly 8 findings, got: {stdout}");
    assert!(lines[0].contains("pawn.cpp:89:16"));
    assert!(lines[0].contains("[drift-unreal::float_outside_fixed_step]"));
    assert!(lines[1].contains("pawn.cpp:90:16"));
    assert!(lines[1].contains("[drift-unreal::float_outside_fixed_step]"));
    assert!(lines[2].contains("pawn.cpp:104:12"));
    assert!(lines[2].contains("[drift-unreal::unseeded_rng]"));
    assert!(lines[3].contains("pawn.cpp:119:12"));
    assert!(lines[3].contains("[drift-unreal::wallclock_read]"));
    assert!(lines[4].contains("pawn.cpp:125:5"));
    assert!(lines[4].contains("[drift-unreal::hashmap_iter]"));
    assert!(lines[5].contains("pawn.cpp:135:15"));
    assert!(lines[5].contains("[drift-unreal::hashmap_iter]"));
    assert!(lines[6].contains("pawn.cpp:166:5"));
    assert!(lines[6].contains("[drift-unreal::unordered_parallelism]"));
    assert!(lines[7].contains("pawn.cpp:193:34"));
    assert!(lines[7].contains("[drift-unreal::usize_in_hashed_state]"));

    // Same float-chain shape as Simulate, but never called from Tick —
    // reachability scoping must not flag it.
    assert!(!stdout.contains("pawn.cpp:97"));
}

#[test]
fn fixed_step_functions_exemption_suppresses_only_the_float_hit() {
    let out = run(Some("drift-unreal-exempt.toml"));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "stdout was: {stdout}");

    // The float chain in the exempted function is gone; the other
    // unconditional rules (unrelated to fixed_step_functions) still fire.
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 6, "expected exactly 6 findings, got: {stdout}");
    assert!(lines[0].contains("[drift-unreal::unseeded_rng]"));
    assert!(lines[1].contains("[drift-unreal::wallclock_read]"));
    assert!(lines[2].contains("[drift-unreal::hashmap_iter]"));
    assert!(lines[3].contains("[drift-unreal::hashmap_iter]"));
    assert!(lines[4].contains("[drift-unreal::unordered_parallelism]"));
    assert!(lines[5].contains("[drift-unreal::usize_in_hashed_state]"));
}
