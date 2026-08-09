+++
title = "MCP"
description = "Structured infrastructure capabilities for Codex, Claude and other agent clients."
weight = 70
[extra]
kicker = "AGENTS"
status = "local policy-gated operations implemented"
+++

MCP is a first-class Lumic interface and the highest-leverage way to operate multiple nodes.

The target workflow is:

1. install Lumic independently on each VPS;
2. configure each Lumic MCP server in the coding agent;
3. let the agent inspect node status and capabilities;
4. describe the infrastructure outcome instead of manually debugging SSH sessions.

Example instruction:

> Create production on node-01 and staging on node-02. Inspect this repository's requirements, provision both environments, configure Git deployment and HTTPS, and verify health.

## Typed tools, not shell

MCP tools should look like:

```text
inspect_server
install_package
install_runtime
install_component
install_service
create_application
plan_deployment
deploy_application
rollback_deployment
search_logs
diagnose_server
```

Lumic must not expose unrestricted root shell execution as its normal MCP model.

## Policy

MCP access is capability-based. A production identity might be allowed to inspect, deploy and restart a service while being denied database deletion, firewall changes, OS upgrades or raw execution.

Mutating operations should carry actor/interface/correlation metadata into Lumic's audit trail.

## Current implementation

The MCP server uses the official Rust SDK over local stdio:

```bash
cargo run -p lumic-mcp
```

It publishes `lumic://server/status` and these shared-service tools:

```text
inspect_server                  diagnose_server
service_inspect                 service_apply
package_inspect                 package_install
application_list                application_inspect
application_plan_deployment     application_deployments
application_create              application_configure_process
application_set_repository      application_provision
application_set_health_check    application_deploy
application_rollback            application_enable_tls
managed_service_list            managed_service_detect
managed_service_inspect         managed_service_plan_install
managed_service_install         managed_service_configure
managed_service_apply           managed_service_declare_dependency
managed_service_database_create managed_service_user_create
managed_service_database_grant  managed_service_backup
managed_service_restore         application_attach_managed_service
recipe_catalog                  recipe_installations
recipe_plan                     recipe_install
recipe_update                   recipe_uninstall
host_operator_snapshot          host_search_logs
host_account_apply              host_permissions_apply
host_firewall_apply             host_process_signal
host_updates_apply              host_backup_schedule
host_remediate
events_list                     audit_list
```

There is no shell tool. Read operations work by default. Every apply tool requires both:

1. the MCP process was deliberately started with `LUMIC_MCP_ALLOW_MUTATIONS=1`;
2. the individual call contains `approved: true` after status/plan review.

For example:

```bash
LUMIC_MCP_ALLOW_MUTATIONS=1 LUMIC_MCP_ACTOR=codex cargo run -p lumic-mcp
```

Tool descriptions identify read-only versus mutating behavior. Apply operations use the same validated application, recipe, host-operator, apt, systemd, PostgreSQL/Redis, nginx, TLS, health and rollback services as the CLI and UI. Detection/list/inspect/plan/snapshot/log search are read-only. Recipe lifecycle, host account/permission/firewall/process/update/backup/remediation, service installation/lifecycle/configuration, database resources and application attachment require mutation policy plus `approved: true`. Actor/interface/correlation data is written to `audit.jsonl`; Git/database/recipe secret values are neither accepted by ordinary reads nor returned.

Remote HTTP transport, authentication, TLS, `lumic mcp setup`, fine-grained identity scopes, self-update and credential import tools are not implemented yet. Do not expose this stdio process remotely through an unauthenticated bridge. Import credentials locally with the CLI, then pass only their reference to MCP. Application process commands are accepted only as an executable/argument array; shell command strings are not a supported escape hatch.
