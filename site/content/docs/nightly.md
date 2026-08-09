+++
title = "Nightly channel"
description = "Early Lumic development ships continuous gated improvements while stable stays conservative."
weight = 120
[extra]
kicker = "RELEASES"
status = "active"
+++

**Lumic gets better every night.**

Nightly is both a release channel and a development discipline during early v2.

A nightly release is produced only after required quality gates pass. The repository CI already targets Rust formatting/lint/tests, a static Linux binary and install smoke tests across supported Ubuntu/Debian images; host/systemd-capable tests are added as those capabilities become real.

Nightly coding-agent work follows `docs/CODEX_NIGHTLY.md`: inspect current state, fix correctness/security/CI regressions first, then complete one coherent improvement with tests and documentation.

## Rules

- never manufacture a feature merely to create nightly activity;
- do not silently move a server between stable and nightly;
- CI must be green before a nightly artifact is publishable;
- discovered work should become explicit follow-up issues;
- documentation changes ship with the behavior they describe.
