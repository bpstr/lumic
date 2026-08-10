+++
title = "MCP"
description = "Structured infrastructure capabilities for Codex, Claude and other agent clients."
weight = 70
[extra]
kicker = "AGENTS"
status = "stdio and authenticated Streamable HTTP implemented"
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

The installed `lumic` binary contains the MCP adapter. Serve it over stdio:

```bash
lumic mcp serve
```

It publishes `lumic://server/status`, `lumic://server/attention`, and these shared-service tools:

```text
inspect_server                  diagnose_server
server_attention
service_inspect                 service_apply
package_inspect                 package_install
resource_catalog               resource_schema
resource_plan                  resource_apply
resource_inspect               resource_bindings
resource_binding_apply         resource_binding_remove
resource_operations            resource_operation_inspect
software_catalog                software_status
software_plan_setup             software_setup
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
managed_service_backup_verify
managed_service_restore         application_attach_managed_service
recipe_catalog                  recipe_installations
recipe_plan                     recipe_install
recipe_update                   recipe_uninstall
host_operator_snapshot          host_search_logs
host_account_apply              host_permissions_apply
host_firewall_apply             host_process_signal
host_updates_apply              host_backup_schedule
host_remediate
infrastructure_status           node_initialize
node_enrollment                 node_register
node_revoke                     node_health
git_repository_host             git_mirror_sync
git_push_deploy_configure       environment_secret_generate
application_environment_reference_set
environment_export              environment_import
environment_diff                resource_endpoint_register
node_membership_configure       coordinated_deployment_begin
coordinated_deployment_report   remote_operation_sign
remote_operation_apply
operations_timeline             operations_incident
operations_provider_signal      operations_webhook_plan
operations_webhook_apply        operations_subscription_apply
operations_rule_plan            operations_rule_apply
operations_run_once             operations_observe
operations_deliveries
operations_configuration_rollback
application_fingerprint         application_configuration_inspect
application_dependency_graph   application_integration_catalog
application_integration_plan   application_integration_apply
application_configuration_rollback
incident_context               incident_analyze
events_list                     audit_list
```

The `resource_*` tools are the generic framework surface. Catalog and schema calls return the
same trusted definitions used by CLI and UI. `resource_plan` is read-only;
`resource_apply` performs catalog installs or typed lifecycle actions. Inspection redacts sensitive
values. Binding mutations validate both endpoints and producer outputs, reject duplicate inputs and
cycles, and take the shared resource lock. Operation queries return durable pipeline and step
journals so an agent can monitor progress, failures, and recovery messages without scraping logs.

The `managed_service_*` tools remain compatibility aliases for provider-specific database, backup,
and dependency operations. New generic orchestration should begin with `resource_catalog`.

`application_provision` requires `runtime_version` for PHP (`8.1`, `8.2`, `8.3`, or `8.4`) and accepts allowlisted extension `components`. The selected packages must exist in the node's configured apt repositories. Its result identifies the persisted runtime resource and exact FPM output; nginx web-host state is committed only after native validation and activation succeed.

The software tools include `nodejs` as a system package installer and `nvm` as
a per-user installer. Pass an existing Linux account in `user` when inspecting,
planning, or setting up NVM. Mutating setup still requires `approved=true`.

There is no shell tool. Read operations work by default. Every apply tool requires all three:

1. the MCP process was deliberately started with `LUMIC_MCP_ALLOW_MUTATIONS=1`;
2. `LUMIC_MCP_SCOPES` grants the required scope;
3. the individual call contains `approved: true` after status/plan review.

For example:

```bash
LUMIC_MCP_ALLOW_MUTATIONS=1 \
LUMIC_MCP_SCOPES=mutations,operations.signal,operations.configure,operations.automate,operations.run,application.integrate,incident.analyze \
LUMIC_MCP_ACTOR=codex lumic mcp serve
```

Register a local stdio server in Codex:

```bash
codex mcp add lumic -- /usr/local/bin/lumic mcp serve
codex mcp list
```

For a remote node, prefer a dedicated SSH key whose server-side `authorized_keys` entry is forced to `/usr/local/bin/lumic mcp serve`. The installer can create that restricted entry when `LUMIC_MCP_AUTHORIZED_KEY` contains the dedicated public key. Then register `ssh -T -o BatchMode=yes root@server`; the forced command prevents that key from opening a general shell. A normal unrestricted root key does not provide the same boundary.

## Streamable HTTP

`lumicd` can additionally expose the same tools at `/mcp` using MCP Streamable HTTP. The listener is disabled unless `LUMIC_MCP_HTTP_BIND` is configured, requires a separate bearer token, and refuses non-loopback binds. Create the token once:

```bash
sudo lumic mcp token rotate
sudo systemctl edit lumicd
```

Add the listener in the systemd override:

```ini
[Service]
Environment=LUMIC_MCP_HTTP_BIND=127.0.0.1:10801
```

After restarting `lumicd`, either use a local tunnel or place an HTTPS reverse proxy in front of `127.0.0.1:10801`. For a tunnel:

```bash
ssh -N -L 10801:127.0.0.1:10801 root@server
export LUMIC_MCP_TOKEN='the-token-shown-once'
codex mcp add lumic-http --url http://127.0.0.1:10801/mcp \
  --bearer-token-env-var LUMIC_MCP_TOKEN
```

Do not bind the daemon listener publicly or send the bearer token over plaintext HTTP. A public URL must terminate TLS before forwarding to the loopback listener. The current token authenticates one node-level MCP policy; OAuth and per-identity grants remain future work.

Tool descriptions identify read-only versus mutating behavior. Existing mutation tools use the `mutations` compatibility scope. Epic E operations are separated into `operations.signal`, `operations.configure`, `operations.automate` and `operations.run`; Epic F integration apply/rollback uses `application.integrate`, and external incident disclosure uses `incident.analyze`. `operations.*` grants that family and `*` grants all scopes. Apply operations use the same validated application, recipe, host-operator, apt, systemd, MySQL/PostgreSQL/Redis, nginx, TLS, health and rollback services as the CLI and UI. Fingerprints, key-only configuration inspection, dependency graphs, integration plans, incident context, timeline, delivery history and backup verification are read-only. Analysis is advisory and its proposed remediations must be executed separately through ordinary typed tools. Actor/interface/correlation data is written to `audit.jsonl`; Git/database/recipe/notification/dotenv secret values are neither accepted by ordinary reads nor returned.

Node enrollment, trust and signed deploy/rollback envelopes are implemented independently of transport. An agent connected to two MCP endpoints can carry an envelope from `remote_operation_sign` to `remote_operation_apply`; expiry, target, trust, signature and replay protection are revalidated by the receiving node. Process scopes constrain the node-level server policy, not separate authenticated client identities. Import credentials locally with the CLI, then pass only their reference to MCP. Application process commands are accepted only as an executable/argument array; shell command strings are not a supported escape hatch.
