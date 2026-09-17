//! Standalone determinism checker for Unreal C++, stock Clang/libclang — no
//! engine fork, no custom clang-tidy check compiled into LLVM (the winget
//! LLVM package ships `clang-c` + `libclang.lib` only, not the full
//! LibTooling/AST headers a real clang-tidy check needs to build against).
//!
//! All 5 rules from the Rust/C# taxonomy, ported (see
//! D:\Claude\drift-planning\drift-godot-unreal-plan.md §4/§6/§8):
//! `unseeded_rng`, `wallclock_read`, `hashmap_iter`,
//! `unordered_parallelism`, and `usize_in_hashed_state` fire
//! unconditionally, repo-wide, same as their Rust/C# counterparts.
//! `float_outside_fixed_step` is opt-in only: does nothing unless
//! `tick_reachable_roots` is configured, same design as drift-lint's own
//! dylint.toml-driven reachability scoping. `hashmap_iter` is detected by
//! type (a range-based-for or `.CreateIterator()`/`.CreateConstIterator()`
//! over a `TMap`/`TSet`), not by call-site spelling — the type-based
//! approach naturally excludes the collect-then-sort false-positive shape
//! the Rust side's own `hashmap_iter` had to special-case, since a
//! `TArray` sorted after being built from a map's contents is a different
//! type than `TMap`/`TSet`.

use anyhow::{bail, Context, Result};
use clang::{Clang, Entity, EntityKind, Index};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

