# Godot walkthrough: adding drift to a project

Full step-by-step example: taking a real Godot 4 GDScript project from zero to a `drift-godot-lint` run, including the fix for a real bug this tool's own dogfooding surfaced (see [the rule catalog](./rule-catalog.md) for the full story) — this walkthrough uses that same real-world case as its running example.

## 1. Get the tool

```bash
# prebuilt (no toolchain needed)
# grab the binary for your platform from the latest release:
# https://github.com/FelixMiddelhoff/drift/releases/latest

# or build from source
git clone https://github.com/FelixMiddelhoff/drift
cargo build --release --manifest-path drift/bindings/godot/drift-godot-lint/Cargo.toml
```

Pure Rust, no engine dependency (via [gdck-syntax](https://crates.io/crates/gdck-syntax)) — no Godot install needed to run it, only to run *your* project.

## 2. First run: three rules fire unconditionally

```bash
drift-godot-lint <file.gd | project directory>
```

`unseeded_rng`, `wallclock_read`, and `unordered_parallelism` need no configuration. Against a script like this:

```gdscript
func roll_damage() -> int:
    return randi_range(1, 6)

func log_event() -> void:
    var now := Time.get_ticks_msec()
```

```
player.gd:2:11: warning: randi_range() reads or reseeds Godot's global RNG, which is not deterministically tracked by default; differs per peer/run [drift-godot::unseeded_rng]
player.gd:5:15: warning: Time.get_ticks_msec() reads wallclock/OS time, which differs per peer/run; do not fold it into simulated state [drift-godot::wallclock_read]
```

## 3. Fixing a real finding

Godot's global RNG is auto-seeded from OS entropy — two peers in a lockstep session get different results from the same bare call. Fix: route through a project-owned, explicitly-seeded `RandomNumberGenerator` instance instead:

```gdscript
# Before — flagged
func roll_damage() -> int:
    return randi_range(1, 6)

# After — deterministic, seed is part of tracked simulation state
var seeded_rng := RandomNumberGenerator.new()

func roll_damage() -> int:
    return seeded_rng.randi_range(1, 6)
```

`unseeded_rng` only matches a **bare** call (`randi_range(...)`) — a member call on your own RNG instance (`seeded_rng.randi_range(...)`) is a different shape and never fires, so this fix is recognized, not just visually similar.

## 4. `float_outside_fixed_step` — no config needed

The one reachability-scoped rule auto-detects Godot's own fixed tick entry points by name — `_process`/`_physics_process` — no config file, unlike the Rust/Unreal sides' `tick_reachable_roots`:

```gdscript
func _physics_process(delta: float) -> void:
    velocity_y = velocity_y + gravity * delta   # fires — reachable, float arithmetic
    elapsed_time += delta                        # fires too — compound assignment, same rule
```

Both non-associative-arithmetic shapes fire: plain binary arithmetic (`a + b * c`) and compound-assignment accumulation (`x += y`) are both matched.

**Real limitation, disclosed not hidden**: GDScript is dynamically typed by default, and this tool has no type checker — an operand only counts as float-ish if it's a float literal or a variable/parameter explicitly typed `: float` *in that same function*. Pure untyped arithmetic (`var c = a + b`, neither side typed or literal) is invisible to this rule even when reachable.

## 5. Running against a real, multi-file project

This is the case that matters most in practice, and where a real bug in this tool was found and fixed: `_process`/`_physics_process` are about the most common function names in any real Godot project (nearly every scene script defines one). Point the tool at your project root, not just one file:

```bash
drift-godot-lint path/to/your/project
DRIFT_GODOT_STATS=1 drift-godot-lint path/to/your/project  # prints a parse/function/finding summary to stderr
```

`DRIFT_GODOT_STATS=1` is worth running once on a new project — it confirms every file actually parsed (0 parse errors) and reports how many function bodies were resolved reachable, so a suspiciously clean run is something you can verify, not just trust.

## Known limitations

See the [rule catalog](./rule-catalog.md#godot-gdscript-bindingsgodotdrift-godot-lint) for the full, honest list. The short version: call-graph edges are resolved by bare function name (no symbol table), which can widen reachability across two unrelated same-named functions — a false-positive risk, not a false-negative one, and this tool's own real dogfooding (including the multi-file collision bug above) is the reason that trade-off is documented instead of assumed safe.
