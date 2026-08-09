+++
title = "Events & automation"
description = "Lumic actively observes server state, records events and can notify or run constrained remediation."
weight = 80
[extra]
kicker = "AUTOMATION"
status = "Epic E mechanism implemented"
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

Lumic keeps two related records. The original mode-0600 event and audit JSON-lines stores retain mutations. The operations service imports those events and takes five-minute host/process/application/managed-service snapshots while `lumicd` is running. It also ingests failed systemd units and selected kernel journal evidence (`OOM`, killed process, panic and I/O errors), and accepts bounded typed provider signals. The correlated timeline is durable at `operations/timeline.jsonl` below the state directory.

```bash
sudo lumic operations capture
sudo lumic operations observe
sudo lumic operations timeline --entity-id demo.service --limit 100
sudo lumic operations incident --entity-id demo.service --since-ms <unix-ms>
sudo lumic operations provider-signal provider.failed provider demo \
  --severity error --summary "reference provider failure" --payload '{"check":"failed"}'
```

Incident reports contain the query window, affected typed resources, the matching evidence, factual failure findings and safe next-step guidance. They do not claim an inferred root cause. Provider payloads are limited to 64 KiB; outbound structured envelopes are limited to 256 KiB. Lumic event payloads are expected to contain references rather than secret values.

A separate private audit JSON-lines store records every attempted mutation, including failed native-tool operations, capability, operation, redacted arguments, before/after data and outcome. `lumic audit` and the read-only MCP `audit_list` tool expose newest-first records. Audit is durable local evidence, not an authorization mechanism by itself.

## Destinations

Generic webhooks are the foundation. Email, Slack, Discord, Teams and other notification channels can be layered on later.

Create a private secret reference, preview/apply an HTTPS destination, then subscribe it to exact events:

```bash
sudo lumic environment secret-generate incident-key
sudo lumic operations webhook-plan incident-hook https://ops.example.test/lumic incident-key
sudo lumic operations webhook-apply incident-hook https://ops.example.test/lumic incident-key
sudo lumic operations subscribe failures incident-hook --event service.failed --event automation.recovered
sudo lumic operations deliveries
```

Loopback HTTP is allowed only for local acceptance testing; remote destinations require HTTPS. Each POST is JSON with schema `lumic.webhook.v1`, delivery ID and the complete structured signal. `X-Lumic-Signature` is `sha256=<HMAC-SHA256>` and `X-Lumic-Delivery` is stable for the delivery. The secret is read locally and never sent as an argument or returned. Timeout is 100–30000 ms, attempts are bounded to 1–8, retry delay is exponential, and delivered/exhausted history is retained. `lumicd` processes due deliveries every 30 seconds; `lumic operations run-once` is the deterministic manual equivalent. `lumic operations observe` bypasses the five-minute snapshot gate for a deliberate acceptance check and may activate an approved rule.

## Remediation

Deterministic, policy-defined remediation is appropriate:

```text
service failure → restart → verify → notify on failure
```

Open-ended AI changes triggered automatically by load or errors are not the default safety model. Agents can inspect status, diagnosis, event and audit history, then plan and execute only explicitly permitted typed changes.

The implemented reference rule is deliberately narrow:

```bash
sudo lumic operations rule-plan restart-demo service.failed demo.service \
  --entity-id demo.service --cooldown-seconds 60 --max-attempts 2
sudo lumic operations rule-apply restart-demo service.failed demo.service \
  --entity-id demo.service --cooldown-seconds 60 --max-attempts 2
```

It invokes Lumic's existing typed systemd restart operation (never a shell string), respects a 5–86400 second cooldown and 1–3 attempt limit, then verifies `active_state=active`. The trigger and remediation share a correlation ID. The plan exposes the target as an impact-preview hook; richer dependency expansion is nightly breadth.

Webhook destinations, subscriptions and rules use a private atomic state file. Each configuration apply retains the prior sibling snapshot; `sudo lumic operations rollback-configuration` restores it. Runtime delivery updates preserve that recovery snapshot. Native application/service configuration rollback and checksum-verified self-update recovery remain the mutation-specific safety layers.

Current reference coverage is Debian/Ubuntu with systemd, procfs and journald. Missing/unavailable kernel journal reads fail closed without blocking the rest of the cycle. Slack/email/Discord adapters, richer metric/provider collectors and additional deterministic rules are intentionally deferred to nightly issues [#37](https://github.com/bpstr/lumic/issues/37), [#38](https://github.com/bpstr/lumic/issues/38) and [#39](https://github.com/bpstr/lumic/issues/39).
