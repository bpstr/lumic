+++
title = "Managed services"
description = "Install and operate native data, queue, storage, search, metrics, and logging services."
weight = 50
[extra]
kicker = "SERVICES"
status = "17 managed-service integrations implemented"
+++

Managed services are long-lived native capabilities with identity, desired configuration, lifecycle, health, logs, data operations and recovery—not aliases for every apt package. The built-in driver set covers MySQL, PostgreSQL, Redis, Typesense, Meilisearch, Valkey, RabbitMQ, MinIO, OpenSearch, Memcached, MongoDB, ClickHouse, Prometheus, Grafana, Loki, Gitea, and Gogs on Debian/Ubuntu. nginx is also independently owned as the singleton `nginx.main` service and provides validated, recoverable application web-host resources. CLI, UI, and MCP expose the shared trusted catalog and configuration schemas rather than maintaining provider lists in each adapter.

Internally, this surface runs through a versioned built-in service catalog and trusted Rust driver registry. Catalog definitions own platform mappings, typed configuration and child-resource schemas, and may map fields to reviewed INI/directive files or direct argv command targets. The generic engine validates and renders those targets without a shell; compiled drivers still own privileged execution, atomic writes, ownership/modes, health, data operations, and recovery. Repository manifests may consume these typed APIs but cannot declare target paths or commands. Stable resource IDs, explicit external-versus-Lumic ownership, typed outputs/bindings, journaled pipelines and cross-process mutation locks form the shared contract. Other catalog entries are not presented as fully managed services until their drivers and recovery paths are complete.

The private, schema-versioned `resources.json` store is authoritative. Existing installations with `managed-services.json` are migrated once after Lumic preserves the exact legacy file as `managed-services.v1.json`; new operations do not write the legacy path.

## Status, plan and apply

Detection is read-only and does not adopt an existing package:

```bash
lumic managed-service catalog
lumic managed-service schema postgresql
lumic managed-service detect postgresql
lumic managed-service detect mysql
lumic managed-service detect redis
lumic managed-service detect typesense
lumic managed-service detect meilisearch
lumic managed-service detect valkey
lumic managed-service detect opensearch
lumic managed-service detect prometheus
```

Installation keeps plan and mutation separate:

```bash
lumic managed-service plan-install primary-db postgresql
sudo lumic managed-service install primary-db postgresql
sudo lumic managed-service install mysql mysql
sudo lumic managed-service install cache redis
sudo lumic managed-service install search typesense
sudo lumic managed-service install alternate-search meilisearch
sudo lumic managed-service install queue rabbitmq
sudo lumic managed-service install object-storage minio
sudo lumic managed-service install metrics prometheus
sudo lumic managed-service install git-forge gitea
lumic managed-service inspect primary-db
```

Install uses either Lumic's approved apt catalog or a driver's pinned verified upstream artifact, writes provider configuration atomically, enables/restarts the systemd unit and requires a provider health probe before persisting managed state. Repeating the operation reconciles the same resource. All seventeen services bind to loopback by default; non-loopback exposure is rejected.

Typesense and Meilisearch require `typesense-server` or `meilisearch` to have a candidate in an apt source already configured and trusted by the operator. Lumic does not enroll a third-party repository or key. Each install generates a private 256-bit credential in Lumic's mode-0600 secret store. Typesense writes `/etc/typesense/typesense-server.ini`; Meilisearch writes a mode-0600 `/etc/meilisearch.env` and a hardened Lumic-owned systemd unit. Health checks call the loopback `/health` endpoint without putting the credential in process arguments. Search backup/restore and database/user commands are not supported by these drivers.

The additional drivers use these native contracts:

| Driver ID | Package and unit | Managed setting or secret | Health gate |
| --- | --- | --- | --- |
| `valkey` | `valkey-server` / `valkey-server.service` | memory limit and eviction policy | `valkey-cli PING` |
| `rabbitmq` | `rabbitmq-server` / `rabbitmq-server.service` | memory high-water mark | `rabbitmq-diagnostics ping` |
| `minio` | `minio` / Lumic-owned `minio.service` | console port, generated root user/password | loopback live endpoint |
| `opensearch` | `opensearch` / `opensearch.service` | cluster name | loopback cluster-health endpoint |
| `memcached` | `memcached` / `memcached.service` | memory limit | active systemd state |
| `mongodb` | `mongodb-org` / `mongod.service` | bind address and port | `mongosh` ping |
| `clickhouse` | `clickhouse-server` / `clickhouse-server.service` | bind address and HTTP port | loopback `/ping` |
| `prometheus` | `prometheus` / `prometheus.service` | scrape interval | loopback healthy endpoint |
| `grafana` | `grafana` / `grafana-server.service` | generated admin password | loopback API health |
| `loki` | `loki` / `loki.service` | retention period | loopback readiness endpoint |

RabbitMQ, Memcached, and Prometheus use distribution packages. Valkey, MinIO, OpenSearch, MongoDB, ClickHouse, Grafana, and Loki require a candidate from an apt source the operator has already configured and trusted. Lumic validates the candidate but does not enroll repositories or keys. MinIO receives a hardened Lumic-owned unit, Prometheus receives an explicit bind-aware systemd override, and secret-bearing MinIO/Grafana configuration is written with restricted permissions. OpenSearch package setup disables the packaged demo-security configuration, then runs single-node with its security plugin disabled; this requires a current package that supports those documented installation flags and is deliberately restricted to loopback. Exposing it directly is not supported.

