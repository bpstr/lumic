+++
title = "Managed services"
description = "Install and operate PostgreSQL and Redis as first-class native resources."
weight = 50
[extra]
kicker = "SERVICES"
status = "PostgreSQL and Redis reference integrations implemented"
+++

Managed services are long-lived native capabilities with identity, desired configuration, lifecycle, health, logs, data operations and recovery—not aliases for every apt package. The first implemented providers are PostgreSQL and Redis on Debian/Ubuntu.

## Status, plan and apply

Detection is read-only and does not adopt an existing package:

```bash
lumic managed-service detect postgresql
lumic managed-service detect redis
```

Installation keeps plan and mutation separate:

```bash
lumic managed-service plan-install primary-db postgresql
sudo lumic managed-service install primary-db postgresql
sudo lumic managed-service install cache redis
lumic managed-service inspect primary-db
```

Install uses Lumic's approved apt catalog, writes provider configuration atomically, enables/restarts the native systemd unit and requires a provider health probe (`pg_isready` or `redis-cli PING`) before persisting managed state. Repeating the operation reconciles the same resource. Both references bind to loopback by default; non-loopback exposure is rejected.

## Lifecycle, configuration and logs

```bash
sudo lumic managed-service configure primary-db --port 5432 --setting max_connections=150
sudo lumic managed-service restart primary-db
lumic managed-service logs primary-db --lines 200
sudo lumic managed-service update primary-db --dry-run
sudo lumic managed-service remove primary-db --dry-run
```

Only provider-specific allowlisted settings are accepted. A material configuration change is written atomically, restarted, health-checked and rolled back to the previous file—or removed if Lumic created it—when validation fails. Removal stops/disables the unit and removes the native package while deliberately retaining service data for recovery; data purge is not bundled into this command.

Dependencies between managed resources are explicit metadata for status and later impact analysis:

```bash
lumic managed-service declare-dependency primary-db cache \
  --purpose "application cache"
```

## PostgreSQL data primitives

```bash
sudo lumic managed-service user-create primary-db app_user
sudo lumic managed-service database-create primary-db app_db --owner app_user
sudo lumic managed-service grant primary-db app_db app_user
sudo lumic managed-service backup primary-db --database app_db
sudo lumic managed-service backup-verify <backup-id>
sudo lumic managed-service restore primary-db <backup-id>
```

Database and role identifiers are strictly validated. Passwords are generated locally, passed to `psql` over stdin, stored in Lumic's private mode-0600 secret store and represented in output only by an opaque secret reference.

Redis backup creates a local snapshot; restore stops Redis, replaces its data file with native ownership, restarts it and verifies health. PostgreSQL backup/restore uses local `pg_dump`/`pg_restore`. New backup records include SHA-256; `backup-verify` checks existence, recorded size, checksum (when present) and the native `REDIS`/`PGDMP` header before restore. Backup records stay in Lumic's private state and backup files live below `/var/backups/lumic`; this is a local reference implementation, not an off-node disaster-recovery claim.

## Application references

```bash
lumic managed-service attach primary-db my-app \
  --role database --database app_db --user app_user
```

Application metadata stores the service, role, database/user identity and secret reference, never the password. Unknown services, databases or users are rejected. Wiring those references into application environment files belongs to the later integration-intelligence mechanism.

Every mutation emits a managed-service event and audit record with actor, interface and correlation data. The same behavior is exposed through CLI, UI and policy-gated MCP tools. Ecosystem breadth beyond PostgreSQL and Redis is deliberately tracked as nightly work.
