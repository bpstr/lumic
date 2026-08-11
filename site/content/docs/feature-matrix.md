+++
title = "Feature matrix"
description = "What Lumic 1.0 implements and what remains deliberate expansion work."
weight = 95
[extra]
kicker = "STATUS"
status = "Lumic 1.0"
+++

| Area | Implemented in 1.0 | Planned expansion / not implemented |
|---|---|---|
| Hosts | x86_64 Debian 12/13, Ubuntu 22.04/24.04; clean-image install smoke | aarch64 artifacts, other distributions |
| Install/update | stable/nightly channels; checksum-verified atomic install and self-update; backup and postflight restore | automated external real-VPS lifecycle gate |
| Host operations | typed apt/systemd, accounts, UFW, filesystems, processes, timers, updates, logs, backups, remediation | broad provider/remediation catalog; generic shell is intentionally absent |
| Applications | versioned strict `lumic.yaml` repository contract with reconciled Node/PHP versions, PHP extensions, Node package managers, typed service requirements, shared release paths, rich worker policy/health, cron, source/public paths, argv-only build/migrate hooks, health and deployment intent; application-scoped authenticated-encrypted environment values with masked MCP inspection, key-only diff, controlled set/rotate/delete and deployment-time injection; native Git push dispatch gated by the contract; immutable releases, deploy locks/cancellation, pinned retry/redeploy, commit metadata/log cursors, health-gated rollback, blue/green Node nginx handoff and drain, resource-bound Certbot/Let's Encrypt TLS | generic certificate mutation controls in every adapter, third-party PHP and Node repositories |
| Managed services | catalog-driven CLI/UI/MCP discovery, schemas, install/lifecycle, inspection and bindings; MySQL, PostgreSQL and Redis data operations; Typesense and Meilisearch credential bindings; native drivers for Valkey, RabbitMQ, MinIO, OpenSearch, Memcached, MongoDB, ClickHouse, Prometheus, Grafana, and Loki; pinned verified Gitea and Gogs drivers; independently owned nginx singleton | backup/restore and provider child resources for newer drivers; broader live-host coverage |
| Recipes | catalog-driven CLI/UI/MCP install, update and uninstall; versioned compiled static Git and checksum-pinned WordPress lifecycles; Laravel, Laravel + Typesense, Drupal, Symfony, Ghost and Matomo repository recipes; Forgejo catalog composition | executable Forgejo application driver; remote signed distribution |
| Infrastructure | namespaced group-shared managed bare Git repositories, external discovery/registration/adoption, explicit HTTPS/SSH remotes and credential references, fetch/push, authenticated Smart HTTP, Gitea/Gogs installers sharing the configured repository root, portable environment references, two-node identity/trust, signed deploy/rollback envelopes and restricted-SSH MCP access | fine-grained repository identities/grants; Forgejo managed driver and automatic forge metadata reconciliation; central fleet UI |
| Operations | correlated timeline, signed webhooks, bounded retry, one typed restart rule, backup verification | notification destinations, richer collectors and deterministic rules |
| Intelligence | Laravel fingerprint/config/dependency graph, `laravel-redis@1`, redacted incident context and advisory analysis | other framework/service definitions and evidence providers |
| Attention | canonical factual report and operations dashboard, six deterministic personalities, CLI/UI/MCP, application/service/deployment and CPU/RAM/disk facts, failed-service/security-update signals, certificate-expiry evidence, 24-hour backup-age policy, and recent incidents/events | configurable certificate/backup thresholds, optional language renderer |
| UI | authenticated loopback Rust UI and confirmed safe actions | persistent/fine-grained identities, remote authentication, mobile polish |
| Software catalog | UI/MCP status, plan and setup for the managed-service packages plus WordPress hosting prerequisites, PHP, nginx, Apache, Node.js and per-user NVM; full WordPress deployment through the separate recipe surface | automatic third-party repository enrollment |
| MCP | installed `lumic mcp serve` stdio, restricted-SSH onboarding, optional bearer-authenticated loopback Streamable HTTP, process scopes and per-call approval | OAuth, per-identity grants and automated TLS proxy setup |

Mechanism tests run in the workspace suite. Installation runs on every supported clean container image; MySQL/PostgreSQL/Redis, Laravel/Redis, and the idempotent WordPress lifecycle use live Ubuntu CI jobs. The source-tree acceptance scripts cover Epics A–G. A container image is not presented as a complete systemd VPS lifecycle: that external-host gate remains tracked nightly work.