These ten drivers provide install, validated configuration, lifecycle, logs, inspection, update, removal, and health. Provider-native backup/restore and child database/user operations are explicitly unsupported until a recovery contract exists. Their catalog mappings and renderers have workspace unit coverage; the clean-image and live managed-service CI gates remain limited to the providers named in the installation support matrix.

## Git forge services

Gitea and Gogs are artifact-backed managed services for `x86_64` and `aarch64` Debian/Ubuntu hosts. Lumic downloads a pinned upstream release over HTTPS, verifies its SHA-256 digest through the shared artifact manager, installs a dedicated system user and hardened systemd unit, and stores private forge metadata in `/var/lib/gitea` or `/var/lib/gogs`. The HTTP service is loopback-only by default, the built-in SSH server is disabled, and exposure through a reviewed reverse-proxy host remains an explicit operator action.

```bash
lumic managed-service plan-install git-forge gitea
sudo lumic managed-service install git-forge gitea
# Or choose Gogs instead; only one forge may own a repository root.
sudo lumic managed-service install git-forge gogs
lumic managed-service inspect git-forge
```

Both drivers use the exact `git.repository_root` from Lumic configuration. Installation creates the `lumic-git` group, reconciles existing repository ownership and group-write permissions, and runs the forge with that shared group. Lumic-created and imported bare repositories use Git's group-sharing mode. The forge keeps its own SQLite metadata, so an existing filesystem repository does not automatically become a forge database record; register or import it through the selected forge when UI visibility is required. Running Gitea and Gogs concurrently over the same root is rejected.

Update reconciles the pinned verified release. Removal stops and disables the unit and removes the installed binary, while retaining configuration, secrets, forge data, and repositories for recovery. Provider-native backup/restore and child-resource operations are not yet exposed for these drivers; back up both the forge data directory and Lumic repository root before maintenance.

## Lifecycle, configuration and logs

```bash
sudo lumic managed-service configure primary-db --port 5432 --setting max_connections=150
sudo lumic managed-service restart primary-db
lumic managed-service logs primary-db --lines 200
sudo lumic managed-service update primary-db --dry-run
sudo lumic managed-service remove primary-db --dry-run
```

An explicit restart clears systemd's failed/start-rate counter for that unit before requesting the restart. This keeps repeated, successful configuration reconciliations from making the next operator-approved restart fail with `start-limit-hit`.

The definition argument is a stable catalog ID, not a closed CLI enum. Adding a reviewed compiled
driver and catalog definition therefore does not require a new CLI command shape. The existing
provider-specific configure and data commands remain available where the selected definition
declares those capabilities.

Only provider-specific allowlisted settings are accepted. A material configuration change is written atomically, restarted, health-checked and rolled back to the previous file—or removed if Lumic created it—when validation fails. Removal is rejected while a database, credential, or search endpoint still has an application binding. Otherwise it stops/disables the unit and removes the native package or verified forge binary while deliberately retaining service data and generated service credentials for recovery; data purge is not bundled into this command.

Dependencies between managed resources are explicit metadata for status and later impact analysis:

```bash
lumic managed-service declare-dependency primary-db cache \
  --purpose "application cache"
```

## Relational database primitives

```bash
sudo lumic managed-service user-create primary-db app_user
sudo lumic managed-service database-create primary-db app_db --owner app_user
sudo lumic managed-service grant primary-db app_db app_user
sudo lumic managed-service backup primary-db --database app_db
sudo lumic managed-service backup-verify <backup-id>
sudo lumic managed-service restore primary-db <backup-id>
```

Database and role identifiers are strictly validated. Passwords are generated locally, passed to the provider's native SQL client over stdin, stored in Lumic's private mode-0600 secret store and represented in output only by an opaque secret reference.

MySQL uses the same command surface, with database ownership expressed through an explicit grant rather than `--owner`:

```bash
sudo lumic managed-service database-create mysql app_primary
sudo lumic managed-service user-create mysql app_primary_user
sudo lumic managed-service grant mysql app_primary app_primary_user
sudo lumic managed-service backup mysql --database app_primary
```

MySQL credentials are passed to the local socket client over stdin. Persisted credential outputs contain only a sensitive `secret://<reference>` value; plaintext is not stored in application or resource state.

Redis backup creates a local snapshot; restore stops Redis, replaces its data file with native ownership, restarts it and verifies health. MySQL uses `mysqldump`/`mysql`; PostgreSQL uses `pg_dump`/`pg_restore`. New backup records include SHA-256; `backup-verify` checks existence, recorded size, checksum (when present) and the native SQL, `REDIS`, or `PGDMP` header before restore. Backup records stay in Lumic's private state and backup files live below `/var/backups/lumic`; this is a local reference implementation, not an off-node disaster-recovery claim.

## Application references

```bash
lumic managed-service attach primary-db my-app \
  --role database --database app_db --user app_user

lumic managed-service attach search my-app --role search
```

Application metadata stores the service, role, database/user identity and secret reference, never the password. Unknown services, databases or users are rejected. Wiring those references into application environment files belongs to the later integration-intelligence mechanism.

An attachment with both a database and user requires a recorded grant. Database and credential outputs are bound separately to role-scoped application inputs, so an application can attach multiple isolated databases without one role replacing another. A Typesense or Meilisearch attachment accepts no database/user flags and binds its reusable `http` endpoint plus sensitive `api_key` or `master_key` reference to `<role>_endpoint` and `<role>_credential`. Those bindings prevent removal of the search service until the application relationship is detached.

Every mutation emits a managed-service event and audit record with actor, interface and correlation data. The same behavior is exposed through CLI, UI and policy-gated MCP tools. Further provider breadth is deliberately tracked as nightly work.
