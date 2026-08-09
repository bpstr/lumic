+++
title = "Feature matrix"
description = "What the nightly actually implements and what remains deliberate expansion work."
weight = 95
[extra]
kicker = "STATUS"
status = "Epics A-G"
+++

| Area | Implemented nightly | Nightly expansion / not implemented |
|---|---|---|
| Hosts | x86_64 Debian 12/13, Ubuntu 22.04/24.04; clean-image install smoke | aarch64 artifacts, other distributions, stable channel |
| Install/update | checksum-verified atomic install; nightly self-update, backup and postflight restore | stable update policy; automated external real-VPS lifecycle gate |
| Host operations | typed apt/systemd, accounts, UFW, filesystems, processes, timers, updates, logs, backups, remediation | broad provider/remediation catalog; generic shell is intentionally absent |
| Applications | static/PHP release proof, Node foundation, nginx, TLS, health-gated deploy and rollback | arbitrary build hooks, database migrations, blue/green Node handoff |
| Managed services | PostgreSQL and Redis lifecycle/config/health/database/backup proof | MariaDB, search, queues, object storage and other providers |
| Recipes | versioned compiled `static-git@1.0.0` mechanism | broader reviewed application catalog; remote signed distribution |
| Infrastructure | native Git, portable environment references, two-node identity/trust and signed deploy/rollback envelopes | authenticated encrypted network MCP; central fleet UI |
| Operations | correlated timeline, signed webhooks, bounded retry, one typed restart rule, backup verification | notification destinations, richer collectors and deterministic rules |
| Intelligence | Laravel fingerprint/config/dependency graph, `laravel-redis@1`, redacted incident context and advisory analysis | other framework/service definitions and evidence providers |
| Attention | canonical factual report, six deterministic personalities, CLI/UI/MCP, latest failed-backup and update/disk/service/app signals | certificate-expiry evidence, backup-age policy, optional language renderer |
| UI | authenticated loopback Rust UI and confirmed safe actions | persistent/fine-grained identities, remote authentication, mobile polish |
| MCP | local stdio resources/tools, process scopes and per-call approval | installer-distributed MCP binary, `lumic mcp setup`, authenticated remote transport |

Mechanism tests run in the workspace suite. Installation runs on every supported clean container image; PostgreSQL/Redis and Laravel/Redis use live Ubuntu CI jobs. The source-tree acceptance scripts cover Epics A–G. A container image is not presented as a complete systemd VPS lifecycle: that external-host gate remains tracked nightly work.
