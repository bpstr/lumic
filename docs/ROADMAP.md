# Implementation roadmap

The roadmap is ordered to protect the architecture: establish safe primitives and contracts first, then build vertical capabilities. Avoid broad placeholder APIs that have no tested implementation.

## Phase 0 — foundation (now)

- Rust workspace and crate boundaries.
- Host/OS facts and detection contracts.
- Operation result/error types.
- Capability/policy skeleton.
- CLI `version` and `status` backed by core/platform, not hard-coded presentation logic.
- Daemon lifecycle skeleton and clean shutdown.
- MCP crate/interface skeleton with first read-only host status resource/tool.
- Installer supporting fresh Debian/Ubuntu detection and local CI install mode.
- Secure remote-bootstrap threat model and protocol contract.
- Explicit rule that Lumic never reads/stores the VPS root password and never depends on a local Lumic client.
- Multi-image CI and nightly artifacts.
- Documentation/agent contract.

See [`BOOTSTRAP_SECURITY.md`](BOOTSTRAP_SECURITY.md). Do not implement convenience shortcuts that contradict that security contract.

## Phase 1 — secure remote access + trusted host operations

Remote access must become trustworthy before privileged agent operations become broad.

### Bootstrap / identity / MCP transport

- Local ephemeral bootstrap helper: `curl ... | sh -s -- root@IP`.
- Bootstrap input validation that cannot turn the SSH destination into shell/options injection.
- Password remains inside system OpenSSH; no password flags, env vars, `sshpass` or `expect`.
- Normal SSH host-key verification; optional explicit fingerprint pinning when available.
- Verified/signed release installation with fail-closed behavior.
- Node identity/state creation.
- Trusted HTTPS endpoint for IP-only nodes and automatic certificate renewal.
- Internal daemon/MCP listeners bound only to loopback/Unix socket; public management traffic through port 443.
- One-time initial-owner enrollment grant: >=256-bit CSPRNG, <=120 seconds, digest-only storage, purpose bound, atomic single-use consumption.
- HTTPS bootstrap exchange endpoint with strict logging redaction, request limits and rate limiting.
- First-owner/device enrollment and revocation model.
- Streamable HTTP MCP endpoint at `https://IP/mcp`.
- MCP-standard OAuth authorization with PKCE, scoped clients, short-lived access tokens and revocable/rotating refresh credentials.
- SSH/stdio MCP fallback when trusted public TLS cannot be established; never weaken certificate verification.
- Client registration helpers for supported Codex/Claude flows without reverse-engineering their credential stores.
- Bootstrap/replay/concurrency/TLS/OAuth/scope-escalation security tests.

### Host operations

- Safe process executor with separated args, timeouts, output limits and audit metadata.
- apt adapter: update/search/install/remove/version with allowlist policy.
- systemd adapter: inspect/start/stop/restart/reload/enable/disable.
- package/component catalog format.
- host CPU/memory/disk/load/process/service inspection.
- structured events and local audit store.
- `lumic diagnose` v1.

Phase 1 is not complete if MCP can authenticate but authenticated clients bypass capability policy. Authentication and authorization remain separate layers.

## Phase 2 — runtimes and web applications

- nginx managed service + config validation/reload.
- PHP runtime + versioning + extension components.
- Node runtime.
- application domain/model and environment handling.
- Git source + credentials model.
- local bare Git repository hosting.
- domains and TLS.
- workers and scheduled jobs.

## Phase 3 — deployments

- release directory model and retention.
- build lifecycle.
- atomic PHP/static activation.
- long-running process blue/green or start/health/switch/drain strategy.
- health checks and automatic rollback.
- deployment events/history.
- GitHub/GitLab webhook adapters.

## Phase 4 — managed data/services

- PostgreSQL.
- MariaDB.
- Redis/Valkey.
- database/user/backup/restore primitives.
- Typesense/Meilisearch and Agnative as managed services.
- backup destinations and schedules.

## Phase 5 — UI and operator experience

- Rust UI shell using the same owner/session authorization model created by secure bootstrap.
- server overview, applications, services, deployments, logs/events.
- device/session/client authorization and revocation views.
- progressive expert details.
- notifications/webhook configuration.
- policy/approval views.

UI may begin earlier once core read models are stable, but must remain an adapter.

## Phase 6 — recipes and infrastructure

- recipe catalog and signed/versioned distribution.
- Laravel, Symfony, WordPress, Drupal, Forgejo/Gitea and other application recipes.
- node roles (app/worker/cache/db/git/media/backup/edge).
- environment export/import and clone/transform workflows.
- explicit multi-node relationships/topology.

## Phase 7 — advanced operations

- richer tracing/correlation reports.
- deterministic remediation rules.
- security hardening profiles.
- optional transactional SSH hardening after replacement access has been independently verified.
- optional Hub/fleet layer.
- container workload support and container-specific diagnostics where valuable.

## Definition of done for any capability

Implementation + tests + CLI/API/MCP mapping where relevant + policy + audit/events + documentation + supported-OS CI. No feature is “done” because a command happens to work once on the developer machine.

For bootstrap/authentication work, “done” additionally means threat-model tests, secret-redaction tests, replay/concurrency tests, TLS failure tests, revocation tests and explicit lockout recovery behavior.