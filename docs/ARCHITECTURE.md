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

### lumic-daemon
Long-running node process. Owns lifecycle wiring, state, event dispatch, scheduled host observation and interface servers. Business behavior must remain in reusable services rather than handlers.

### lumic-cli
Human interface. Commands translate arguments to the same capability/application operations exposed elsewhere. `--json` should become available for automation as commands mature.

### lumic-mcp
Agent adapter. MCP exposes resources and typed tools rather than generic shell. Tool descriptions should contain preconditions, risk and output schemas useful for coding agents.

### UI/API
Future crates/adapters. The UI is Rust-based and calls the same services. Do not place privileged host logic in browser-facing code.

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

## Multi-node

Each node remains autonomous. Cross-node orchestration may initially be performed by an external agent connected to multiple MCP servers. A future Lumic Hub may coordinate fleets but cannot become mandatory for single-node correctness.
