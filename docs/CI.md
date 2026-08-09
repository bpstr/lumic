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
- `lumic version` and `lumic status` smoke checks
- Cargo dependency advisory audit

As host features grow, add privileged/VM/systemd tests instead of pretending Docker images model a complete VPS. Containers are acceptable for package-manager/OS-detection/install smoke coverage only.

## Nightly channel

The nightly workflow runs scheduled CI, builds the static Linux artifact, and updates the `nightly` prerelease when all gates pass. A bad build must not replace the last working nightly.

## Future matrix

Add architecture and real-VM coverage as implementations arrive: x86_64 + aarch64, Debian/Ubuntu supported releases, systemd lifecycle, nginx, runtime/service installation, zero-downtime deployments and rollback.
