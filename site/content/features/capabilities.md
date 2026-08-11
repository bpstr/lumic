+++
title = "Capabilities"
description = "Host, deployment, infrastructure, automation, and agent operations."
weight = 30
[extra]
kicker = "CATALOG"
status = "current nightly"
+++

Lumic wraps trusted Linux mechanisms with validation, policy, plans, locks, health checks, and audit data. The normal interface is a typed domain operation, not unrestricted shell execution.

| Area | Implemented capability |
|---|---|
| Host operations | Packages, systemd services, users, UFW, filesystems, processes, timers, updates, logs, diagnostics, and backups. |
| Application delivery | Git-backed releases, runtime and web-host resources, TLS attachment, workers, schedules, health gates, retention, and rollback. |
| Git repositories | Managed bare repositories, import, discovery, registration, adoption, remotes, fetch, push, Smart HTTP, and push-to-deploy configuration. |
| Infrastructure | Portable environment references, node identity and trust, signed deployment envelopes, and autonomous multi-node coordination. |
| Operations | Durable events, correlated timelines, signed webhooks, retry policy, constrained restart remediation, and backup verification. |
| Server intelligence | Evidence-backed Laravel detection, dependency impact, safe Redis wiring, incident context, and advisory analysis. |
| Attention | One factual node report with deterministic presentation styles across CLI, UI, and MCP. |
| Interfaces | CLI, authenticated loopback Rust UI, HTTP read models, stdio MCP, restricted-SSH MCP, and authenticated loopback Streamable HTTP MCP. |
| Safety | Separate status, suggestion, plan, and apply boundaries; input validation; allowlists; idempotency; resource locks; audit; and recovery guidance. |

See the [Feature matrix](@/docs/feature-matrix.md) for supported operating systems and explicit planned work.
