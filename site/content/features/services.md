+++
title = "Managed services"
description = "Native service integrations with typed lifecycle and health contracts."
weight = 20
[extra]
kicker = "CATALOG"
status = "current nightly"
+++

Managed services are native host resources, not containers or generic package aliases. Every listed integration has a trusted catalog entry and compiled driver; depth varies by provider.

| Group | Services | Current scope |
|---|---|---|
| Databases | MySQL, PostgreSQL | Lifecycle, health, configuration, databases, users, grants, backup, restore, and application bindings. |
| Cache | Redis | Lifecycle, health, configuration, backup, restore, and application bindings. |
| Search | Typesense, Meilisearch, OpenSearch | Native lifecycle and health; Typesense and Meilisearch also publish credential bindings. |
| Queue | RabbitMQ | Native lifecycle, configuration, logs, inspection, update, and removal. |
| Object storage | MinIO | Verified native artifact, lifecycle, configuration, logs, inspection, update, and removal. |
| Data platforms | MongoDB, ClickHouse | Native lifecycle, configuration, logs, inspection, update, and removal. |
| Cache alternatives | Valkey, Memcached | Native lifecycle, configuration, logs, inspection, update, and removal. |
| Observability | Prometheus, Grafana, Loki | Native lifecycle, configuration, logs, inspection, update, and removal. |
| Git forges | Gitea, Gogs | Pinned verified artifacts, hardened systemd services, loopback HTTP, and a shared managed repository root. |
| Web | nginx | Independently owned singleton with validated, recoverable application web-host resources. |

Newer drivers do not yet claim provider-native backup, restore, or child-resource operations. See [Managed services](@/docs/services.md) for provider details, restrictions, and recovery behavior.
