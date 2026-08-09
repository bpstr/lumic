+++
title = "Nightly channel"
description = "Lumic ships continuous gated builds while stable stays conservative."
weight = 120
[extra]
kicker = "RELEASES"
status = "active"
+++

**Lumic gets better every night.**

Nightly is an explicit opt-in release channel for testing the latest gated build from `main`.

A nightly release is produced only after required quality gates pass. The repository CI already targets Rust formatting/lint/tests, a static Linux binary and install smoke tests across supported Ubuntu/Debian images; host/systemd-capable tests are added as those capabilities become real.

## Rules

- never manufacture a feature merely to create nightly activity;
- do not silently move a server between stable and nightly;
- CI must be green before a nightly artifact is publishable;
- documentation changes ship with the behavior they describe.
