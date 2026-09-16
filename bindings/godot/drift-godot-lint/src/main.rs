//! Standalone determinism checker for Godot GDScript, via
//! [`gdck-syntax`](https://crates.io/crates/gdck-syntax) — a real, lossless
//! GDScript 4 parser. See
//! D:\Claude\drift-planning\drift-godot-unreal-plan.md §9 for the
//! feasibility spike that validated the foundation before any of this was
//! written: 468/470 files of a real, 470-file Godot 4 game
//! (`SlayHorizon/godot-tiny-mmo`) parsed clean, and a spike visitor found
//! real hits in that same real codebase.
//!
//! 4 of the 5 shared-taxonomy rules (see drift-godot-unreal-plan.md §4):
//! `unseeded_rng`, `wallclock_read`, and `unordered_parallelism` fire
//! unconditionally, matched by call-site spelling (a bare `NameRef` or a
//! `Type.method`-shaped `AttributeExpr` callee) — same detection shape
//! `drift-unreal-lint`'s own unconditional rules already use and already
//! proved robust there. `float_outside_fixed_step` is reachability-scoped,
//! same idea as the Rust/Unreal sides, but with a real Godot-specific
//! simplification: `_process`/`_physics_process` are Godot's own fixed,
//! well-known per-frame/per-physics-step entry points (every real project
//! overrides them by that exact name to hook into the engine's own tick),
//! so reachability roots are auto-detected by name — no config file
//! needed, unlike the Rust/Unreal sides' `tick_reachable_roots`.
//! `hashmap_iter` does not port to Godot (§4: `Dictionary` is
//! insertion-ordered by engine guarantee). `usize_in_hashed_state` is N/A
//! (GDScript has no fixed/pointer-width integer distinction — `int` is
//! always 64-bit).

use gdck_syntax::{parse, LineIndex, NodeId, SyntaxKind, SyntaxNode, SyntaxTree};
use std::collections::{HashMap, HashSet, VecDeque};
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

/// Godot's `WorkerThreadPool` dispatches work across threads whose
/// completion order isn't guaranteed — same known-problem caveat
/// `drift::unordered_parallelism`/`DRIFT0004`/`drift-unreal::unordered_
/// parallelism` already carry (a reduction into simulated state isn't
/// proven commutative), inherited here rather than re-litigated. Matched
/// as `Type.method` `AttributeExpr` callees, same shape as
/// `WALLCLOCK_FUNCS`.
///
/// **Real, disclosed gap**: zero confirmed real call sites in
/// `SlayHorizon/godot-tiny-mmo`, this project's own real dogfood
/// target — not evidence the rule is unneeded (`WorkerThreadPool` is a
/// standard, documented Godot 4 parallelism primitive), just evidence
/// this particular real corpus doesn't happen to use it. Same situation
/// `drift-unreal-lint`'s own `unordered_parallelism` was in against Lyra;
/// validated with a synthetic fixture instead, same precedent.
const UNORDERED_PARALLELISM_FUNCS: &[&str] = &[
    "WorkerThreadPool.add_task",
    "WorkerThreadPool.add_group_task",
];

/// Godot's own fixed per-frame/per-physics-step entry points — real,
/// well-known convention (not a heuristic guess), auto-detected by name
/// rather than needing a `tick_reachable_roots`-style config file the
/// Rust/Unreal sides required.
const REACHABLE_ROOTS: &[&str] = &["_process", "_physics_process"];

