# Production readiness audit

Audit date: 2026-08-09

Scope: the Rust workspace, CLI, daemon, local operator UI, stdio MCP server, native-platform adapters, installer, public/internal documentation, tests, and GitHub Actions workflows.

## Executive summary

**NOT READY** for a stable production release.

The reviewed product has a strong safety baseline: typed operations use separated process arguments, the UI is loopback-only, MCP mutation is policy/scope/approval gated, secret values are kept out of public state, destructive paths are validated, release builds succeed, the dependency advisory scan is clean, and meaningful Linux lifecycle/smoke coverage exists. No known critical vulnerability remains after this audit.

This audit fixed concrete session-lifecycle, append-only persistence, bounded-read, symlink, duplicate-code, and stale-smoke-test problems. The repository nevertheless identifies itself as `2.0.0-alpha.1`, stable releases are not published, and several state services still perform unlocked read-modify-write cycles across the independently runnable CLI and daemon. That lost-update risk must be addressed and exercised under contention before a stable release. Signed/provenanced release artifacts and UI login throttling also remain release-hardening work.

## Critical findings

No known unresolved critical security vulnerability, credential disclosure, cross-tenant access, or immediately reproducible data-loss defect was found.

Lumic is a single-node/single-administrator product at present; there is no tenant boundary to bypass. Sensitive UI operations are authorized server-side through a valid session and session-bound CSRF value. MCP mutations are independently checked by node policy, scope, and explicit approval.

## High-priority findings

### Fixed: concurrent JSON-lines writes could corrupt durable history

The event, audit, and operations-timeline stores serialized directly into append-mode files in multiple writes and without an inter-process lock. Concurrent CLI/daemon writers could interleave JSON with its newline or with another serialized record. The three implementations also followed final-component symlinks. They now use one shared helper that serializes a complete line before taking an advisory file lock, rejects non-regular/final-component symlink targets on Unix, writes the line as one buffered operation, and syncs it.

### Fixed: history reads had unbounded memory behavior

Event, audit, and operations-timeline reads previously collected the entire file before applying a requested limit. They now retain only the requested tail in a bounded `VecDeque`; the operations timeline has a 10,000-record read ceiling matching its public query limit. Time remains linear in file size, so rotation/compaction is still recommended.

### Fixed: operator sessions were not revocable or bounded

Rotating the UI admin token did not invalidate already authenticated eight-hour sessions, and expired/unreferenced sessions accumulated until daemon restart. Sessions now carry the credential digest revision, are rejected and removed after token rotation, are pruned on authentication/login, and are capped at 1,024 entries. A poisoned session lock no longer panics the logout request.

### Fixed: the Epic G smoke test asserted the wrong storage contract

The test expected a typed provider signal from the operations timeline to appear in the attention report, although attention intentionally consumes the canonical durable event store. The test now verifies the provider signal in `operations timeline` and verifies an actual personality-change event in attention. All seven Linux smoke suites pass with that documented separation.

### Remaining blocker: cross-process state read-modify-write is not serialized

Atomic replacement protects individual JSON state files from torn writes, but services such as operations automation can still load, independently modify, and replace the same state from concurrent CLI and daemon processes. The last writer can silently discard the other process's update. This needs a state-store locking/transaction contract plus contention tests before stable release.

## Code removed

- Removed three identical hexadecimal encoders from operations, infrastructure, and secret storage; one crate-private implementation remains.
- Removed duplicate event/audit/operations JSON-lines append and read implementations in favor of one small persistence helper.
- Removed the production-only UI credential verification wrapper after login was changed to retain the verified credential revision directly.

No public command, route, MCP tool, or documented extension point was removed. Graph reachability inspection and strict all-target Clippy did not establish other production code as safely dead. Test-only `unwrap`/`expect` calls were not treated as production panic paths.

## Simplifications

Append-only persistence now has one implementation and one error/safety policy. Newest-first bounded reads are consistent across audit, event, and operations history. Credential verification and session creation use a single digest read, which both authenticates the token and records the revision required for revocation.

## Security findings

Remediated findings:

- UI admin-token rotation now revokes existing sessions.
- UI session memory is bounded and expired entries are proactively removed.
- Durable JSON-lines stores reject unsafe final-component file types/symlinks on Unix and serialize concurrent readers/writers with advisory locks.
- Concurrent append and symlink rejection have regression tests.

