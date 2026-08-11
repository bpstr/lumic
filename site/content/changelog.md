+++
title = "Changelog"
description = "Operator-visible changes from stable releases and gated nightly builds."
template = "changelog.html"
+++

The changelog tracks shipped behavior, not every commit. Nightly entries are dated, concise, and linked to the documentation that defines the current contract.

## 2026-08-11 · 2.0.0-alpha.2

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
