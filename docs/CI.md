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

Installer coverage is organized into four explicit levels: catalog definition validation,
plan/dry-run validation, live service lifecycle, and complete application golden tests.
The reusable harness under `tests/installer/` validates every trusted managed-service
definition and plan. PostgreSQL, MySQL and Redis additionally run on a real systemd host
with health and loopback-port checks, a convergent second install, native client
read/write probes, backup validation, secret-state checks and cleanup. CI uploads compact
JSON result artifacts for the catalog and live-service suites so failures can be compared
across future OS and architecture matrices.

GitHub-hosted Ubuntu images include a password-initialized MySQL instance. Before live
service and recipe gates, CI removes that preinitialized server and its data so Lumic is
tested against the fresh-host installation contract used on a new VPS.

As host features grow, add privileged/VM/systemd tests instead of pretending Docker images model a complete VPS. Containers are acceptable for OS-detection/install/status smoke coverage only. Process timeout, bounded-output and argv behavior is covered by host-runner tests outside containers.

## Nightly channel

The nightly workflow runs scheduled CI, builds the static Linux artifact, and updates the `nightly` prerelease when all gates pass. A bad build must not replace the last working nightly.

## Versioned releases

Release candidates are pushed to `main` without a version tag. The exact candidate commit must first pass every required push workflow, including CI, documentation, and application golden coverage. A fix or test commit is not independently released; related commits share one candidate version.

After the candidate is green and final, an immutable tag matching the Cargo workspace version may be pushed. Stable `MAJOR.MINOR.PATCH` tags and explicit `MAJOR.MINOR.PATCH-PRERELEASE.N` tags run the release workflow's formatting, lint, test, audit, static-build, smoke, checksum and attestation gates. GitHub publishes tags with a prerelease suffix as prereleases; only stable tags can become the latest stable release.

A tag pushed before green candidate CI is invalid. If any gate fails, delete that local and remote tag, publish no GitHub release for it, retire the version, and advance the workspace version before trying again. Successful published release tags remain immutable.

## Future matrix

Add architecture and real-VM coverage as implementations arrive: x86_64 + aarch64, Debian/Ubuntu supported releases, systemd lifecycle, nginx, runtime/service installation, zero-downtime deployments and rollback.
