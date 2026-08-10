# CI strategy

CI is infrastructure product testing, not just Rust compilation.

## Pull request/main gates

- `cargo fmt --check`
- clippy on workspace/all targets/all features with warnings denied
- workspace unit/integration tests
- release build
- static musl build used by image tests
- installer smoke tests on Ubuntu 22.04, Ubuntu 24.04, Debian 12 and Debian 13
- installer idempotency (install same binary twice)
- deterministic `lumic version` and structured `lumic status --json` smoke checks
- Cargo dependency advisory audit
- host-source fixture tests and process-runner behavior tests on the CI host
- package identifier/policy regression tests, durable event-store tests, and a real local-Git static deploy/persistence/rollback integration test
- a live Ubuntu 24.04 WordPress golden gate covering pinned artifact verification, PHP/MySQL/nginx provisioning, WP-CLI and HTTP health, convergent second install, duplicate-resource rejection, and safe uninstall retention

GitHub-hosted Ubuntu images include a password-initialized MySQL instance. Before live
service and recipe gates, CI removes that preinitialized server and its data so Lumic is
tested against the fresh-host installation contract used on a new VPS.

As host features grow, add privileged/VM/systemd tests instead of pretending Docker images model a complete VPS. Containers are acceptable for OS-detection/install/status smoke coverage only. Process timeout, bounded-output and argv behavior is covered by host-runner tests outside containers.

## Nightly channel

The nightly workflow runs scheduled CI, builds the static Linux artifact, and updates the `nightly` prerelease when all gates pass. A bad build must not replace the last working nightly.

## Versioned releases

An immutable tag must exactly match the Cargo workspace version. Stable `MAJOR.MINOR.PATCH` tags and explicit `MAJOR.MINOR.PATCH-PRERELEASE.N` tags run the same formatting, lint, test, audit, static-build, smoke, checksum and attestation gates. GitHub publishes tags with a prerelease suffix as prereleases; only stable tags can become the latest stable release.

## Future matrix

Add architecture and real-VM coverage as implementations arrive: x86_64 + aarch64, Debian/Ubuntu supported releases, systemd lifecycle, nginx, runtime/service installation, zero-downtime deployments and rollback.
