# Drift

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Static lints that catch game-simulation non-determinism — unordered-container iteration, unseeded RNG, wall-clock reads, float ops outside a fixed step, and more — before they cause a lockstep/rollback-netcode desync, instead of debugging the desync after the fact.

Companion to [Foldback](https://github.com/FelixMiddelhoff/foldback) (runtime desync detection and bisection) and [Poncelet](https://github.com/FelixMiddelhoff/poncelet) (a bit-exact ballistics library used as a real-world dogfood target here): drift prevents what it can at build time, Foldback finds what slips through at runtime.

Full plan and design rationale: [drift-planning/drift-plan.md](../drift-planning/drift-plan.md).

## License

Licensed under either of

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