#[derive(Debug, Default, Deserialize)]
struct Config {
    #[serde(default)]
    tick_reachable_roots: Vec<String>,
    #[serde(default)]
    fixed_step_functions: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CompileCommand {
    directory: String,
    file: String,
    #[serde(default)]
    arguments: Option<Vec<String>>,
    #[serde(default)]
    command: Option<String>,
}

fn split_command(cmd: &str) -> Vec<String> {
    // Compile commands here come from UnrealBuildTool's own
    // GenerateClangDatabase, which already shell-quotes with plain
    // double quotes and no embedded escapes in practice — a real,
    // minimal splitter is enough; not a general shell parser.
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for c in cmd.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !cur.is_empty() {
                    args.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        args.push(cur);
    }
    args
}

/// Recursively expands `@response-file` arguments — real UnrealBuildTool
/// `compile_commands.json` entries for a Development/Editor target are
/// just `clang-cl.exe @foo.rsp`, and `foo.rsp` itself nests a second
/// `@FooShared.rsp` holding the actual include/define flags. libclang
/// can't expand these itself, so this tool has to.
fn expand_response_files(args: Vec<String>, depth: u32) -> Vec<String> {
    if depth > 8 {
        return args; // real UBT nests two deep; a cap just guards a cycle.
    }
    let mut out = Vec::new();
    for arg in args {
        if let Some(path) = arg.strip_prefix('@') {
            match std::fs::read_to_string(path) {
                Ok(text) => out.extend(expand_response_files(split_command(&text), depth + 1)),
                Err(_) => out.push(arg),
            }
        } else {
            out.push(arg);
        }
    }
    out
}

fn qualified_name(entity: &Entity) -> String {
    let mut parts = vec![entity.get_name().unwrap_or_else(|| "<anon>".into())];
    let mut cur = entity.get_semantic_parent();
    while let Some(p) = cur {
        match p.get_kind() {
            EntityKind::ClassDecl
            | EntityKind::StructDecl
            | EntityKind::Namespace
            | EntityKind::ClassTemplate => {
                if let Some(name) = p.get_name() {
                    parts.push(name);
                }
                cur = p.get_semantic_parent();
            }
            _ => break,
        }
    }
    parts.reverse();
    parts.join("::")
}

fn is_function_like(kind: EntityKind) -> bool {
    matches!(
        kind,
        EntityKind::FunctionDecl
            | EntityKind::Method
            | EntityKind::Constructor
            | EntityKind::Destructor
            | EntityKind::FunctionTemplate
    )
}

/// Operator token spelling for a binary-operator cursor: libclang's C API
/// doesn't expose the operator kind directly, only via tokenizing the
/// cursor's own source range and taking the token that falls between the
/// two operand sub-ranges.
fn binary_operator_spelling(entity: &Entity) -> Option<String> {
    let children: Vec<_> = entity.get_children();
    if children.len() != 2 {
        return None;
    }
    let lhs_end = children[0]
        .get_range()?
        .get_end()
        .get_spelling_location()
        .offset;
    let rhs_start = children[1]
        .get_range()?
        .get_start()
        .get_spelling_location()
        .offset;
    let range = entity.get_range()?;
    for token in range.tokenize() {
        let loc = token.get_range().get_start().get_spelling_location().offset;
        if loc >= lhs_end && loc < rhs_start {
            return Some(token.get_spelling());
        }
    }
    None
}

const ARITHMETIC_OPS: &[&str] = &["+", "-", "*", "/"];
/// Compound-assignment counterparts (`x += y`) — a real gap found
/// dogfooding drift-godot-lint against a real project
/// (Orama-Interactive/Pixelorama's own `_process` accumulating a timer via
/// `+=`): the original `EntityKind::BinaryOperator`-only scan below never
/// visited these, since libclang represents `+=`/`-=`/`*=`/`/=` as a
/// distinct `EntityKind::CompoundAssignOperator` node, not a
/// `BinaryOperator`. Confirmed against the `clang` crate's own EntityKind
/// docs, not assumed.
const COMPOUND_ARITHMETIC_OPS: &[&str] = &["+=", "-=", "*=", "/="];

fn is_float_type(entity: &Entity) -> bool {
    entity
        .get_type()
        .map(|t| {
            let spelling = t.get_display_name();
            spelling == "float" || spelling == "double"
        })
        .unwrap_or(false)
}

#[derive(Serialize, Deserialize)]
struct Finding {
    file: PathBuf,
    line: u32,
    column: u32,
    rule: String,
    message: String,
}

/// One worker process's complete output for a single translation unit —
/// crossing the process boundary as JSON is what makes a crash in one TU
/// (real, confirmed: a full 388-file Lyra sweep hit a genuine libclang
/// `LLVM ERROR: out of memory` that took the whole single-process run down
/// with it, per docs/rule-catalog.md's "real, disclosed cost") harmless to
/// every other TU instead of losing the entire run's findings.
///
/// `float_candidates` intentionally ignores reachability — a worker only
/// sees its own TU, not the whole program's call graph, so it can't know
/// which of its own functions are tick-reachable. It scans every function
/// body unconditionally and tags each finding with its owning function's
/// qualified name; the orchestrator (the only place with the full,
/// cross-TU call graph) filters by reachability after aggregating every
/// worker's edges.
#[derive(Serialize, Deserialize, Default)]
struct WorkerOutput {
    findings: Vec<Finding>,
    edges: Vec<(String, String)>,
    float_candidates: Vec<(String, Finding)>,
}

fn scan_float_chains(body: Entity, findings: &mut Vec<Finding>) {
    fn visit(entity: Entity, inside_qualifying_parent: bool, findings: &mut Vec<Finding>) {
        let mut this_qualifies = false;
        let kind = entity.get_kind();
        if (kind == EntityKind::BinaryOperator || kind == EntityKind::CompoundAssignOperator)
            && is_float_type(&entity)
        {
            if let Some(op) = binary_operator_spelling(&entity) {
                if ARITHMETIC_OPS.contains(&op.as_str())
                    || COMPOUND_ARITHMETIC_OPS.contains(&op.as_str())
                {
                    this_qualifies = true;
                }
            }
        }

        if this_qualifies && !inside_qualifying_parent {
            if let Some(range) = entity.get_range() {
                let loc = range.get_start().get_spelling_location();
                findings.push(Finding {
                    file: loc.file.map(|f| f.get_path()).unwrap_or_default(),
                    line: loc.line,
                    column: loc.column,
                    rule: "float_outside_fixed_step".to_string(),
                    message: "float arithmetic reachable from tick-reachable code; \
                              non-associative reordering can desync across platforms"
                        .to_string(),
                });
            }
        }

        let child_inside_qualifying = inside_qualifying_parent || this_qualifies;
        for child in entity.get_children() {
            visit(child, child_inside_qualifying, findings);
        }
    }
    visit(body, false, findings);
}

/// Unreal's global RNG (`FMath::Rand`/`FRand`/etc., all backed by the same
/// process-global `appRand`-style state) is seeded from OS/engine entropy
/// unless a project explicitly calls `FMath::RandInit`/`SRandInit` with a
/// tracked value — same hazard as Rust's `rand::thread_rng()`. Fires
/// unconditionally, no reachability scoping needed: unlike float
/// arithmetic, a raw call to one of these is *always* worth flagging, not
/// just inside simulation code — mirrors `drift::unseeded_rng`'s own
/// unconditional behavior on the Rust side.
///
/// Matched against the call's own source spelling, not the resolved
/// declaration's qualified name — a real bug hit building this: `FMath`
/// (`Engine/.../UnrealMathUtility.h`) is `struct FMath : public
/// FPlatformMath`, and `Rand`/`FRand` are actually declared on the base
/// (`FGenericPlatformMath` or a platform-specific typedef of it), so
/// `FMath::FRand()`'s resolved declaration has a *different* qualified
/// name than the class name written at the call site. Every real call in
/// Lyra's own source (`grep`-confirmed) is still spelled `FMath::...`, so
/// matching the literal call-site tokens sidesteps the inheritance chain
/// entirely instead of trying to enumerate every platform's base struct.
const UNSEEDED_RNG_FUNCS: &[&str] = &[
    "FMath::Rand",
    "FMath::RandRange",
    "FMath::RandHelper",
    "FMath::FRand",
    "FMath::FRandRange",
    "FMath::RandBool",
    "FMath::VRand",
    "FMath::VRandCone",
    "FMath::VRandCone2D",
];

/// The call's callee spelling exactly as written at the call site (e.g.
/// `"FMath::FRand"`), by tokenizing up to the opening `(` — see
/// `UNSEEDED_RNG_FUNCS` for why this is matched instead of the resolved
/// declaration's qualified name.
fn call_site_spelling(entity: &Entity) -> Option<String> {
    let range = entity.get_range()?;
    let mut spelling = String::new();
    for token in range.tokenize() {
        let text = token.get_spelling();
        if text == "(" {
            break;
        }
        spelling.push_str(&text);
    }
    Some(spelling)
}

fn scan_unseeded_rng(tu_root: Entity, findings: &mut Vec<Finding>) {
    fn visit(entity: Entity, findings: &mut Vec<Finding>) {
        if entity.get_kind() == EntityKind::CallExpr {
            if let Some(spelling) = call_site_spelling(&entity) {
                if UNSEEDED_RNG_FUNCS.contains(&spelling.as_str()) {
                    if let Some(range) = entity.get_range() {
                        let loc = range.get_start().get_spelling_location();
                        findings.push(Finding {
                            file: loc.file.map(|f| f.get_path()).unwrap_or_default(),
                            line: loc.line,
                            column: loc.column,
                            rule: "unseeded_rng".to_string(),
                            message: format!(
                                "{spelling}() reads Unreal's global RNG, which is not seeded \
                                 deterministically by default; differs per peer/run"
                            ),
                        });
                    }
                }
            }
        }
        for child in entity.get_children() {
            visit(child, findings);
        }
    }
    visit(tu_root, findings);
}

/// Wallclock/OS-time reads — same hazard as Rust's `wallclock_read`/C#'s
/// DRIFT0003: a value that differs per peer/run must never feed simulated
/// state directly. Fires unconditionally, no reachability scoping, same as
/// `unseeded_rng`.
///
/// Matched by call-site spelling, not resolved declaration, same reasoning
/// as `unseeded_rng` — verified here rather than assumed: `FPlatformTime`
/// (`HAL/PlatformTime.h`) is a platform `typedef` (e.g. `typedef
/// FWindowsPlatformTime FPlatformTime;` on Windows), and
/// `FWindowsPlatformTime::Seconds/Cycles/Cycles64` are declared directly on
/// that platform struct, not inherited from `FGenericPlatformTime` — so a
/// call written as `FPlatformTime::Seconds()` resolves to
/// `FWindowsPlatformTime::Seconds`, a different qualified name than the
/// call-site text, same shape as `FMath`'s own base-class split.
/// `FDateTime::Now`/`UtcNow` don't have this split (`FDateTime` is a plain
/// struct declaring them directly), but are matched the same way for
/// consistency and because it's already proven robust.
const WALLCLOCK_READ_FUNCS: &[&str] = &[
    "FPlatformTime::Seconds",
    "FPlatformTime::Cycles",
    "FPlatformTime::Cycles64",
    "FDateTime::Now",
    "FDateTime::UtcNow",
];

/// `true` if a type's display name (as libclang prints it, e.g. `"const
/// TMap<FGameplayTag, FActiveGamePhaseEntry, ...> &"`) is a `TMap`/`TSet` —
/// hash-backed containers (`Containers/Map.h`/`Set.h`, confirmed by
/// reading the real headers: both select between `TSparseSet`/`TCompactSet`
/// internally), unlike `TSortedMap`/`TSortedSet`'s own existence being
/// itself evidence the base containers carry no ordering guarantee.
/// Deliberately excludes `TSortedMap`/`TSortedSet`/`TMultiMap` — scope
/// matches the plan's own taxonomy entry, not every hash-adjacent
/// container.
fn is_map_or_set_type(display_name: &str) -> bool {
    let trimmed = display_name.trim_start_matches("const ").trim();
    trimmed.starts_with("TMap<") || trimmed.starts_with("TSet<")
}

/// Range-based-for detection: `ForRangeStmt`'s own compiler-desugared
/// children (confirmed with a real `-ast-dump`, not assumed) are a `NULL`
/// placeholder followed by a `DeclStmt` wrapping the synthesized `auto&&
/// __range = <container>;` binding — the binding's `VarDecl` (one level
/// below the `DeclStmt`, not a direct child of the `ForRangeStmt` itself)
/// has the container's own type, so both levels are checked.
fn for_range_over_map_or_set(entity: &Entity) -> bool {
    entity.get_children().iter().any(|child| {
        let self_match = child
            .get_type()
            .map(|t| is_map_or_set_type(&t.get_display_name()))
            .unwrap_or(false);
        self_match
            || child.get_children().iter().any(|grandchild| {
                grandchild
                    .get_type()
                    .map(|t| is_map_or_set_type(&t.get_display_name()))
                    .unwrap_or(false)
            })
    })
}

/// `.CreateIterator()`/`.CreateConstIterator()` on a `TMap`/`TSet` — the
/// explicit-iterator counterpart to range-based-for iteration, same
/// hazard. A `CallExpr`'s own first child is the `MemberRefExpr` for a
/// method call (`Map.CreateIterator()`), whose own first child is the
/// receiver expression.
fn is_map_or_set_create_iterator_call(entity: &Entity) -> bool {
    if entity.get_kind() != EntityKind::CallExpr {
        return false;
    }
    let Some(member_ref) = entity.get_children().into_iter().next() else {
        return false;
    };
    if member_ref.get_kind() != EntityKind::MemberRefExpr {
        return false;
    }
    let Some(name) = member_ref.get_name() else {
        return false;
    };
    if name != "CreateIterator" && name != "CreateConstIterator" {
        return false;
    }
    member_ref
        .get_children()
        .into_iter()
        .next()
        .and_then(|receiver| receiver.get_type())
        .map(|t| is_map_or_set_type(&t.get_display_name()))
        .unwrap_or(false)
}

fn scan_hashmap_iter(tu_root: Entity, findings: &mut Vec<Finding>) {
    fn visit(entity: Entity, findings: &mut Vec<Finding>) {
        let hit = match entity.get_kind() {
            EntityKind::ForRangeStmt => for_range_over_map_or_set(&entity),
            EntityKind::CallExpr => is_map_or_set_create_iterator_call(&entity),
            _ => false,
        };
        if hit {
            if let Some(range) = entity.get_range() {
                let loc = range.get_start().get_spelling_location();
                findings.push(Finding {
                    file: loc.file.map(|f| f.get_path()).unwrap_or_default(),
                    line: loc.line,
                    column: loc.column,
                    rule: "hashmap_iter".to_string(),
                    message: "iterating a TMap/TSet — order is not guaranteed stable across \
                              peers"
                        .to_string(),
                });
            }
        }
        for child in entity.get_children() {
            visit(child, findings);
        }
    }
    visit(tu_root, findings);
}

fn scan_wallclock_read(tu_root: Entity, findings: &mut Vec<Finding>) {
    fn visit(entity: Entity, findings: &mut Vec<Finding>) {
        if entity.get_kind() == EntityKind::CallExpr {
            if let Some(spelling) = call_site_spelling(&entity) {
                if WALLCLOCK_READ_FUNCS.contains(&spelling.as_str()) {
                    if let Some(range) = entity.get_range() {
                        let loc = range.get_start().get_spelling_location();
                        findings.push(Finding {
                            file: loc.file.map(|f| f.get_path()).unwrap_or_default(),
                            line: loc.line,
                            column: loc.column,
                            rule: "wallclock_read".to_string(),
                            message: format!(
                                "{spelling}() reads wallclock/OS time, which differs per \
                                 peer/run; do not fold it into simulated state"
                            ),
                        });
                    }
                }
            }
        }
        for child in entity.get_children() {
            visit(child, findings);
        }
    }
    visit(tu_root, findings);
}

/// Parallel iteration/dispatch whose reduction into shared state isn't
/// proven commutative — same known-problem caveat `drift::unordered_
/// parallelism`/`DRIFT0004` already carry, inherited here rather than
/// re-litigated. Fires unconditionally, no reachability scoping.
///
/// Deliberately excludes `AsyncTask` — checked against real non-Lyra
/// Engine source before deciding, not assumed: every real call site found
/// (`AndroidPlatformMemory.cpp`, `ConfigContext.cpp`,
/// `IPlatformFileManagedStorageWrapper.h`) was a memory warning,
/// deprecation message, or background file operation, none touching
/// simulated state — including it would flag far more UI/logging/IO code
/// than real hazards, a worse signal-to-noise ratio than `ParallelFor`.
/// Deferred the same way `float_outside_fixed_step` was originally
/// deferred: real evidence against inclusion, not a guess.
const UNORDERED_PARALLELISM_FUNCS: &[&str] = &[
    "ParallelFor",
    "ParallelForWithTaskContext",
    "UE::Tasks::Launch",
];

fn scan_unordered_parallelism(tu_root: Entity, findings: &mut Vec<Finding>) {
    fn visit(entity: Entity, findings: &mut Vec<Finding>) {
        if entity.get_kind() == EntityKind::CallExpr {
            if let Some(spelling) = call_site_spelling(&entity) {
                if UNORDERED_PARALLELISM_FUNCS.contains(&spelling.as_str()) {
                    if let Some(range) = entity.get_range() {
                        let loc = range.get_start().get_spelling_location();
                        findings.push(Finding {
                            file: loc.file.map(|f| f.get_path()).unwrap_or_default(),
                            line: loc.line,
                            column: loc.column,
                            rule: "unordered_parallelism".to_string(),
                            message: format!(
                                "{spelling}() dispatches unordered parallel work; folding \
                                 its results into simulated state without a deterministic \
                                 reduction can desync across peers"
                            ),
                        });
                    }
                }
            }
        }
        for child in entity.get_children() {
            visit(child, findings);
        }
    }
    visit(tu_root, findings);
}

/// `SIZE_T`/`size_t`/`uintptr_t`/`intptr_t` are pointer-width — 32-bit on
/// Win32, 64-bit on Win64/most platforms, exactly the Rust `usize`/C#
/// `nint` hazard `drift::usize_in_hashed_state`/`DRIFT0005` already flag.
/// Matched by spelling, same reasoning as everywhere else in this tool:
/// syntactic, not a full platform-ABI resolution.
fn is_pointer_width_type(display_name: &str) -> bool {
    matches!(display_name, "SIZE_T" | "size_t" | "uintptr_t" | "intptr_t")
}

/// Every pointer-width-typed field, keyed by its containing struct/class's
/// own qualified name — the lookup `scan_usize_in_hashed_state` uses to
/// check a `GetTypeHash` overload's parameter type against.
fn collect_pointer_width_fields(tu_root: Entity) -> HashMap<String, HashSet<String>> {
    fn visit(entity: Entity, map: &mut HashMap<String, HashSet<String>>) {
        if entity.get_kind() == EntityKind::FieldDecl {
            if let (Some(t), Some(parent), Some(field_name)) = (
                entity.get_type(),
                entity.get_semantic_parent(),
                entity.get_name(),
            ) {
                if is_pointer_width_type(&t.get_display_name()) {
                    map.entry(qualified_name(&parent))
                        .or_default()
                        .insert(field_name);
                }
            }
        }
        for child in entity.get_children() {
            visit(child, map);
        }
    }
    let mut map = HashMap::new();
    visit(tu_root, &mut map);
    map
}

/// Unreal has no `#[derive(Hash)]`/`record` equivalent — hashing is always
/// a hand-written free function `GetTypeHash(const T&)` found via
/// Argument-Dependent Lookup, so this mirrors C#'s `DRIFT0005` hand-
/// written-`GetHashCode()` path more than the Rust side's own struct-
/// derive-attribute scan: (1) find pointer-width-typed fields, (2) check
/// whether they're referenced anywhere in the body of a `GetTypeHash`
/// overload taking that field's containing type by const-ref — same
/// syntactic, conservative reference-detection (not real dataflow) C#'s
/// own version already uses: a field merely read, not actually folded
/// into the returned hash, still gets flagged, a known, documented
/// limitation inherited rather than re-solved here. Fires unconditionally,
/// no reachability scoping — a hash function's own body isn't a
/// per-frame-tick concern the way arithmetic reachability is.
fn scan_usize_in_hashed_state(tu_root: Entity, findings: &mut Vec<Finding>) {
    let fields_by_struct = collect_pointer_width_fields(tu_root);
    if fields_by_struct.is_empty() {
        return;
    }

    fn param_struct_name(param: &Entity) -> Option<String> {
        let ty = param.get_type()?;
        let pointee = ty.get_pointee_type().unwrap_or(ty);
        let decl = pointee.get_declaration()?;
        Some(qualified_name(&decl))
    }

    fn visit(
        entity: Entity,
        fields_by_struct: &HashMap<String, HashSet<String>>,
        findings: &mut Vec<Finding>,
    ) {
        if entity.get_kind() == EntityKind::FunctionDecl
            && entity.is_definition()
            && entity.get_name().as_deref() == Some("GetTypeHash")
        {
            let params = entity.get_arguments().unwrap_or_default();
            if params.len() == 1 {
                if let Some(struct_name) = param_struct_name(&params[0]) {
                    if let Some(flagged_fields) = fields_by_struct.get(&struct_name) {
                        scan_body_for_flagged_fields(entity, flagged_fields, findings);
                    }
                }
            }
        }
        for child in entity.get_children() {
            visit(child, fields_by_struct, findings);
        }
    }

    fn scan_body_for_flagged_fields(
        entity: Entity,
        flagged_fields: &HashSet<String>,
        findings: &mut Vec<Finding>,
    ) {
        if entity.get_kind() == EntityKind::MemberRefExpr {
            if let Some(name) = entity.get_name() {
                if flagged_fields.contains(&name) {
                    if let Some(range) = entity.get_range() {
                        let loc = range.get_start().get_spelling_location();
                        findings.push(Finding {
                            file: loc.file.map(|f| f.get_path()).unwrap_or_default(),
                            line: loc.line,
                            column: loc.column,
                            rule: "usize_in_hashed_state".to_string(),
                            message: format!(
                                "'{name}' is SIZE_T/uintptr_t/intptr_t and used in a \
                                 GetTypeHash overload — width varies across platforms"
                            ),
                        });
                    }
                }
            }
        }
        for child in entity.get_children() {
            scan_body_for_flagged_fields(child, flagged_fields, findings);
        }
    }

    visit(tu_root, &fields_by_struct, findings);
}

fn collect_call_edges(body: Entity, caller: &str, edges: &mut HashMap<String, HashSet<String>>) {
    fn visit(entity: Entity, caller: &str, edges: &mut HashMap<String, HashSet<String>>) {
        if entity.get_kind() == EntityKind::CallExpr {
            if let Some(referenced) = entity.get_reference() {
                let callee = qualified_name(&referenced);
                edges.entry(caller.to_string()).or_default().insert(callee);
            }
        }
        for child in entity.get_children() {
            visit(child, caller, edges);
        }
    }
    visit(body, caller, edges);
}

fn load_config(config_path: Option<&Path>) -> Result<Config> {
    match config_path {
        Some(p) => {
            let text = std::fs::read_to_string(p)
                .with_context(|| format!("reading config {}", p.display()))?;
            Ok(toml::from_str(&text).with_context(|| format!("parsing config {}", p.display()))?)
        }
        None => Ok(Config::default()),
    }
}

fn load_commands(compile_commands_path: &Path) -> Result<Vec<CompileCommand>> {
    let text = std::fs::read_to_string(compile_commands_path)
        .with_context(|| format!("reading {}", compile_commands_path.display()))?;
    Ok(serde_json::from_str(&text)?)
}

/// Parses one `compile_commands.json` entry against a fresh `Index` —
/// exactly the per-command argument munging `run`'s own predecessor did
/// inline, unchanged, just factored out so a worker process can call it
/// for its single assigned command.
fn parse_one<'i>(index: &'i Index, cmd: &CompileCommand) -> Option<clang::TranslationUnit<'i>> {
    let mut args = cmd.arguments.clone().unwrap_or_else(|| {
        cmd.command
            .as_deref()
            .map(split_command)
            .unwrap_or_default()
    });
    if args.is_empty() {
        return None;
    }
    // The JSON Compilation Database spec requires relative paths in
    // `arguments`/`command` to resolve against `directory`, not the
    // caller's own cwd — real, confirmed the hard way: real UBT
    // response files use relative `-I../Plugins/...` include paths
    // meant to resolve against `directory` (here Engine/Source), and
    // without this every file transitively including a plugin header
    // hits a fatal "file not found" partway through, silently
    // truncating that TU's AST to whatever was parsed before the
    // failure — a real, large undercount, not just a cosmetic diag.
    let _ = std::env::set_current_dir(&cmd.directory);
    // args[0] is always the compiler executable per the JSON
    // Compilation Database spec — not a real compiler flag, drop it
    // like argv[0]. Real UBT invokes `clang-cl.exe`, whose MSVC-style
    // flags (`/FI`, `/Fo`, `/clang:...`) libclang's default
    // GCC-style argv parser won't understand — `--driver-mode=cl`
    // (the same thing baked into the `clang-cl` binary's own name)
    // switches it, checked here rather than assumed since this
    // tool's own fixture uses plain `clang++`-style args instead.
    let is_cl_driver = args[0].to_ascii_lowercase().contains("clang-cl");
    args.remove(0);
    args = expand_response_files(args, 0);
    // The source file is already passed separately as the parser's
    // own `file` argument below; passing it a second time inside
    // `arguments` — which a `.rsp` file's first line does — makes
    // `clang_parseTranslationUnit2` return `CXError_ASTReadError`
    // (code 4), not a normal parse failure. Confirmed the hard way.
    args.retain(|a| a != &cmd.file);
    if is_cl_driver {
        args.insert(0, "--driver-mode=cl".to_string());
    }
    // Real, confirmed: this LLVM install's own AVX512 intrinsic
    // headers (avx512fintrin.h etc., pulled in transitively once
    // real headers resolve) reference builtins this exact clang
    // frontend doesn't implement (e.g.
    // `__builtin_elementwise_fshr`) — a handful of real but
    // irrelevant errors, confined to system intrinsic headers, that
    // otherwise hit clang's default `-ferror-limit=20` and abort the
    // whole parse before ever reaching the target file's own code.
    // Disabling the limit is the standard fix for exactly this in
    // static-analysis tooling: keep going, collect the AST for
    // everything past the noise instead of giving up on it.
    args.push("-ferror-limit=0".to_string());

    index
        .parser(&cmd.file)
        .arguments(&args)
        .detailed_preprocessing_record(false)
        .parse()
        .ok()
}

