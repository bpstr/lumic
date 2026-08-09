+++
title = "Attention and personality"
description = "One factual answer to how the node is doing, with optional deterministic personality."
weight = 18
[extra]
kicker = "STATUS"
status = "Epic G implemented"
+++

Lumic has one canonical attention report for the operator question: “How are you doing? Feeling good? Anything need attention?”

```bash
lumic how-are-you
lumic how-are-you --period-hours 72 --json
```

The report always contains a severity, evidence-backed facts, changes recorded in the selected period, current incidents, upcoming attention and recommendations. Live diagnosis supplies host load, memory, failed systemd units, filesystems and package updates. Lumic state supplies application health, TLS configuration, managed services, the latest backup outcome and durable events.

The history window affects only `changes`. A failed deployment event from yesterday is not called an active incident unless current host or application evidence still reports failure. Certificate expiry is not yet reported because the application state does not contain trustworthy expiry evidence; it is tracked as nightly expansion rather than guessed.

## Personality is presentation

```bash
lumic personality show
sudo lumic personality set dry
sudo lumic personality set professional
```

Available personalities are `professional`, `dry`, `grumpy`, `paranoid`, `cheerful` and `idiot`. The setting lives in private Lumic state, is written atomically and produces an audit record plus a change event. It affects only deterministic headings and phrasing around facts and recorded deployment/recovery/events.

All personalities render critical severity as `HEALTH: CRITICAL`, include every incident and upcoming item, and retain evidence and recommended actions. No LLM is required, and personality never changes structured data.

## Shared surfaces

- CLI: `lumic how-are-you`, with `--json` for automation.
- UI: the authenticated overview shows the same rendered report.
- MCP tool: `server_attention`, with an optional `period_hours` from 1 to 720.
- MCP resource: `lumic://server/attention`, using a 24-hour period.

The MCP response contains `personality`, `rendered`, and `summary`. Agents should treat `summary` as authoritative. The prose is convenient operator copy, not a separate diagnostic source.

## Failure and recovery

Attention reads are read-only. If live host diagnosis fails, the report fails instead of returning a reassuring partial answer. Invalid or symlinked personality state is rejected. A changed personality file retains the normal atomic-write recovery sibling, and the audit/event trail records who changed presentation; it cannot grant or alter operational permissions.

Current supported hosts are x86_64 Debian 12/13 and Ubuntu 22.04/24.04, matching Lumic's install-image matrix. `tests/epic-g-smoke.sh` verifies personality, factual JSON shape, real recorded change history and input bounds on each supported image.
