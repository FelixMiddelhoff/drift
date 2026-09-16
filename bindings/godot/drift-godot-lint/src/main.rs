//! Standalone determinism checker for Godot GDScript, via
//! [`gdck-syntax`](https://crates.io/crates/gdck-syntax) — a real, lossless
//! GDScript 4 parser. See
//! D:\Claude\drift-planning\drift-godot-unreal-plan.md §9 for the
//! feasibility spike that validated the foundation before any of this was
//! written: 468/470 files of a real, 470-file Godot 4 game
//! (`SlayHorizon/godot-tiny-mmo`) parsed clean, and a spike visitor found
//! real hits for both rules below in that same real codebase.
//!
//! v1: 2 of the 5 shared-taxonomy rules (see
//! drift-godot-unreal-plan.md §4) — the two the spike already confirmed
//! real targets for. Both fire unconditionally, matched by call-site
//! spelling (a bare `NameRef` or a `Type.method`-shaped `AttributeExpr`
//! callee) — same detection shape `drift-unreal-lint`'s `unseeded_rng`/
//! `wallclock_read` already use and already proved robust there.
//! `hashmap_iter` does not port to Godot (§4: `Dictionary` is
//! insertion-ordered by engine guarantee). `unordered_parallelism` and
//! `usize_in_hashed_state` (GDScript has no fixed/pointer-width integer
//! distinction — `int` is always 64-bit — so this one is N/A per §4, not
//! deferred) are not built here; `float_outside_fixed_step` needs the same
//! reachability-scoping precondition the Rust/Unreal sides required before
//! it was viable, not yet designed for GDScript.

use gdck_syntax::{parse, LineIndex, SyntaxKind, SyntaxNode};
use std::fs;
use std::path::{Path, PathBuf};

/// Godot's global RNG (`@GlobalScope`'s `randi`/`randf`/etc., all backed by
/// the same process-global state) is auto-seeded from OS entropy unless a
/// project explicitly calls `seed(value)` with a tracked value — same
/// hazard as Rust's `rand::thread_rng()`/Unreal's `FMath::Rand()`.
/// `randomize()` itself is included too: it explicitly reseeds *from* OS
/// entropy, the opposite of a tracked seed, so a call to it is exactly as
/// real a hazard as a bare read.
///
/// Matched as a **bare** `NameRef` callee — deliberately excludes a
/// member-call shape like `generator.randi()`, which is what a project's
/// own deterministic RNG wrapper (a real, confirmed pattern: the
/// `unseeded_rng`-adjacent addon named in drift-godot-unreal-plan.md §1's
/// own demand check ships exactly this shape, `NetworkRandomNumberGenerator`
/// wrapping a seedable `RandomNumberGenerator` instance) looks like at the
/// call site. A bare `randi()` always resolves to the one true global RNG;
/// a member call never does.
const RNG_FUNCS: &[&str] = &[
    "randi",
    "randf",
    "randi_range",
    "randf_range",
    "randfn",
    "randomize",
];

/// Wallclock/OS-time reads — same hazard as Rust's `wallclock_read`/C#'s
/// DRIFT0003/Unreal's `wallclock_read`: a value that differs per peer/run
/// must never feed simulated state directly. Matched as `Type.method`
/// `AttributeExpr` callees, same shape as Unreal's own
/// `FPlatformTime::Seconds`-style matching.
const WALLCLOCK_FUNCS: &[&str] = &[
    "OS.get_ticks_msec",
    "OS.get_ticks_usec",
    "Time.get_ticks_msec",
    "Time.get_ticks_usec",
    "Time.get_unix_time_from_system",
];

struct Finding {
    file: PathBuf,
    line: u32,
    column: u32,
    rule: &'static str,
    message: String,
}

/// A `CallExpr`'s own first child node is the callee expression: a bare
/// `NameRef` (`randi()`) or a `Type.method`-shaped `AttributeExpr`
/// (`OS.get_ticks_msec()`). A real bug hit building this: `NameRef`/
/// `AttributeExpr` carry their own leading whitespace trivia as a child
/// token (confirmed with a real tree dump, not assumed), so `.text()`
/// returns `" randi_range"`, not `"randi_range"` — every call silently
/// failed to match before `.trim()` was added, the same kind of near-miss
/// `drift-unreal-lint`'s own UI test caught for the Rust side's
/// `unseeded_rng`/`wallclock_read` rules.
fn callee_text(call: SyntaxNode) -> Option<String> {
    let callee = call.child_nodes().next()?;
    match callee.kind() {
        SyntaxKind::NameRef | SyntaxKind::AttributeExpr => Some(callee.text().trim().to_string()),
        _ => None,
    }
}

fn scan_source(path: &Path, source: &str, findings: &mut Vec<Finding>) -> usize {
    let tree = parse(source);
    let lines = LineIndex::new(source);
    for node in tree.root().descendants() {
        if node.kind() != SyntaxKind::CallExpr {
            continue;
        }
        let Some(callee) = callee_text(node) else {
            continue;
        };
        let rule = if RNG_FUNCS.contains(&callee.as_str()) {
            Some((
                "unseeded_rng",
                format!(
                    "{callee}() reads or reseeds Godot's global RNG, which is not \
                     deterministically tracked by default; differs per peer/run"
                ),
            ))
        } else if WALLCLOCK_FUNCS.contains(&callee.as_str()) {
            Some((
                "wallclock_read",
                format!(
                    "{callee}() reads wallclock/OS time, which differs per peer/run; \
                     do not fold it into simulated state"
                ),
            ))
        } else {
            None
        };
        let Some((rule, message)) = rule else {
            continue;
        };
        let loc = lines.line_col(node.range().start());
        findings.push(Finding {
            file: path.to_path_buf(),
            line: loc.line,
            column: loc.col,
            rule,
            message,
        });
    }
    tree.errors().len()
}

/// Skips `.godot`, Godot's own editor cache directory — never real project
/// source, and can be large/irrelevant.
fn collect_gd_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == ".godot") {
                continue;
            }
            collect_gd_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("gd") {
            out.push(path);
        }
    }
}

fn run(root: &Path) -> anyhow::Result<i32> {
    let mut files = Vec::new();
    if root.is_dir() {
        collect_gd_files(root, &mut files);
    } else {
        files.push(root.to_path_buf());
    }
    files.sort();

    let mut findings = Vec::new();
    let mut total_parse_errors = 0usize;
    for path in &files {
        let source = fs::read_to_string(path)?;
        total_parse_errors += scan_source(path, &source, &mut findings);
    }

    if std::env::var("DRIFT_GODOT_STATS").is_ok() {
        eprintln!(
            "stats: {} .gd files scanned, {total_parse_errors} parse errors, {} findings",
            files.len(),
            findings.len()
        );
    }

    findings.sort_by(|a, b| {
        (&a.file, a.line, a.column, a.rule).cmp(&(&b.file, b.line, b.column, b.rule))
    });
    for f in &findings {
        println!(
            "{}:{}:{}: warning: {} [drift-godot::{}]",
            f.file.display(),
            f.line,
            f.column,
            f.message,
            f.rule
        );
    }

    Ok(if findings.is_empty() { 0 } else { 1 })
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        anyhow::bail!(
            "usage: drift-godot-lint <file.gd | project directory>\n\
             unseeded_rng and wallclock_read always run; both are the only \
             rules built so far (see docs/rule-catalog.md)"
        );
    }
    let code = run(&PathBuf::from(&args[1]))?;
    std::process::exit(code);
}
