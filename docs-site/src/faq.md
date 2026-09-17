# FAQ / troubleshooting

Real questions with real answers — every claim here is checked against the actual source or a real run, not written from memory. If something here turns out stale, [open an issue](https://github.com/FelixMiddelhoff/drift/issues).

## General

### A rule didn't fire on code I expected it to flag — is that a bug?

Usually not — check these first, in order:

1. **Is the rule reachability-scoped, and did you configure it?** `float_outside_fixed_step` is opt-in on the Rust and Unreal sides (needs `tick_reachable_roots` in `dylint.toml`/`config.toml`) — without it, that one rule does nothing at all, silently. Godot auto-detects `_process`/`_physics_process` by name instead, so this doesn't apply there.
2. **On Rust specifically**: configuring `tick_reachable_roots` at all scopes *every* rule except `usize_in_hashed_state`, not just `float_outside_fixed_step` — a real, easy-to-miss behavior. If your root is too narrow, the other five rules can stop firing on code that isn't reachable from it. See the [Rust walkthrough](./rust-walkthrough.md) for the full explanation.
3. **Did you run against the whole project, not just one file?** On Godot specifically, `_process`/`_physics_process` bodies are only counted if the tool actually walked that file — run with `DRIFT_GODOT_STATS=1` (see below) to confirm every file parsed and how many functions were resolved reachable, rather than trusting a clean run.
4. **Type-inference limits (Godot only)**: GDScript is dynamically typed, and `drift-godot-lint` has no type checker — `float_outside_fixed_step` only recognizes a variable as float-ish if it's a literal or explicitly typed `: float` *in the same function*. Untyped arithmetic (`var c = a + b`, neither side typed) is invisible to it. This is real and permanent, not a bug — see the [rule catalog](./rule-catalog.md) for the exact rule.
5. **Syntactic matching, not full symbol resolution (Unreal/Godot)**: `unseeded_rng`/`wallclock_read`/`unordered_parallelism` match by call-site spelling (`FMath::Rand`, `Time.get_ticks_msec`, etc.), not resolved declarations. An aliased import or an unusual call shape can slip past. The Rust and C# sides use real compiler type information instead and don't have this limitation.

### The tool reported zero findings — is that trustworthy?

Don't just trust a clean run — verify it actually processed your code:

- **Godot**: run with `DRIFT_GODOT_STATS=1` — it prints files scanned, parse errors, function count, and reachable-function count to stderr. Zero findings alongside `0 parse errors` and a plausible function count is real evidence; zero findings with parse errors or a suspiciously low function count means the tool silently skipped something.
- **Unreal**: run with `DRIFT_UNREAL_STATS=1` — prints `N/M translation units parsed`. If `N < M`, some files failed to parse (a real, disclosed cost of parsing real Unreal C++ without a precompiled header) and their findings are missing, not absent.
- **Rust**: `cargo dylint` fails loudly on a compile error rather than silently skipping files, so a clean exit is stronger evidence here than on the other two.

### How do I suppress a specific finding?

Depends on the binding — and this is genuinely uneven, not a design choice:

- **Rust**: `#[allow(drift_hashmap_iter)]` (or the matching attribute name for any other rule) on the item.
- **C# / Unity**: `#pragma warning disable DRIFT0001` / `#pragma warning restore DRIFT0001`.
- **Unreal / Godot**: **no suppression mechanism exists yet** — no comment-based ignore, no config-level allowlist. If you have a confirmed false positive, your only options today are to restructure the code so it doesn't match the rule's pattern, or accept the finding and don't fail your CI build on it. This is a real, disclosed gap, not a hidden one — worth an issue if it's blocking you.

## Rust (`drift-lint`)

### `cargo dylint` fails to build with a linker error mentioning `dylint-link`

Install it: `cargo install dylint-link` (alongside `cargo-dylint`). `crates/drift-lint`'s own `.cargo/config.toml` points `rustc` at `dylint-link` as the linker — without it on `PATH`, even an unrelated dependency's build script fails to link.

### My `dylint.toml` config isn't being picked up

Root paths are matched against `TyCtxt::def_path_str`, which is **crate-relative** — it never includes your crate's own name. `tick_reachable_roots = ["my_crate::tick"]` silently matches nothing; use `["tick"]` for a top-level function or `["module::path::tick"]` for a nested one. Confirmed the hard way this session — a first attempt with a crate-name prefix produced zero reachable functions with no error at all.

### Building `crates/drift-lint` is slow / seems to hang on a clean checkout

Its `dylint_driver` build script does a full, uncached clone of `rust-lang/rust-clippy` on every clean build (needed to extract symbol data for recent nightlies) — a real operational fragility, not a bug in this project. `Swatinem/rust-cache` (already used in this repo's own CI) caches `target/` across runs so this only bites on a genuinely clean cache.

### Do I need to publish this to crates.io to use it?

No, and you can't — `drift-lint` depends on `clippy_utils` via git (`rust-lang/rust-clippy`), which is itself never published to crates.io (its own README says it provides no stability guarantees). crates.io rejects any crate with an unversioned git dependency, confirmed via a real `cargo publish --dry-run`. Install via `cargo dylint --path <clone>` or `--git https://github.com/FelixMiddelhoff/drift` instead — see the [Rust quickstart](./quickstart-rust.md).

## C# / Unity (`Drift.Analyzers` / `GameDeterminism.Analyzers`)

### Why is the NuGet package name different from the project name?

`Drift` is a NuGet ID prefix reserved by an unrelated company — confirmed via nuget.org's own upload UI, which returned "The package ID is reserved" on every version pushed under `Drift.Analyzers`. Published as `GameDeterminism.Analyzers` instead; the underlying project (`bindings/csharp/Drift.Analyzers`) and DLL filename are unchanged. See the [C# quickstart](./quickstart-csharp.md).

### The analyzer isn't showing up in my IDE after installing

Roslyn analyzers need to be referenced in the correct NuGet layout (`analyzers/dotnet/cs/`) to auto-load — the published package does this correctly. If you're referencing the DLL directly instead of via NuGet, double-check the `<Analyzer Include="...">` item group, not `<Reference>` or `<PackageReference>` styles meant for ordinary libraries.

## Unreal (`drift-unreal-lint`)

### `LIBCLANG_PATH` errors, or the tool can't find libclang

Point it at your LLVM install's `bin` directory explicitly — the `clang` crate doesn't reliably find a system libclang on its own across platforms. Needed both to build the tool and to run it (a prebuilt release binary still needs this set at runtime; it links libclang dynamically, doesn't bundle it).

### A file's findings seem to be missing entirely

Run with `DRIFT_UNREAL_STATS=1` and check `N/M translation units parsed` — a real libclang crash (OOM, a genuine parse failure) costs that one file's findings, reported to stderr as a warning, not silently dropped from the count. This was a real bug found dogfooding a 388-file sweep (see the [rule catalog](./rule-catalog.md)) and is now crash-isolated per file via a worker-subprocess architecture, but a crash still means that one file contributes nothing.

### The sweep is slow against a large codebase

Workers run concurrently by default (a thread pool sized to `available_parallelism()`). Cap it with `DRIFT_UNREAL_JOBS=<n>` if you need to control peak memory instead — each worker is a full libclang parse, which is genuinely expensive without a precompiled header.

## Godot (`drift-godot-lint`)

### The tool found the same finding twice at the same location

Should be fixed — a real duplicate-finding bug (flat, name-only call graph resolving two same-named functions to one, scanning the same body twice) was found and fixed with a dedup pass. If you still see this, it's a regression worth reporting with the exact command and file.

### A finding is reported on the wrong line

Should also be fixed — a real bug where a compound-assignment (`x += y`) finding got attributed to the *previous* statement's line whenever it wasn't the first statement in its function (a `gdck-syntax` parser checkpoint quirk). Fixed and locked in with a regression test. If you see a location that clearly doesn't match the reported rule, it's worth reporting with the exact file.

### `hashmap_iter`/`usize_in_hashed_state` don't exist for Godot — why?

Not deferred, genuinely N/A: Godot's `Dictionary` is insertion-ordered by engine guarantee (unlike Rust's `HashMap` or C#'s `Dictionary`), so iteration-order desync isn't a real hazard there. GDScript's `int` is always 64-bit — there's no fixed/pointer-width distinction for `usize_in_hashed_state` to have a bug in.
