# Architecture

## Stable shape

Lumic is a modular Rust workspace. The initial crates intentionally establish dependency boundaries before broad features arrive.

```text
CLI ─┐
UI  ─┼─> application services -> lumic-core <- adapters
API ─┤                              ↑
MCP ─┘                        lumic-platform
```

### lumic-core
Pure domain model: node facts, capabilities, operation requests/results, suggestions, plans, policy decisions, events, audit records and abstractions. No transport/UI/Linux command syntax.

### lumic-platform
Linux host detection and adapters for package manager, service manager, process/filesystem/network/firewall mechanisms. Debian and Ubuntu first. Adapters execute native tools through argument-safe process APIs.

The host-status service reads through a testable `HostDataSource`. The system adapter reads `/etc/os-release`, procfs, the process-visible CPU count, and root filesystem capacity; diagnosis adds load, uptime, process and failed-systemd-unit evidence. CLI and MCP call these same services. The internal async process runner accepts an executable and an argument vector plus narrowly scoped environment, enforces a timeout, bounds both output streams, and records exit code/signal/truncation metadata; it is not itself a public capability.

Epic A adds small concrete adapters rather than a plugin framework: apt package/runtime catalogs, systemd lifecycle, atomic recoverable files, nginx/TLS, application processes, and checksum-verified self-update. `ApplicationService` composes these adapters into persistent application provisioning and deployment. The runtime-neutral deployment state machine owns locks, immutable Git checkout and provenance, explicit argv-only pre/build/migrate/post phases, activation, health, cancellation, retry and logs. The process and nginx adapters implement Node's release-scoped blue/green start, upstream handoff and drain.

`lumic.yaml` is the versioned repository-to-server contract. `lumic-core` owns its strict transport-neutral schema, validation, and resolution into existing runtime, deployment, health, process, schedule, and managed-service requirement types. `lumic-platform` performs bounded non-symlink reads from the repository root and keeps inspection, planning, and approved application separate. During deployment, the manifest from the exact checked-out commit is authoritative for source/public paths and deployment phases; unresolved service requirements block the release. The contract cannot express shell fragments, secrets, package installation, or Linux-specific implementation details.

The service/resource framework is the additive replacement for the original two-provider model. `lumic-core` owns strongly validated, versioned, built-in TOML definitions; structured configuration schemas; stable string resource IDs; ownership/drift state; typed outputs and bindings; provider-neutral certificate lifecycle contracts; and journalable pipelines. Catalog data is trusted metadata only: it cannot contain shell snippets. Provider behavior is selected by compile-time Rust contracts and registries.

Framework state uses an explicit schema version and mode-0600 atomic writes. The first load can migrate the legacy `managed-services.json` representation to `resources.json`; Lumic copies the exact legacy bytes to `managed-services.v1.json` before committing the new state and leaves the source untouched. Bindings enforce referential integrity, reject dependency cycles, and prevent removal while consumers remain. Mutating pipelines use cross-process advisory locks per resource plus separate global package/repository and nginx locks. The web foundation persists nginx as `nginx.main`, sites as owned service resources, and selected PHP runtimes as `php.<version>` resources whose typed FPM output is explicitly bound to each PHP web host.

Reusable application resources keep portable intent in `lumic-core` and Linux translation in `lumic-platform`. Artifact definitions are immutable version/URL/SHA-256 contracts; the platform artifact manager locks, downloads privately, verifies and atomically caches them before any consumer can use them. Application schedules express calendar or interval timing, missed-run policy and jitter without naming systemd directives. The Linux adapter renders those definitions as systemd services and timers. Native package requirements carry a reason and gain an explicit trust source only after package-policy review; application lifecycle plans retain that review result.

Certificates are independent resources bound explicitly to owned nginx web hosts. The built-in Certbot/Let's Encrypt provider implements read-only planning, native preflight, `certonly` issuance, inspection, named renewal, and detach/delete. Certbot does not retain ownership of Lumic's nginx files: a separate nginx consumer adapter writes the inspected certificate paths atomically, validates with `nginx -t`, reloads, and restores the previous HTTP configuration on failure. Certificate and nginx locks serialize reconciliation; state is committed only after native activation succeeds.

Fifteen providers remain on the compatible `ManagedServiceManager` command surface: MySQL, PostgreSQL, Redis, Typesense, Meilisearch, Valkey, RabbitMQ, MinIO, OpenSearch, Memcached, MongoDB, ClickHouse, Prometheus, Grafana, and Loki. The built-in driver registry owns their platform mapping, configuration validation/rendering, path metadata, health probes, backup/restore plans where supported, and child-resource behavior. The manager executes typed plans without selecting provider behavior itself. `resources.json` is authoritative for these commands; `managed-services.json` is read only by the one-time backup-first migration and is never a write target. Compatibility updates replace only records owned by registered drivers, preserving framework-native services, resources, bindings, and pipeline journals. Relational database and credential child resources publish separate outputs; application roles bind to those outputs only after the user grant exists. Search services publish an HTTP endpoint and a sensitive credential reference directly from the managed-service resource, and reverse bindings protect them from removal. Secret values continue to live in the private secret store and native configuration receives only the value required to start the provider. The ten newer native drivers intentionally reject backup/restore and child-resource operations until provider-specific recovery contracts are implemented.

