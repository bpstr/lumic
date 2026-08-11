+++
title = "Changelog"
description = "Operator-visible changes from stable releases and gated nightly builds."
template = "changelog.html"
+++

The changelog tracks shipped behavior, not every commit. Nightly entries are dated, concise, and linked to the documentation that defines the current contract.

## 2026-08-12 · 2.0.0-alpha.10

- Required release candidates to pass all GitHub push workflows before receiving a version tag; individual repair commits are no longer treated as releases.
- Resolved Node blue/green systemd startup by rendering the managed runtime executable as an absolute path.
- Expanded installer validation across every managed-service definition and plan, plus reusable live PostgreSQL, MySQL, and Redis lifecycle checks with machine-readable CI results.
- Corrected Redis eviction-policy rendering so managed `maxmemory_policy` updates restart cleanly with Redis's native `maxmemory-policy` directive.
- Kept ordinary application deployments independent of the root-only runtime environment directory; protected environment files are now materialized only for persistent processes and blue/green Node handoffs that consume them.
- Made nginx validation independent of an interactive root `PATH`, including application provisioning and certificate attachment.
- Enforced the exact [`lumic.yaml`](@/docs/lumic-yaml.md) Node/PHP version, PHP extension, package-manager, and typed managed-service intent during apply and deployment; added persistent shared release paths and richer supervised worker configuration.
- Expanded the built-in application catalog with Laravel, Drupal, Symfony, Forgejo, Ghost, Matomo, and framework/service combinations, and completed recipe lifecycle actions in the operator UI alongside CLI and MCP.
- Added SSH private-key authentication to managed repository import/fetch/push with operation-scoped decrypted identity files and isolated OpenSSH configuration.

## 2026-08-11 · 2.0.0-alpha.3

- Added the strict, versioned [`lumic.yaml`](@/docs/lumic-yaml.md) repository contract for runtimes, source/public paths, builds, workers, cron, service requirements, health, migrations, and deployment behavior across CLI, MCP, and the contract-gated Git push release path.
- Added serialized production deployments with explicit pre-deploy, build, database migration, activation, health, post-deploy and drain phases.
- Added blue/green Node release units and atomic nginx upstream handoff, cooperative cancellation, pinned retry/redeploy, persistent log cursors, and Git commit provenance. See [Deployments](@/docs/deployments.md).
- Added application-scoped encrypted environment values with masked inspection, controlled set/rotate/delete operations, deployment-time injection, and deployment-log redaction. See [Applications](@/docs/applications.md).
- Turned the UI overview into an operations dashboard backed by the shared attention verdict, including certificate-expiry and latest-backup-age evidence.
- Added first-class managed and external Git repositories with discovery, adoption, remotes, synchronization, authenticated Smart HTTP, and deployment configuration.
- Added pinned, verified Gitea and Gogs managed-service drivers that share Lumic's configured repository root.
- Added repository status and create/import workflows to CLI, UI, and MCP.
- Hardened concurrent repository state updates, decoded state and lock-file handling, privileged dependency installation, and outbound webhook destination validation.
- Added this dedicated Features catalog for applications, services, and host capabilities.

## 2026-08-10 · Nightly

- Added native managed-service drivers for Valkey, RabbitMQ, MinIO, OpenSearch, Memcached, MongoDB, ClickHouse, Prometheus, Grafana, and Loki.
- Added typed resource orchestration, stable resource identities, shared locks, and durable pipeline progress.
- Expanded live MySQL coverage and versioned PHP package policy.
- Restored the `2.0.0-alpha.1` forward development line after the `1.0.0` release.

## 2026-08-10 · 1.0.0

- Published the first stable Lumic Control Center release contract.
- Coordinated stable installation, update, artifacts, version reporting, and public documentation.

## 2026-08-09 · Nightly

- Completed the single-VPS application platform, managed services, WordPress recipe, host operations, native Git and multi-node infrastructure foundations.
- Added active operations, server intelligence, the canonical attention report, and deterministic personality styles.
- Added the authenticated Rust operator UI and shared CLI/UI/MCP capability model.

For channel behavior and quality gates, see [Nightly](@/docs/nightly.md). For exact current scope and limitations, see the [Feature matrix](@/docs/feature-matrix.md).