/// Parses one command and runs every rule against just that TU — the
/// whole of one worker process's job. Unconditional rules go straight
/// into `findings`; `float_outside_fixed_step` candidates are collected
/// for *every* function definition in this TU, tagged by owning function,
/// regardless of reachability — this TU alone can't know what's
/// tick-reachable, only the orchestrator (with every worker's edges
/// aggregated) can, so the reachability filter happens there instead.
fn process_command(cmd: &CompileCommand, config: &Config) -> WorkerOutput {
    let mut out = WorkerOutput::default();

    let clang = match Clang::new() {
        Ok(c) => c,
        Err(_) => return out,
    };
    let index = Index::new(&clang, false, false);
    let Some(tu) = parse_one(&index, cmd) else {
        return out;
    };

    scan_unseeded_rng(tu.get_entity(), &mut out.findings);
    scan_wallclock_read(tu.get_entity(), &mut out.findings);
    scan_hashmap_iter(tu.get_entity(), &mut out.findings);
    scan_unordered_parallelism(tu.get_entity(), &mut out.findings);
    scan_usize_in_hashed_state(tu.get_entity(), &mut out.findings);

    if !config.tick_reachable_roots.is_empty() {
        let mut edges: HashMap<String, HashSet<String>> = HashMap::new();
        tu.get_entity().visit_children(|cur, _parent| {
            if is_function_like(cur.get_kind()) && cur.is_definition() {
                let qname = qualified_name(&cur);
                let mut chain_findings = Vec::new();
                scan_float_chains(cur, &mut chain_findings);
                for finding in chain_findings {
                    out.float_candidates.push((qname.clone(), finding));
                }
                collect_call_edges(cur, &qname, &mut edges);
            }
            clang::EntityVisitResult::Recurse
        });
        for (caller, callees) in edges {
            for callee in callees {
                out.edges.push((caller.clone(), callee));
            }
        }
    }

    out
}

