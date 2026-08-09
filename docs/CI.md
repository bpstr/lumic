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

As host features grow, add privileged/VM/systemd tests instead of pretending Docker images model a complete VPS. Containers are acceptable for OS-detection/install/status smoke coverage only. Process timeout, bounded-output and argv behavior is covered by host-runner tests outside containers.

## Nightly channel

The nightly workflow runs scheduled CI, builds the static Linux artifact, and updates the `nightly` prerelease when all gates pass. A bad build must not replace the last working nightly.

## Future matrix

Add architecture and real-VM coverage as implementations arrive: x86_64 + aarch64, Debian/Ubuntu supported releases, systemd lifecycle, nginx, runtime/service installation, zero-downtime deployments and rollback.
