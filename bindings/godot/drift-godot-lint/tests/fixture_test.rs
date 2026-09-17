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
    assert_eq!(lines.len(), 5, "expected exactly 5 findings, got: {stdout}");
    assert!(lines[0].contains("player.gd:8:8"));
    assert!(lines[0].contains("[drift-godot::unseeded_rng]"));
    assert!(lines[1].contains("player.gd:20:12"));
    assert!(lines[1].contains("[drift-godot::wallclock_read]"));
    assert!(lines[2].contains("player.gd:34:2"));
    assert!(lines[2].contains("[drift-godot::unordered_parallelism]"));
    assert!(lines[3].contains("player.gd:63:14"));
    assert!(lines[3].contains("[drift-godot::float_outside_fixed_step]"));
    // Compound assignment (`elapsed += delta`, an AssignStmt, not a
    // BinaryExpr) — previously a real, disclosed gap found dogfooding
    // against Pixelorama, fixed here. Fires as its own hit.
    assert!(lines[4].contains("player.gd:74:2"));
    assert!(lines[4].contains("[drift-godot::float_outside_fixed_step]"));

    // A member call on a project-owned seedable RNG instance
    // (`seeded_rng.randi_range(...)`) must never fire — only the bare
    // global call above does.
    assert!(!stdout.contains("player.gd:15"));
    // A locally-defined `get_ticks_msec()` and a call to it must never
    // fire — only `Time.get_ticks_msec()` does.
    assert!(!stdout.contains("player.gd:24"));
    assert!(!stdout.contains("player.gd:27"));
    // An unrelated TaskQueue's own `add_task` method must never fire —
    // only `WorkerThreadPool.add_task()` does.
    assert!(!stdout.contains("player.gd:40"));
    assert!(!stdout.contains("player.gd:44"));
    // Pure untyped-variable arithmetic (`var c = a + b`, neither typed)
    // must never fire, even reachable from _physics_process — no type
    // inference for untyped locals.
    assert!(!stdout.contains("player.gd:59"));
    // Same float-arithmetic shape as apply_gravity, but never called from
    // _process/_physics_process — reachability scoping must exclude it.
    assert!(!stdout.contains("player.gd:80"));
}

/// Real bug found dogfooding against a real 250-file project
/// (Orama-Interactive/Pixelorama): the whole-project function table used to
/// key by bare name in a single `HashMap<String, FuncInfo>`, so two files
/// each defining `_physics_process` (as common as a name gets in a real
/// multi-scene Godot project) collided — the second `.insert()` silently
/// discarded the first, and only the surviving one ever got scanned as a
/// reachability root. `tests/fixture/collision/{a,b}.gd` both define
/// `_physics_process` with their own distinct qualifying float
/// arithmetic — both must fire when the directory is scanned together,
/// proving same-named functions across files no longer collide.
#[test]
fn same_named_functions_across_files_do_not_collide() {
    let out = Command::new(env!("CARGO_BIN_EXE_drift-godot-lint"))
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixture/collision"
        ))
        .output()
        .expect("failed to run drift-godot-lint");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "stdout was: {stdout}");

    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "expected exactly 2 findings, got: {stdout}");
    assert!(lines[0].contains("a.gd:13:11"));
    assert!(lines[0].contains("[drift-godot::float_outside_fixed_step]"));
    assert!(lines[1].contains("b.gd:9:8"));
    assert!(lines[1].contains("[drift-godot::float_outside_fixed_step]"));
}