/// Runs as a subprocess of `run`, one per `compile_commands.json` entry —
/// the crash-isolation boundary. A real full-388-file Lyra sweep hit a
/// genuine libclang `LLVM ERROR: out of memory` that took the whole
/// single-process run down with it via a segfault (`docs/rule-catalog.md`,
/// "real, disclosed cost") — this worker process can crash the same way,
/// but now it only takes its own single TU down with it; the orchestrator
/// (`run`) treats a failed/crashed worker as "no findings from this file"
/// and keeps going, instead of losing every finding already collected.
fn run_worker(
    index: usize,
    compile_commands_path: &Path,
    config_path: Option<&Path>,
) -> Result<()> {
    let config = load_config(config_path)?;
    let commands = load_commands(compile_commands_path)?;
    let Some(cmd) = commands.get(index) else {
        bail!(
            "worker index {index} out of range ({} commands)",
            commands.len()
        );
    };
    let output = process_command(cmd, &config);
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}

/// Number of worker processes to run concurrently — one OS process per
/// translation unit already isolates crashes (see `run_worker`'s own doc
/// comment); running several at once is purely a throughput win, bounded by
/// available parallelism so this doesn't oversubscribe the machine.
/// `DRIFT_UNREAL_JOBS` overrides it (e.g. to cap memory use on a machine
/// where each libclang parse is large — real cost seen dogfooding: a full
/// 388-file Lyra sweep peaks well above one TU's worth of memory per
/// concurrent worker).
fn worker_job_count() -> usize {
    std::env::var("DRIFT_UNREAL_JOBS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&n: &usize| n > 0)
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(4, |n| n.get()))
}

