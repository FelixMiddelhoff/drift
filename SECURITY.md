# Security Policy

## Reporting a vulnerability

Please use GitHub's [private vulnerability reporting](https://github.com/FelixMiddelhoff/drift/security/advisories/new) (Security tab → "Report a vulnerability") rather than a public issue — it opens a private advisory only the maintainer can see until a fix is ready.

If you'd rather not use GitHub, DM [@FelixMiddelhoff](https://github.com/FelixMiddelhoff) on GitHub.

Please include:
- What you found and why it's a security issue, not just a bug.
- Steps to reproduce, or a minimal input triggering it.
- The affected crate/binding and commit or version.

## Scope

Drift is a build-time/CI static-analysis tool, not something that runs in a trust boundary by design — it doesn't execute the code it analyzes, only parses/lints it. The main thing worth reporting here is a crash, hang, or resource-exhaustion issue in the lint engine (`drift-lint` or, once it exists, `Drift.Analyzers`) triggered by adversarial or pathological source input — a tool that's supposed to run in CI shouldn't be able to hang a CI job or crash on legitimate-but-unusual code.

Out of scope: issues that require an attacker to already have local code execution on the machine running drift, and vulnerabilities in third-party dependencies without a demonstrated impact on drift itself (report those upstream — Dependabot already tracks known-vulnerable dependencies here).

## Supported versions

Pre-1.0 (`0.x`), not yet published to crates.io — fixes land on `main` and the latest commit is the only supported one. This section will be replaced with a real version table once there's a tagged release history to support.

## Response

This is currently a solo-maintained project — no formal SLA, but security reports get priority over routine issues. Expect an initial response within a few days.