Verified controls:

- The daemon refuses non-loopback UI binding. Cookies are HttpOnly and SameSite=Strict, mutating UI requests require session-bound CSRF, and browser responses set CSP, no-sniff, no-referrer, and no-store headers.
- The default local HTTP transport is documented for SSH tunnels or an authenticated TLS reverse proxy. `Secure` is intentionally not set because direct loopback HTTP is supported.
- MCP is stdio-only; mutation requires the opt-in environment policy, an allowed scope, and `approved=true`. It is explicitly not a network authentication layer.
- Process execution uses fixed executables and separated arguments; reviewed package, systemd, firewall, path, URL, webhook, and repository inputs are typed/validated.
- Secret scanning of tracked files found no live credential, private key, or token. Test values are synthetic and secret-returning surfaces use references/redaction.
- `cargo audit --deny warnings` scanned 163 locked dependencies against 1,198 RustSec advisories with no warning.

Unresolved security hardening:

- The UI has no built-in login throttling. Loopback binding limits exposure, and a remote TLS/auth proxy should enforce throttling, but this must be explicit in a production deployment profile.
- Nightly artifacts have SHA-256 checksums but no signature/attestation rooted separately from the release channel. A checksum detects corruption, not a compromised publisher. GitHub Actions are referenced by moving major tags rather than immutable commit SHAs.
- The local admin token represents one coarse administrator identity; fine-grained/persistent identities are not implemented and must not be claimed.

## Tests and validation

Passed on the audited tree:

- `cargo fmt --check`
- `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`
- `cargo test --locked --workspace --all-features` (71 unit/integration tests on macOS; the Linux-only daemon test is compile-time excluded there)
- `cargo build --locked --workspace --release`
- `cargo audit --deny warnings` (163 locked dependencies, no advisory warning)
- `sh -n install.sh tests/*.sh`
- Clean Debian 12 container: `cargo test --locked -p lumic-daemon --test graceful_shutdown` (daemon started on loopback, handled SIGTERM, and exited cleanly)
- Clean Debian 12 container: locked release build of `lumic`/`lumicd`
- Clean Debian 12 container: `tests/epic-a-smoke.sh` through `tests/epic-g-smoke.sh` against the release CLI (all seven passed)

The install smoke script was not rerun locally because the available Docker host is AArch64 and the installer correctly rejects architectures other than the currently published x86_64 target. CI defines install-image checks for Ubuntu 22.04/24.04 and Debian 12/13 using the exact x86_64 musl artifacts it builds. The workflow definition was inspected, but this audit did not obtain remote run results or reproduce real systemd/apt mutations on a disposable x86_64 VPS.

## Remaining risks

- Unlocked cross-process read-modify-write state can lose concurrent updates even though individual file replacement is atomic.
- JSON-lines reads are memory-bounded but still scan the complete retained file; there is no rotation, compaction, or corruption-tail recovery policy.
- Stable installation/release is intentionally unavailable and the crate version is alpha.
- Release provenance is checksum-only and CI actions are not commit-pinned.
- UI login attempts are not throttled in-process.
- Live managed-service, package, firewall, systemd-install, backup/restore, self-update rollback, and supported-image installation behavior was not independently rerun on real VPS images during this local audit. CI contains dedicated jobs, but production promotion should require their successful results and recovery drills.
- There is no database schema or migration layer in this repository; migration safety is therefore not applicable. Background work is a bounded daemon loop, but long-running soak, forced-crash, disk-full, and concurrent-writer fault tests are absent.

The production defaults reviewed are conservative, documentation matches the implemented local UI/MCP boundaries after this change, and no competing implementation remains for the duplicated helpers fixed here. Those positives do not outweigh the state-concurrency and release-provenance blockers.

## Recommended follow-up

1. Introduce a minimal cross-process lock/transaction boundary for every mutable state file, then add CLI/daemon contention and crash-recovery tests.
2. Sign or attest release artifacts, pin third-party CI actions immutably, and make successful supported-image install/live-service jobs mandatory for promotion.
3. Define a documented login-throttling boundary and add audit/event rotation plus corrupt-tail recovery tests.
4. Run install, upgrade, rollback, backup restore, disk-full, and abrupt-termination drills on each claimed x86_64 Debian/Ubuntu image before declaring a stable release.