fn run(compile_commands_path: &Path, config_path: Option<&Path>) -> Result<i32> {
    let config = load_config(config_path)?;
    let commands = load_commands(compile_commands_path)?;
    let exe = std::env::current_exe().context("resolving own executable path")?;
    let stats = std::env::var("DRIFT_UNREAL_STATS").is_ok();

    let mut findings = Vec::new();
    let mut edges: HashMap<String, HashSet<String>> = HashMap::new();
    let mut float_candidates: Vec<(String, Finding)> = Vec::new();
    let mut worker_ok_count = 0usize;

    // Work-stealing pool: each thread pulls the next unclaimed command index
    // off a shared atomic counter and spawns+waits on that one worker
    // process, so a slow/large TU on one thread doesn't stall the others
    // the way a fixed static split would. Results are collected into a
    // per-index slot rather than a shared Vec so no per-item lock
    // contention is needed while workers are running.
    let next_index = AtomicUsize::new(0);
    let results: Vec<Mutex<Option<Option<WorkerOutput>>>> =
        (0..commands.len()).map(|_| Mutex::new(None)).collect();
    let job_count = worker_job_count().min(commands.len().max(1));

    std::thread::scope(|scope| {
        for _ in 0..job_count {
            scope.spawn(|| loop {
                let i = next_index.fetch_add(1, Ordering::Relaxed);
                if i >= commands.len() {
                    break;
                }
                let mut worker = Command::new(&exe);
                worker
                    .arg("--worker")
                    .arg(i.to_string())
                    .arg(compile_commands_path);
                if let Some(p) = config_path {
                    worker.arg(p);
                }
                let output = worker.output();
                let parsed = output.ok().and_then(|o| {
                    if o.status.success() {
                        serde_json::from_slice::<WorkerOutput>(&o.stdout).ok()
                    } else {
                        None
                    }
                });
                *results[i].lock().unwrap() = Some(parsed);
            });
        }
    });

    for (i, cmd) in commands.iter().enumerate() {
        match results[i].lock().unwrap().take().flatten() {
            Some(worker_output) => {
                worker_ok_count += 1;
                findings.extend(worker_output.findings);
                for (caller, callee) in worker_output.edges {
                    edges.entry(caller).or_default().insert(callee);
                }
                float_candidates.extend(worker_output.float_candidates);
            }
            None => {
                // A crashed or failed worker (real example: a genuine
                // libclang out-of-memory abort on one Lyra file, see
                // docs/rule-catalog.md) costs this one file's findings,
                // not the whole run's — reported, not silently dropped.
                eprintln!(
                    "warning: worker for {} failed or crashed; skipping",
                    cmd.file
                );
            }
        }
    }

    if stats {
        eprintln!(
            "stats: {worker_ok_count}/{} translation units parsed",
            commands.len()
        );
        eprintln!(
            "stats: {} call edges, {} float_outside_fixed_step candidates",
            edges.values().map(|v| v.len()).sum::<usize>(),
            float_candidates.len()
        );
    }

    // float_outside_fixed_step: opt-in only, same rationale as
    // drift-lint's dylint.toml-gated reachability rules — without
    // configured roots, blanket-flagging float arithmetic across a whole
    // UE codebase would be far too noisy to be useful.
    if !config.tick_reachable_roots.is_empty() {
        let roots: HashSet<String> = config.tick_reachable_roots.iter().cloned().collect();
        let exempt: HashSet<String> = config.fixed_step_functions.iter().cloned().collect();

        let mut reachable: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<String> = roots.into_iter().collect();
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

        for (owner, finding) in float_candidates {
            if reachable.contains(&owner) && !exempt.contains(&owner) {
                findings.push(finding);
            }
        }
    }

    // A macro-expanded argument (e.g. UE_LOG's own internal
    // implementation, confirmed by dogfooding against real Lyra source —
    // `LyraAssetManagerStartupJob.cpp`'s UE_LOG call re-evaluates its
    // `FPlatformTime::Seconds()` argument at the same file:line:column
    // three times) makes the identical call-site AST node reachable via
    // more than one expansion path; dedup on the exact reported location
    // rather than leaving misleadingly inflated finding counts.
    findings.sort_by(|a, b| {
        (&a.file, a.line, a.column, &a.rule).cmp(&(&b.file, b.line, b.column, &b.rule))
    });
    findings.dedup_by(|a, b| {
        a.file == b.file && a.line == b.line && a.column == b.column && a.rule == b.rule
    });
    for f in &findings {
        println!(
            "{}:{}:{}: warning: {} [drift-unreal::{}]",
            f.file.display(),
            f.line,
            f.column,
            f.message,
            f.rule
        );
    }

    Ok(if findings.is_empty() { 0 } else { 1 })
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // Hidden worker mode: `--worker <index> <compile_commands.json>
    // [config.toml]`. `run` spawns itself with this once per TU — never a
    // mode a user invokes directly.
    if args.get(1).map(String::as_str) == Some("--worker") {
        let index: usize = args[2].parse().context("worker index")?;
        let compile_commands = PathBuf::from(&args[3]);
        let config_path = args.get(4).map(PathBuf::from);
        return run_worker(index, &compile_commands, config_path.as_deref());
    }

    if args.len() < 2 {
        bail!(
            "usage: drift-unreal-lint <compile_commands.json> [config.toml]\n\
             unseeded_rng, wallclock_read, hashmap_iter, \
             unordered_parallelism, and usize_in_hashed_state always run; \
             float_outside_fixed_step needs config.toml's \
             tick_reachable_roots"
        );
    }
    let compile_commands = PathBuf::from(&args[1]);
    let config_path = args.get(2).map(PathBuf::from);
    let code = run(&compile_commands, config_path.as_deref())?;
    std::process::exit(code);
}