### lumic-daemon
Long-running node process. Owns lifecycle wiring, state, event dispatch, scheduled host observation and interface servers. The installed daemon currently serves the loopback-only operator UI and graceful shutdown. Business behavior remains in reusable services rather than handlers.

### lumic-cli
Human interface. Commands translate arguments to the same capability/application operations exposed elsewhere. `--json` should become available for automation as commands mature.

### lumic-mcp
Agent adapter. MCP exposes resources and typed tools rather than generic shell. Tool descriptions should contain preconditions, risk and output schemas useful for coding agents.

The MCP adapter uses the official Rust `rmcp` SDK. `lumic mcp serve` owns stdio transport; `lumicd` can opt into bearer-authenticated Streamable HTTP on a loopback-only listener. Both instantiate the same `LumicMcpServer` tool/resource adapter. Read tools are available by default. Apply tools require process-level `LUMIC_MCP_ALLOW_MUTATIONS=1`, a matching `LUMIC_MCP_SCOPES` grant and per-call `approved=true`. Public HTTP must terminate TLS in a reverse proxy; OAuth and per-identity grants are not yet implemented.

Epic E adds one `OperationsService` rather than a telemetry framework. It periodically folds live host/process/application/service observations, selected journald kernel evidence, existing durable events and typed provider signals into an append-only correlated timeline. The same service owns private webhook/rule/subscription state, a bounded delivery queue and the single reference automation (`service.failed -> typed systemd restart -> active-state verification`). CLI, daemon and MCP are adapters over it. Generic webhook HMAC delivery is the notification substrate; destination-specific adapters remain catalog breadth. Runtime state updates preserve the last configuration snapshot so rollback stays meaningful.

Epic F adds one `ApplicationIntelligence` orchestration boundary, not a framework plugin system or an embedded autonomous agent. Core types describe fingerprints, evidence, configuration previews, dependency nodes/edges, integration definitions/plans/results, incident context and typed advisory remediations. The platform implementation reads bounded deployed-source files, composes `ApplicationService`, `ManagedServiceManager`, systemd and `OperationsService`, and owns integrity-checked dotenv snapshots. The compiled `laravel-redis@1` definition is the only reference. Optional analysis reuses a configured signed webhook destination, sends a bounded redacted evidence package and rejects unknown remediation fields, invalid evidence citations and non-typed actions.

### lumic-ui / API
The initial Axum UI is a server-rendered adapter over `ApplicationService`, `ManagedServiceManager`, `ResourceFramework`, `SoftwareManager`, `RecipeManager`, `HostOperator`, host inspection and event storage. It owns authentication/session/CSRF and HTML presentation, but no privileged host implementation. `ResourceFramework` is the shared CLI/UI/MCP read and binding boundary for trusted catalog definitions, redacted resource inspection, explicit bindings, and durable pipeline journals; binding mutations take the framework lock and validate the complete graph before persistence. `SoftwareManager` separately composes the apt adapter with the fixed installer catalog. System installers use the typed apt path; NVM uses a distinct per-user, pinned Git path because it is loaded by a user's shell. The UI binds only to loopback, uses in-memory sessions and exposes confirmed catalog install/restart/deploy/rollback/security-update actions. A general HTTP API and remote-auth model remain future interfaces.

## No companion client or skills layer

Lumic deliberately does **not** have a separate remote client binary and does **not** require a Lumic-specific AI skills package.

A Lumic node exposes its own operational surface through CLI, UI, HTTP/API where needed, and MCP. Remote coding agents connect directly to the node MCP endpoint. Do not create `lumic-client`, SDK-like command wrappers, generated remote CLIs, or a separate skills repository unless a concrete future requirement cannot be solved through the node interfaces.

Operational knowledge should stay close to the product through:

- MCP tool/resource schemas and descriptions;
- public Lumic documentation;
- runtime/service/application catalogs and recipes;
- host and application inspection;
- structured suggestion results.

This avoids duplicated knowledge, version skew, extra installation steps, and a second release lifecycle.

## Core interaction model

For humans and agents, Lumic should converge on four conceptual stages:

```text
STATUS
What exists now?

SUGGEST
What would make sense for this project/host?

PLAN
What exactly would change?

APPLY
Perform the approved change.
```

These are different responsibilities and must remain separate.

- **Status** reports evidence from the actual host and managed resources.
- **Suggest** is read-only reasoning support. It detects likely project requirements and returns recommendations with evidence. It never mutates the host.
- **Plan** resolves a concrete desired change against current state, policy, risks and preconditions.
- **Apply** performs the validated plan and records the result.

Suggestions inform; plans execute.

## Suggestion model

`suggest` exists so an LLM can quickly understand the likely infrastructure requirements of a known stack or inspected repository without Lumic becoming the LLM itself.

Representative human interface:

```text
lumic suggest laravel
lumic suggest nextjs
lumic suggest --path /srv/app
```

Representative MCP capability:

```text
suggest_application_setup
```

