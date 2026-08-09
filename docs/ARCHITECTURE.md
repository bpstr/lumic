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
Pure domain model: node facts, capabilities, operation requests/results, plans, policy decisions, events, audit records and abstractions. No transport/UI/Linux command syntax.

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

## Capability model

Operations should be typed:

```text
inspect host
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
