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
- Multi-image CI and nightly artifacts.
- Documentation/agent contract.

## Phase 1 — trusted host operations

- Safe process executor with separated args, timeouts, output limits and audit metadata.
- apt adapter: update/search/install/remove/version with allowlist policy.
- systemd adapter: inspect/start/stop/restart/reload/enable/disable.
- package/component catalog format.
- host CPU/memory/disk/load/process/service inspection.
- structured events and local audit store.
- `lumic diagnose` v1.

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

- Rust UI shell and authentication.
- server overview, applications, services, deployments, logs/events.
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
- optional Hub/fleet layer.
- container workload support and container-specific diagnostics where valuable.

## Definition of done for any capability

Implementation + tests + CLI/API/MCP mapping where relevant + policy + audit/events + documentation + supported-OS CI. No feature is “done” because a command happens to work once on the developer machine.
