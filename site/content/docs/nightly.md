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

Operator-visible additions and fixes are summarized in the [Changelog](@/changelog.md).

A nightly release is produced only after required quality gates pass. The repository CI already targets Rust formatting/lint/tests, a static Linux binary and install smoke tests across supported Ubuntu/Debian images; host/systemd-capable tests are added as those capabilities become real.

## Catalog expansion

Nightly is also Lumic's delivery lane for breadth after a shared mechanism has been proven. Application recipes such as Laravel, Drupal, Symfony, Forgejo, Ghost and Matomo, and framework/service definitions such as Laravel + Typesense, should arrive as incremental catalog additions over the existing recipe, resource, binding and lifecycle contracts. They do not require a new fast-track epic.

Each addition must retain STATUS -> SUGGEST -> PLAN -> APPLY separation, use typed operations and secret references, include recovery and focused tests, and document its actual supported-host coverage. If an integration exposes a missing shared primitive, the change should add only that narrow reusable capability. Catalog work remains independent of core operator-UX work and must not hold up improvements that benefit every application and service.

## Rules

- never manufacture a feature merely to create nightly activity;
- add catalog breadth as small reviewed slices, not framework-specific orchestration projects;
- do not silently move a server between stable and nightly;
- CI must be green before a nightly artifact is publishable;
- documentation changes ship with the behavior they describe.
