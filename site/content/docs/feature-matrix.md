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
| Applications | static/PHP release proof, explicit apt-available PHP 8.1–8.4 runtimes and extensions, runtime-bound owned nginx web hosts, Node foundation, resource-bound Certbot/Let's Encrypt TLS with attachment rollback, health-gated deploy/rollback, UI/MCP pipeline progress and failure inspection | generic certificate mutation controls in every adapter, third-party PHP repositories, arbitrary build hooks, database migrations, blue/green Node handoff |
| Managed services | catalog-driven CLI/UI/MCP discovery, schemas, install/lifecycle, inspection and bindings; MySQL, PostgreSQL and Redis config/health/database/backup; Typesense and Meilisearch config/health with generated credentials and reusable endpoint bindings; independently owned nginx singleton and recoverable web-host resources | search backup/restore, queues, object storage and other providers |
| Recipes | versioned compiled `static-git@1.0.0` mechanism; checksum-pinned, idempotent `wordpress@1.0.0` lifecycle proof | broader reviewed application catalog; remote signed distribution |
| Infrastructure | native Git, portable environment references, two-node identity/trust, signed deploy/rollback envelopes and restricted-SSH MCP access | OAuth/per-identity MCP policy; central fleet UI |
| Operations | correlated timeline, signed webhooks, bounded retry, one typed restart rule, backup verification | notification destinations, richer collectors and deterministic rules |
| Intelligence | Laravel fingerprint/config/dependency graph, `laravel-redis@1`, redacted incident context and advisory analysis | other framework/service definitions and evidence providers |
| Attention | canonical factual report, six deterministic personalities, CLI/UI/MCP, latest failed-backup and update/disk/service/app signals | certificate-expiry evidence, backup-age policy, optional language renderer |
| UI | authenticated loopback Rust UI and confirmed safe actions | persistent/fine-grained identities, remote authentication, mobile polish |
| Software catalog | UI/MCP status, plan and setup for WordPress hosting prerequisites, PHP, MySQL, PostgreSQL, Redis, Typesense, Meilisearch, nginx, Apache, Node.js and per-user NVM; full WordPress deployment through the separate recipe surface | automatic third-party repository enrollment |
| MCP | installed `lumic mcp serve` stdio, restricted-SSH onboarding, optional bearer-authenticated loopback Streamable HTTP, process scopes and per-call approval | OAuth, per-identity grants and automated TLS proxy setup |

Mechanism tests run in the workspace suite. Installation runs on every supported clean container image; MySQL/PostgreSQL/Redis, Laravel/Redis, and the idempotent WordPress lifecycle use live Ubuntu CI jobs. The source-tree acceptance scripts cover Epics A–G. A container image is not presented as a complete systemd VPS lifecycle: that external-host gate remains tracked nightly work.