Possible inputs include an explicit stack/framework, repository/application path, or target role. Repository-aware suggestion should inspect relevant manifests and framework signals such as `composer.json`, `package.json`, `Cargo.toml`, `pyproject.toml`, lock files, environment examples, migration/config files, worker/scheduler hints and runtime version files.

Results are structured and evidence-backed. Example fields:

```text
detected framework/runtime/package manager
required runtime/components
recommended services
web/runtime process model
workers/scheduler
persistent paths
deployment strategy
source evidence
```

Suggestion should identify requirements and recommendations, not make hidden sizing or mutation decisions. The agent can combine suggestion output with live server status to decide the final topology and configuration.

## Capability model

Operations should be typed:

```text
inspect host
suggest application setup
search/install/remove package
inspect/start/stop/restart service
install runtime/component
create/configure application
plan/deploy/rollback application
search logs
diagnose server
```

Each mutation can carry metadata: actor, permissions, dry-run, correlation ID, source interface and approval context.

## Native command gateway

There may be a low-level process executor, but it is internal infrastructure. Higher layers call adapters with validated arguments. Package whitelisting should validate identifiers and trusted repositories before calling apt. Never implement normal capabilities by accepting arbitrary shell strings.

## Policy

Policy is capability-based. Example scopes:

```text
server.read
package.read
package.install.allowed
service.restart
application.deploy
database.backup
system.exec   # disabled by default
```

High-risk capabilities can require explicit approval. Audit all mutations and security-relevant reads.

## State

Prefer authoritative live host inspection plus a small Lumic state store for managed resources, desired configuration, history, audits, events and secrets references. Do not create a shadow copy of Linux that can silently drift. Reconciliation should detect differences between desired/managed state and actual host state.

## Plans and idempotency

Provisioning operations should converge toward desired state. A plan describes current state, intended state, operations, risks, validation and rollback availability. Re-running a completed desired-state operation should normally be safe.

## Extensibility contracts

- Package: identifier mapping, trust source, detect/version/capability metadata.
- Component: install/configure/attach/detach/validate.
- Managed service: detect/install/configure/start/stop/reload/health/logs/upgrade/backup hooks/events.
- Runtime: detect/install/build/start/stop/reload/health/deployment activation.
- Recipe: declarative composition + setup contract.
- Role: declarative node composition/topology intent.

Prefer data-driven catalogs where behavior is native/simple. Use Rust adapters only when logic actually requires it.

## Zero-downtime deployment

Deployment domain is independent of GitHub/GitLab and runtime. Stages: source resolution -> release preparation -> dependencies/build -> shared links -> pre-activation validation -> runtime-specific activation -> health -> post-activation -> retention. Activation strategy is supplied by runtime/application adapter.

The file-based strategy serializes each application deployment, creates immutable releases, resolves the checked-out revision's `lumic.yaml`, runs migrations before activation, and atomically replaces `current`. HTTP health or post-deploy failure restores the prior target before the failed release is removed. For Node, a release-scoped systemd unit must pass direct readiness before nginx is atomically moved to its inactive port; the old unit drains only after the public health gate. nginx configuration uses the same safety principle: sibling atomic write, backup, native validation, reload, and restoration on validation/reload failure. Cancellation is cooperative at phase boundaries, retry/redeploy pins the recorded commit, and persistent logs use monotonic cursors. Deployment plans are read-only objects; deployment apply remains a distinct auditable operation.

Application environment secrets are references owned by an application environment, not a general-purpose vault API. Values use authenticated encryption under a node-local private key. They are resolved only at the deployment/process boundary into a root-readable volatile environment file, while application status, diffs, audit records, UI and ordinary MCP inspection retain keys and masked references only. Set, random rotate and delete are separate auditable mutations; deleting an application-owned value removes both the current encrypted envelope and its rotation backup.

## Multi-node

Each node remains autonomous. `InfrastructureService` composes private state, the secret store, Ed25519 identity/trust, native Git, portable application configuration and the existing application deployment service. CLI and MCP are adapters over it; the UI and authenticated HTTP endpoint expose its read model.

Cross-node orchestration is performed by an external agent connected to multiple MCP servers. The coordinator creates a durable member/failure-boundary record, asks each target to execute a short-lived signed allowlisted request, and records the node-local deployment/health result. There is no node-to-node control loop, consensus system or generic remote executor. A future Lumic Hub may coordinate fleets but cannot become mandatory for single-node correctness.

## Attention summary

`lumic-core::attention` defines the transport-independent summary, evidence items and deterministic personality renderer. `lumic-platform::AttentionService` gathers live host diagnostics and folds existing application, managed-service, certificate, backup and event stores into that model. Certificate-expiry and latest-backup-age policies raise shared warning/critical evidence, and the UI operations overview renders the same verdict alongside applications, services, deployments, resource pressure, failed services, security updates and recent incidents. CLI, UI and MCP are thin adapters over the service; none derives its own health verdict or personality copy.

This separation is intentional: evidence and severity are computed once, then presentation is applied without being allowed to remove or change any operational field. Adding an attention source should extend factual collection in the platform service (or a proven provider boundary), not create interface-specific heuristics.
