//! Standalone determinism checker for Unreal C++, stock Clang/libclang — no
//! engine fork, no custom clang-tidy check compiled into LLVM (the winget
//! LLVM package ships `clang-c` + `libclang.lib` only, not the full
//! LibTooling/AST headers a real clang-tidy check needs to build against).
//!
//! Two rules, ported from the Rust/C# taxonomy (see
//! D:\Claude\drift-planning\drift-godot-unreal-plan.md §4/§6):
//! `unseeded_rng` fires unconditionally, repo-wide, same as its Rust/C#
//! counterparts. `float_outside_fixed_step` is opt-in only: does nothing
//! unless `tick_reachable_roots` is configured, same design as
//! drift-lint's own dylint.toml-driven reachability scoping.

use anyhow::{bail, Context, Result};
use clang::{Clang, Entity, EntityKind, Index};
use serde::Deserialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

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

fn is_float_type(entity: &Entity) -> bool {
    entity
        .get_type()
        .map(|t| {
            let spelling = t.get_display_name();
            spelling == "float" || spelling == "double"
        })
        .unwrap_or(false)
}

struct Finding {
    file: PathBuf,
    line: u32,
    column: u32,
    rule: &'static str,
    message: String,
}

fn scan_float_chains(body: Entity, findings: &mut Vec<Finding>) {
    fn visit(entity: Entity, inside_qualifying_parent: bool, findings: &mut Vec<Finding>) {
        let mut this_qualifies = false;
        if entity.get_kind() == EntityKind::BinaryOperator && is_float_type(&entity) {
            if let Some(op) = binary_operator_spelling(&entity) {
                if ARITHMETIC_OPS.contains(&op.as_str()) {
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
                    rule: "float_outside_fixed_step",
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
                            rule: "unseeded_rng",
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

fn run(compile_commands_path: &Path, config_path: Option<&Path>) -> Result<i32> {
    let config: Config = match config_path {
        Some(p) => {
            let text = std::fs::read_to_string(p)
                .with_context(|| format!("reading config {}", p.display()))?;
            toml::from_str(&text).with_context(|| format!("parsing config {}", p.display()))?
        }
        None => Config::default(),
    };

    let text = std::fs::read_to_string(compile_commands_path)
        .with_context(|| format!("reading {}", compile_commands_path.display()))?;
    let commands: Vec<CompileCommand> = serde_json::from_str(&text)?;

    let clang = Clang::new().map_err(|e| anyhow::anyhow!(e))?;
    let index = Index::new(&clang, false, false);

    let mut tus = Vec::new();
    for cmd in &commands {
        let mut args = cmd.arguments.clone().unwrap_or_else(|| {
            cmd.command
                .as_deref()
                .map(split_command)
                .unwrap_or_default()
        });
        if args.is_empty() {
            continue;
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

        let parse = index
            .parser(&cmd.file)
            .arguments(&args)
            .detailed_preprocessing_record(false)
            .parse();
        let tu = match parse {
            Ok(tu) => tu,
            Err(_) => continue,
        };
        tus.push((PathBuf::from(&cmd.directory).join(&cmd.file), tu));
    }
    let stats = std::env::var("DRIFT_UNREAL_STATS").is_ok();
    if stats {
        eprintln!(
            "stats: {}/{} translation units parsed",
            tus.len(),
            commands.len()
        );
    }

    let mut findings = Vec::new();

    // unseeded_rng: unconditional, every parsed TU, no config needed.
    for (_file, tu) in &tus {
        scan_unseeded_rng(tu.get_entity(), &mut findings);
    }

    let mut bodies: HashMap<String, Entity> = HashMap::new();
    let mut edges: HashMap<String, HashSet<String>> = HashMap::new();

    for (_file, tu) in &tus {
        tu.get_entity().visit_children(|cur, _parent| {
            if is_function_like(cur.get_kind()) && cur.is_definition() {
                let qname = qualified_name(&cur);
                bodies.insert(qname.clone(), cur);
                collect_call_edges(cur, &qname, &mut edges);
            }
            clang::EntityVisitResult::Recurse
        });
    }

    if stats {
        eprintln!(
            "stats: {} function/method definitions found, {} call edges",
            bodies.len(),
            edges.values().map(|v| v.len()).sum::<usize>()
        );
        for root in &config.tick_reachable_roots {
            eprintln!(
                "stats: root '{root}' resolved: {}",
                bodies.contains_key(root)
            );
        }
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

        for name in &reachable {
            if exempt.contains(name) {
                continue;
            }
            if let Some(body) = bodies.get(name) {
                scan_float_chains(*body, &mut findings);
            }
        }
    }

    findings.sort_by(|a, b| (&a.file, a.line, a.column).cmp(&(&b.file, b.line, b.column)));
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
    if args.len() < 2 {
        bail!(
            "usage: drift-unreal-lint <compile_commands.json> [config.toml]\n\
             unseeded_rng always runs; float_outside_fixed_step needs \
             config.toml's tick_reachable_roots"
        );
    }
    let compile_commands = PathBuf::from(&args[1]);
    let config_path = args.get(2).map(PathBuf::from);
    let code = run(&compile_commands, config_path.as_deref())?;
    std::process::exit(code);
}
