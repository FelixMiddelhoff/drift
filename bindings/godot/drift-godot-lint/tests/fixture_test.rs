//! A real, permanent regression suite — verifies an exact diff, not just
//! "the tool ran and didn't crash", same discipline as drift-unreal-lint's
//! own fixture test.

use std::process::{Command, Output};

fn run() -> Output {
    Command::new(env!("CARGO_BIN_EXE_drift-godot-lint"))
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixture/player.gd"
        ))
        .output()
        .expect("failed to run drift-godot-lint")
}

#[test]
fn unconditional_rules_fire_on_bare_calls_only() {
    let out = run();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "stdout was: {stdout}");

    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "expected exactly 2 findings, got: {stdout}");
    assert!(lines[0].contains("player.gd:8:8"));
    assert!(lines[0].contains("[drift-godot::unseeded_rng]"));
    assert!(lines[1].contains("player.gd:20:12"));
    assert!(lines[1].contains("[drift-godot::wallclock_read]"));

    // A member call on a project-owned seedable RNG instance
    // (`seeded_rng.randi_range(...)`) must never fire — only the bare
    // global call above does.
    assert!(!stdout.contains("player.gd:15"));
    // A locally-defined `get_ticks_msec()` and a call to it must never
    // fire — only `Time.get_ticks_msec()` does.
    assert!(!stdout.contains("player.gd:24"));
    assert!(!stdout.contains("player.gd:27"));
}
