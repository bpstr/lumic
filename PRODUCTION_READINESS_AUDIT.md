# Production readiness audit

Audit date: 2026-08-10

Scope: the Rust workspace, CLI, daemon, local operator UI, stdio MCP server, native-platform adapters, installer, public/internal documentation, tests, and GitHub Actions workflows.

## Executive summary

**READY FOR GATED 1.0.0 PROMOTION.**

The audit found no known critical vulnerability. The release candidate has typed direct-argument operations, loopback-only UI access, policy/scope/approval-gated MCP mutation, redacted secrets, validated destructive paths, atomic durable writes, serialized concurrent operations-state updates, bounded and revocable UI sessions, in-process login throttling, verified updates, commit-pinned CI actions, checksummed artifacts, and artifact provenance.

The repository and installer now consistently identify the stable release as `1.0.0`. A numeric Semantic Versioning tag starts the stable release workflow, which refuses a Cargo-version mismatch and publishes only after formatting, Clippy, tests, the RustSec audit, all capability smoke suites, live managed-service/recipe gates, and supported-image installer tests pass. Creating a tag is therefore the start of promotion, not evidence that promotion succeeded.

## Critical findings

No known unresolved critical security vulnerability, credential disclosure, cross-tenant access, or immediately reproducible data-loss defect was found.

Lumic is a single-node/single-administrator product at present; there is no tenant boundary to bypass. Sensitive UI operations are authorized server-side through a valid session and session-bound CSRF value. MCP mutations are independently checked by node policy, scope, and explicit approval.

## High-priority findings resolved

### Concurrent JSON-lines writes

Event, audit, and operations-timeline stores previously serialized directly into append-mode files in multiple writes and without an inter-process lock. They now use one shared helper that serializes a complete line before taking an advisory lock, rejects non-regular/final-component symlink targets on Unix, writes the line as one buffered operation, and syncs it.

### Unbounded and crash-fragile history reads

History reads now retain only their requested tail in a bounded `VecDeque`; the operations timeline has a 10,000-record public ceiling. A single incomplete, unterminated crash tail is ignored, while a malformed complete record remains an error. Time remains linear in retained file size, so rotation or compaction is still recommended.

### Operations-state lost updates

Operations automation formerly performed unlocked read-modify-write replacement from independently runnable CLI and daemon paths. The operations state now uses a validated sibling lock file and holds an exclusive advisory lock across load, mutation, and atomic save. Runtime signal, webhook, subscription, rule, and rollback mutations use that transaction boundary. An eight-thread contention regression test verifies 400 updates without loss.

Other state services retain atomic replacement. They do not yet share this general transaction primitive; any service gaining independently concurrent writers must adopt the same boundary before that writer is introduced.

### Operator session and login lifecycle

Admin-token rotation now revokes existing sessions. Expired sessions are pruned, the in-memory collection is capped, poisoned logout locking does not panic, and repeated login failures are throttled after five attempts in 60 seconds with `429 Too Many Requests` and `Retry-After`.

### Release integrity and delivery

Stable installation defaults to GitHub's latest stable release, while exact versions resolve immutable numeric tags. The installer records the selected channel, verifies downloaded SHA-256 assets, and the self-updater follows the recorded stable/nightly channel with backup and rollback behavior.

Stable and nightly workflows pin third-party actions to full commit SHAs and issue GitHub artifact attestations for release binaries. The stable workflow additionally checks the tag against the Cargo workspace version and gates publication on the dependency audit, full Rust tests, smoke suites, supported Debian/Ubuntu installer images, and live service/recipe tests.

## Code removed and simplified

- Removed three identical hexadecimal encoders; one crate-private implementation remains.
- Replaced duplicate event/audit/operations JSON-lines persistence with one helper and one safety policy.
- Removed the redundant production-only UI credential wrapper.
- Removed the early-development Codex kickoff, fast-track, and scheduled-job documents. Generic agent instructions and reusable skills remain.

No public command, route, MCP tool, or documented extension point was removed.

## Verified security controls

- The daemon refuses non-loopback UI binding. Cookies are HttpOnly and SameSite=Strict, mutating UI requests require session-bound CSRF, and browser responses set CSP, no-sniff, no-referrer, and no-store headers.
- MCP is stdio-only; mutation requires the opt-in environment policy, an allowed scope, and `approved=true`. It is not a network authentication layer.
- Process execution uses fixed executables and separated arguments. Reviewed package, systemd, firewall, path, URL, webhook, and repository inputs are typed and validated.
- Secret scanning found no live tracked credential, private key, or token. Test values are synthetic and secret-returning surfaces use references or redaction.
- Durable JSON-lines stores serialize writers, reject unsafe final-component file types on Unix, and have concurrency/symlink regression tests.
- The local administrator token remains one coarse identity. Fine-grained persistent identities are not implemented and must not be claimed.

## Local validation

Passed on the 1.0.0 release-candidate tree:

- `cargo fmt --check`
- `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`
- `cargo test --locked --workspace --all-features` (76 tests on macOS; Linux-only coverage is compile-time excluded there)
- `cargo build --locked --workspace --release`
- `sh -n install.sh tests/*.sh docker/dev-entrypoint.sh`
- GitHub workflow YAML parsing

Linux-only smoke and live mutation claims are not inferred from this macOS run. The stable release workflow is the authoritative promotion gate for Ubuntu 22.04/24.04, Debian 12/13 installer behavior and the Ubuntu 24.04 managed-service/recipe scenarios.

## Remaining risks

- JSON-lines reads are memory-bounded but still scan the complete retained file; rotation and compaction are not implemented.
- State services other than operations use atomic replacement but do not yet use a common cross-process transaction helper. Their current ownership model must not be expanded to multiple writers without locking and contention coverage.
- Release artifacts currently target x86_64 only.
- Long-running soak, forced disk-full, abrupt-termination, complete self-update rollback, and real off-runner VPS recovery drills remain operational follow-up work.
- There is no database schema or migration layer in this repository; migration safety is not applicable.

## Recommended follow-up

1. Add audit/event rotation and retention policy.
2. Generalize the operations transaction primitive before introducing concurrent writers for another state service.
3. Add AArch64 release coverage before advertising that architecture.
4. Run periodic disposable-VPS install, update, rollback, backup-restore, disk-full, and abrupt-termination drills outside the release runner.
