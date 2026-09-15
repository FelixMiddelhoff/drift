# Contributing to Drift

## Building and testing

```bash
cargo build --workspace
cargo test --workspace
```

Or, with [`just`](https://github.com/casey/just) installed, run the same gate CI runs:

```bash
just check
```

This runs `cargo fmt --check`, `cargo clippy -D warnings`, and `cargo test --workspace` — matching the required CI job. Run it locally before pushing.

## Toolchain

The Rust version is pinned in `rust-toolchain.toml`. `rustup` will pick it up automatically; you don't need to install it separately.

## Rule-authoring changes

Any change to a lint rule (`crates/drift-lint`, or once it exists, `bindings/csharp/Drift.Analyzers`) is validated against `drift-lint-testing`'s fixture corpus — a rule's false-positive/false-negative behavior is the actual product, not an implementation detail, so a rule change without an updated or added fixture won't be merged. See `drift-planning/drift-plan.md §7` (validation strategy) for why this matters more here than in a typical project.

## Submitting a pull request

1. Open an issue first for anything non-trivial (a new rule, a change to the reachability-tagging API, a new suppression mechanism) — cheap to discuss before code exists, expensive to discuss after.
2. Keep PRs scoped to one change. One new rule (implementation + fixtures + docs/rule-catalog.md entry) per PR is the expected shape.
3. Fill in the PR template's changelog-relevant-description checkbox — it feeds the changelog automation.
4. CI must pass (fmt, clippy, tests) before merge.
5. Every commit needs a `Signed-off-by:` trailer (`git commit -s`) — a Developer Certificate of Origin, not a copyright assignment: you're attesting you have the right to submit the code under this project's license, and you keep your own copyright. Enforced by the `DCO` CI check. Forgot on an existing commit? `git commit --amend -s` (or `git rebase --signoff <base>` for a range).

## Documentation

`docs/rule-catalog.md` is the actual product surface most users read — a new or changed rule isn't done until its catalog entry (what it flags, why, a real example, the fix) is written or updated in the same PR.

## Code of Conduct

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md).
