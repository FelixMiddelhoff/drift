# Rust quickstart

```bash
cargo install cargo-dylint dylint-link
```

```bash
cargo dylint --path crates/drift-lint --workspace
```

`--path` works against a local clone; a `--git https://github.com/FelixMiddelhoff/drift` install works the same way once this is pushed. Not published to crates.io — see the [rule catalog](./rule-catalog.md) for why, and `cargo dylint`'s own docs for the full CLI.

## Reachability scoping

`float_outside_fixed_step` is opt-in — configure it via a `dylint.toml` at your workspace root:

```toml
[drift-lint]
tick_reachable_roots = ["main"]
```

**Real behavior, not obvious from the name**: setting `tick_reachable_roots` at all scopes *every* rule except `usize_in_hashed_state`, not just `float_outside_fixed_step` — see the [rule catalog](./rule-catalog.md) for the full explanation.
