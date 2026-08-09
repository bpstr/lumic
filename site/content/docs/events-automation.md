+++
title = "Events & automation"
description = "Lumic actively observes server state, records events and can notify or run constrained remediation."
weight = 80
[extra]
kicker = "AUTOMATION"
status = "local event trail implemented; automation planned"
+++

Lumic is an active participant on the server, not a command that only wakes when invoked.

Events may include:

```text
server.booted
service.failed
deployment.started
deployment.succeeded
deployment.failed
certificate.expiring
certificate.renewal_failed
backup.completed
healthcheck.failed
healthcheck.recovered
disk.threshold_exceeded
repository.updated
```

The nightly implementation appends structured JSON events to a mode-0600 local JSON-lines store for package, systemd, application, repository, runtime, process, TLS, deployment, rollback, self-update and deletion mutations. `lumic events` shows concise output and `lumic events --json` returns the typed records. Actor, interface, entity, correlation ID, timestamp, and structured payload are retained. Generic webhook delivery is not implemented yet.

A separate private audit JSON-lines store records every attempted mutation, including failed native-tool operations, capability, operation, redacted arguments, before/after data and outcome. `lumic audit` and the read-only MCP `audit_list` tool expose newest-first records. Audit is durable local evidence, not an authorization mechanism by itself.

## Destinations

Generic webhooks are the foundation. Email, Slack, Discord, Teams and other notification channels can be layered on later.

## Remediation

Deterministic, policy-defined remediation is appropriate:

```text
service failure → restart → verify → notify on failure
```

Open-ended AI changes triggered automatically by load or errors are not the default safety model. Agents can inspect status, diagnosis, event and audit history, then plan and execute only explicitly permitted typed changes.
