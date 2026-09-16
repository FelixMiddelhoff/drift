## What this changes

<!-- One or two sentences: what changed and why. -->

## Changelog-relevant description

<!-- One line suitable for the changelog, or "N/A" if this PR has no user-visible effect
     (internal refactor, CI, docs-only). -->

- [ ] This PR is changelog-relevant (description above will be used in release notes)
- [ ] This PR is internal-only, no changelog entry needed

## Checklist

- [ ] `just check` passes locally (fmt, clippy, tests)
- [ ] If this adds/changes a lint rule: fixtures added/updated in that binding's own test corpus (`crates/drift-lint/ui`+`tests`, `bindings/csharp/Drift.Analyzers.Tests`, `bindings/unreal/drift-unreal-lint/tests`, or `bindings/godot/drift-godot-lint/tests`), and `docs/rule-catalog.md` updated in the same PR
- [ ] If this changes public API (rule IDs, suppression attributes, reachability-tagging API): docs updated in the same PR, not deferred