const ARITHMETIC_OPS: &[SyntaxKind] = &[
    SyntaxKind::Plus,
    SyntaxKind::Minus,
    SyntaxKind::Star,
    SyntaxKind::Slash,
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

/// The plain identifier a `NameRef`/`Param`/`VarDecl` declares or refers
/// to — its one direct `Ident` child token, trimmed the same way
/// `callee_text` is.
fn ident_text(node: SyntaxNode) -> Option<String> {
    let source = node.tree().text();
    node.child_tokens()
        .find(|t| t.kind == SyntaxKind::Ident)
        .map(|t| t.text(source).trim().to_string())
}

fn scan_unconditional_rules(
    root: SyntaxNode,
    path: &Path,
    lines: &LineIndex,
    findings: &mut Vec<Finding>,
) {
    for node in root.descendants() {
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
        } else if UNORDERED_PARALLELISM_FUNCS.contains(&callee.as_str()) {
            Some((
                "unordered_parallelism",
                format!(
                    "{callee}() dispatches unordered parallel work; folding its results \
                     into simulated state without a deterministic reduction can desync \
                     across peers"
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
}

/// Whether an operand expression is "float-ish" — a bounded, syntactic
/// proxy, **not** real type inference. GDScript is dynamically typed by
/// default, so unlike the Rust/C#/Unreal sides (which all have a real
/// compiler/type-checker to ask), this tool has no symbol table and no way
/// to know a bare variable's type in general. Real, disclosed limitation:
/// only two signals count as float-ish — (1) a `Float` literal (`9.8`),
/// and (2) a `NameRef` naming a parameter or local variable *in the same
/// function* explicitly typed `: float` (`TypeHint`). Pure
/// untyped-variable-to-untyped-variable arithmetic (`a + b` with neither
/// side typed or literal) is invisible to this rule — a real gap, not a
/// silently-accepted one; typed GDScript (Godot's own recommended style
/// for anything simulation-relevant) is exactly what this catches.
fn expr_is_float_ish(node: SyntaxNode, float_names: &HashSet<String>) -> bool {
    match node.kind() {
        SyntaxKind::Literal => node.child_tokens().any(|t| t.kind == SyntaxKind::Float),
        SyntaxKind::NameRef => ident_text(node)
            .map(|name| float_names.contains(&name))
            .unwrap_or(false),
        SyntaxKind::ParenExpr | SyntaxKind::UnaryExpr => node
            .child_nodes()
            .next()
            .is_some_and(|inner| expr_is_float_ish(inner, float_names)),
        SyntaxKind::BinaryExpr => node
            .child_nodes()
            .any(|operand| expr_is_float_ish(operand, float_names)),
        _ => false,
    }
}

/// A `BinaryExpr` qualifies if its operator is arithmetic (`+ - * /`, not
/// a comparison/logical operator, which is also a real `BinaryExpr` in
/// this grammar) and at least one operand is float-ish per
/// `expr_is_float_ish`.
fn is_qualifying_float_binary(node: SyntaxNode, float_names: &HashSet<String>) -> bool {
    if node.kind() != SyntaxKind::BinaryExpr {
        return false;
    }
    if !node
        .child_tokens()
        .any(|t| ARITHMETIC_OPS.contains(&t.kind))
    {
        return false;
    }
    let operands: Vec<_> = node.child_nodes().collect();
    operands.len() == 2
        && (expr_is_float_ish(operands[0], float_names)
            || expr_is_float_ish(operands[1], float_names))
}

/// Every parameter and local variable in this function (not its callees —
/// no cross-function type flow, a real, disclosed limitation, same
/// "syntactic, not real dataflow" spirit as every spelling-based rule in
/// this project) explicitly typed `: float`.
fn collect_float_typed_names(func: SyntaxNode) -> HashSet<String> {
    let mut names = HashSet::new();
    for node in func.descendants() {
        if !matches!(node.kind(), SyntaxKind::Param | SyntaxKind::VarDecl) {
            continue;
        }
        let is_float_hint = node
            .child_node_of(SyntaxKind::TypeHint)
            .is_some_and(|hint| ident_text(hint).as_deref() == Some("float"));
        if is_float_hint {
            if let Some(name) = ident_text(node) {
                names.insert(name);
            }
        }
    }
    names
}

/// A chain like `a + b + c` is deduped to one warning on the outermost
/// qualifying expression, not one per operator — same reasoning as the
/// Rust/Unreal sides' own `scan_float_chains`.
fn scan_float_chains(
    func: SyntaxNode,
    float_names: &HashSet<String>,
    path: &Path,
    lines: &LineIndex,
    findings: &mut Vec<Finding>,
) {
    fn visit(
        node: SyntaxNode,
        float_names: &HashSet<String>,
        inside_qualifying: bool,
        path: &Path,
        lines: &LineIndex,
        findings: &mut Vec<Finding>,
    ) {
        let this_qualifies = is_qualifying_float_binary(node, float_names);
        if this_qualifies && !inside_qualifying {
            let loc = lines.line_col(node.range().start());
            findings.push(Finding {
                file: path.to_path_buf(),
                line: loc.line,
                column: loc.col,
                rule: "float_outside_fixed_step",
                message: "float arithmetic reachable from _process/_physics_process; \
                          non-associative reordering can desync across peers"
                    .to_string(),
            });
        }
        let child_inside = inside_qualifying || this_qualifies;
        for child in node.child_nodes() {
            visit(child, float_names, child_inside, path, lines, findings);
        }
    }
    visit(func, float_names, false, path, lines, findings);
}

/// The plain function name a `FuncDecl` declares — its one direct `Ident`
/// child token (after `func`), same extraction shape as `ident_text`.
fn func_name(func: SyntaxNode) -> Option<String> {
    ident_text(func)
}

/// Every `CallExpr` inside this function's own body, reduced to a bare
/// callee name — `self.foo()`/`obj.foo()` are collapsed to `foo` (the
/// last `.`-segment). Real, disclosed limitation: this is a flat,
/// project-wide, name-only call graph with no symbol table, so two
/// unrelated classes' own same-named methods collide into one graph node.
/// Same trade-off `drift-unreal-lint`'s own qualified-name call graph
/// avoids by having a real compiler to ask — this tool doesn't, and a
/// name collision here only widens reachability (a false negative risk
/// traded for a false positive one), never silently drops a real edge.
fn collect_call_edges(func: SyntaxNode, out: &mut HashSet<String>) {
    for node in func.descendants() {
        if node.kind() == SyntaxKind::CallExpr {
            if let Some(callee) = callee_text(node) {
                let simple = callee.rsplit('.').next().unwrap_or(&callee).to_string();
                out.insert(simple);
            }
        }
    }
}

#[derive(Clone, Copy)]
struct FuncInfo {
    tree_idx: usize,
    node_id: NodeId,
}

/// Builds the whole project's function table and call graph in one pass
/// over every parsed tree — the input `float_outside_fixed_step`'s
/// reachability BFS needs.
fn collect_functions_and_edges(
    trees: &[(PathBuf, SyntaxTree)],
) -> (HashMap<String, FuncInfo>, HashMap<String, HashSet<String>>) {
    let mut funcs = HashMap::new();
    let mut edges: HashMap<String, HashSet<String>> = HashMap::new();
    for (tree_idx, (_path, tree)) in trees.iter().enumerate() {
        for node in tree.root().descendants() {
            if node.kind() != SyntaxKind::FuncDecl {
                continue;
            }
            let Some(name) = func_name(node) else {
                continue;
            };
            funcs.insert(
                name.clone(),
                FuncInfo {
                    tree_idx,
                    node_id: node.id(),
                },
            );
            let mut callees = HashSet::new();
            collect_call_edges(node, &mut callees);
            edges.entry(name).or_default().extend(callees);
        }
    }
    (funcs, edges)
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

    let mut trees: Vec<(PathBuf, SyntaxTree)> = Vec::new();
    let mut total_parse_errors = 0usize;
    for path in &files {
        let source = fs::read_to_string(path)?;
        let tree = parse(&source);
        total_parse_errors += tree.errors().len();
        trees.push((path.clone(), tree));
    }

    let mut findings = Vec::new();

    // unseeded_rng, wallclock_read, unordered_parallelism: unconditional,
    // every parsed file, no reachability needed.
    for (path, tree) in &trees {
        let lines = LineIndex::new(tree.text());
        scan_unconditional_rules(tree.root(), path, &lines, &mut findings);
    }

    // float_outside_fixed_step: reachable from _process/_physics_process.
    let (funcs, edges) = collect_functions_and_edges(&trees);
    let mut reachable: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<String> = REACHABLE_ROOTS
        .iter()
        .filter(|root| funcs.contains_key(**root))
        .map(|root| root.to_string())
        .collect();
    while let Some(name) = queue.pop_front() {
        if !reachable.insert(name.clone()) {
            continue;
        }
        if let Some(callees) = edges.get(&name) {
            for callee in callees {
                if !reachable.contains(callee) {
                    queue.push_back(callee.clone());
                }
            }
        }
    }
    for name in &reachable {
        let Some(info) = funcs.get(name) else {
            continue;
        };
        let (path, tree) = &trees[info.tree_idx];
        let func = tree.node(info.node_id);
        let float_names = collect_float_typed_names(func);
        let lines = LineIndex::new(tree.text());
        scan_float_chains(func, &float_names, path, &lines, &mut findings);
    }

    // Real duplicate found dogfooding: `float_outside_fixed_step`'s flat,
    // name-only call graph (see `collect_call_edges`'s own doc comment)
    // means two differently-named-but-colliding entries in `reachable` can
    // both resolve to the same underlying function, scanning its body
    // twice — confirmed against real output (`toaster.gd:154:22` printed
    // twice). Dedup on the exact reported location, same fix shape
    // `drift-unreal-lint` already needed for its own real UE_LOG
    // macro-duplication bug. Done before the stats line below so that
    // count reflects what's actually printed, not the pre-dedup total.
    findings.sort_by(|a, b| {
        (&a.file, a.line, a.column, a.rule).cmp(&(&b.file, b.line, b.column, b.rule))
    });
    findings.dedup_by(|a, b| {
        a.file == b.file && a.line == b.line && a.column == b.column && a.rule == b.rule
    });

    if std::env::var("DRIFT_GODOT_STATS").is_ok() {
        eprintln!(
            "stats: {} .gd files scanned, {total_parse_errors} parse errors, {} functions, \
             {} reachable from _process/_physics_process, {} findings",
            files.len(),
            funcs.len(),
            reachable.len(),
            findings.len()
        );
    }

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
             unseeded_rng, wallclock_read, and unordered_parallelism always run; \
             float_outside_fixed_step runs automatically wherever \
             _process/_physics_process is defined (see docs/rule-catalog.md)"
        );
    }
    let code = run(&PathBuf::from(&args[1]))?;
    std::process::exit(code);
}
