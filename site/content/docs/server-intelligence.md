+++
title = "Server intelligence"
description = "Evidence-backed application detection, safe service wiring, dependency impact and incident context."
weight = 47
[extra]
kicker = "INTELLIGENCE"
status = "Epic F implemented"
+++

Lumic can understand a deployed application and its host relationships without embedding an unrestricted agent in the daemon. Discovery is deterministic, plans remain separate from apply, and every mutation composes existing typed application, managed-service and systemd operations.

## Inspect before changing

```bash
lumic intelligence fingerprint shop
lumic intelligence config shop
lumic intelligence graph shop
lumic intelligence catalog
```

Fingerprint output includes evidence and confidence. The Laravel detector requires both `laravel/framework` in `composer.json` and an `artisan` entry point for high confidence. Configuration inspection returns dotenv key names, duplicate-key warnings and sensitivity labels; it never returns values. Discovery files must be regular files no larger than 1 MiB inside the managed application root.

## Laravel + Redis reference

```bash
lumic intelligence plan shop
lumic intelligence apply shop
```

`laravel-redis@1` is intentionally the only compiled integration definition. The plan selects an existing Redis service, or plans a managed service named `redis`; previews only configured/unset states for `REDIS_HOST`, `REDIS_PORT`, cache, session and queue keys; shows affected queue/Horizon workers; and includes a dependency graph, risks, validation and recovery.

The CLI defaults `--integration` to `laravel-redis@1`, while CLI and MCP both accept the versioned definition ID so future catalog entries can use the same plan/apply dispatch without changing the contract.

Apply requires the high-confidence fingerprint and a duplicate-free active `.env`. It installs/selects loopback-only managed Redis, creates a private content-hashed snapshot, atomically updates the dotenv file while preserving unrelated comments, restarts only affected worker units, verifies Redis and the application's configured health check, and finally attaches the typed cache reference. A failed verification restores the dotenv snapshot and restarts workers that had already accepted the new environment. A newly installed healthy Redis service is retained as a managed resource rather than destructively removed during recovery. Redis credentials are not generated for the loopback-only reference because they are not required; authenticated future service adapters must use private secret references.

The apply result returns a snapshot ID. Restore it with:

```bash
lumic intelligence rollback shop cfg-<timestamp>-<digest>
```

Rollback accepts only a Lumic-owned snapshot whose application, active target path and SHA-256 content digest all match.

## Incident context and optional analysis

```bash
lumic intelligence incident --app shop --since-ms <unix-ms>
lumic intelligence analyze incident-adapter --app shop --since-ms <unix-ms>
```

Incident context combines the factual operations timeline with affected dependency nodes and evidence IDs. Sensitive payload fields are recursively replaced with `[redacted]`; the signal count and outbound JSON payload are bounded. The report presents evidence and findings, not an invented root cause.

Analysis uses an enabled webhook destination configured through `lumic operations webhook-apply`. Lumic signs the request with that destination's secret reference and accepts only a closed JSON response:

```json
{
  "diagnosis": "Evidence-backed explanation",
  "evidence_references": ["signal-id"],
  "proposed_remediations": [
    { "kind": "restart_service", "unit": "redis-server.service" }
  ],
  "advisory": true
}
```

Evidence IDs must come from the supplied context. Proposed actions may only be typed service restart or owned application-configuration rollback. Lumic forces `advisory` to true and executes nothing; plan and apply the appropriate normal capability separately. MCP additionally requires `application.integrate` for integration apply/rollback and `incident.analyze` plus approval before any context leaves the node.

## Current scope

Laravel + Redis is the mechanism proof. Laravel + Typesense, additional Laravel services, Drupal and other framework/service definitions are explicitly deferred to nightly work. There is no generic integration DSL, shell hook or in-daemon LLM runtime.
