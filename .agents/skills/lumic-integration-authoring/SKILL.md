---
name: lumic-integration-authoring
description: Add or change Lumic packages, runtime components, runtimes, managed services, service resources, verified artifacts, application processes or schedules, bindings, and application definitions. Use whenever implementing a new Lumic integration, provider, catalog entry, recipe, lifecycle, output, or cross-resource dependency, including CLI, UI, MCP, tests, and documentation for that integration.
---

# Lumic Integration Authoring

Extend Lumic through its existing resource contracts. Never create a parallel orchestration system, arbitrary shell execution, a giant provider `match`, or application-specific infrastructure hacks.

## Begin with the current contracts

Before editing code:

1. Read the repository `AGENTS.md`, `docs/ARCHITECTURE.md`, and `docs/DOCUMENTATION.md`.
2. Inspect the current definitions and traits instead of relying on this skill as an API snapshot. Prefer the codebase knowledge graph for code discovery.
3. Read the closest catalog definition under `crates/lumic-core/catalog/` and the corresponding implementation and tests.
4. For application composition, inspect the WordPress definition and implementation as the reference for typed resources, artifacts, bindings, rollback, idempotency, and ownership boundaries.
5. State which resource contracts the change extends before implementing it.

Useful starting points include:

- `crates/lumic-core/src/catalog.rs`, `resource.rs`, `binding.rs`, `pipeline.rs`, and `artifact.rs`
- `crates/lumic-core/src/application.rs` and `application_lifecycle.rs`
- `crates/lumic-platform/src/` for native apt, systemd, filesystem, artifact, runtime, and process primitives
- `crates/lumic-service/src/` for orchestration and driver registries
- `crates/lumic-core/catalog/applications/wordpress.toml` and the WordPress recipe/installer

Read [worked-examples.md](references/worked-examples.md) when implementing any of its matching integration types.

## Classify the integration first

Choose the smallest correct resource. Do not promote a package into a managed service merely because it runs software.

| Kind | Use it when | Typical identity and lifecycle |
|---|---|---|
| Package | A reviewed native package is the unit being installed or inspected | Distribution package name, installed version, apt state |
| Component | Software is attached to a runtime or service and is not independently operated | PHP extension, PostgreSQL extension; owned by its parent |
| Runtime | Applications execute through a versioned environment with typed capabilities | PHP or Node version, components, CLI/FPM outputs |
| Managed Service | An independently operated daemon needs lifecycle, config, health, logs, upgrades, and recovery | Redis, PostgreSQL, Meilisearch |
| Service Resource | A child object exists inside a service and depends on that service | Database, user, schema, virtual host; owned by its service instance |
| Artifact | Immutable downloaded bytes are an installation input | Versioned HTTPS URL plus digest, verified cache identity |
| Application Process | An application-owned long-running command needs supervision | Laravel Horizon worker |
| Application Schedule | An application-owned command needs backend-neutral recurring execution | Laravel scheduler mapped to a systemd timer |
| Binding | A consumer input depends on a producer output | App cache input bound to a Redis connection output |
| Application definition | Declarative desired composition describes an application and its capability requirements | WordPress, Laravel, Forgejo definition |

An endpoint is normally a typed provider output or a child resource published by a managed service. Do not make endpoint strings an untracked application setting when they participate in dependency or secret handling.

If one feature spans several kinds, model each separately. For example, an application may own a process and schedule, depend on a runtime, and consume a managed-service output through bindings.

## Choose catalog data or Rust behavior

A declarative TOML definition is sufficient only when all lifecycle behavior already exists behind a reviewed driver or generic resource contract. TOML should contain trusted metadata:

- stable IDs and definition/schema versions;
- configuration fields, defaults, constraints, sensitivity, and apply behavior;
- capabilities, typed outputs, and application requirements;
- supported platform/package/unit mappings;
- a driver ID that is already registered and compatible.

TOML is not a scripting language. Never put shell fragments, templates with executable logic, arbitrary commands, or recovery procedures in a catalog definition.

Add or extend a built-in Rust driver when the provider needs distinct behavior, such as:

- provider-specific configuration files or validation beyond the schema;
- a distinct health probe, endpoint discovery, logs, backup/restore, or upgrade strategy;
- service-resource commands or protocol behavior;
- artifact installation, user/directory/unit provisioning, or special recovery;
- application lifecycle behavior that generic composition cannot express safely.

Register the driver through the existing registry and make catalog loading fail if its driver ID is unknown. Keep provider behavior in the driver. Shared orchestration selects a trait implementation; it must not grow one branch per provider.

Use an application-specific Rust installer only for genuinely provider-specific application lifecycle work, as WordPress does for verified downloads, database setup, release activation, and rollback. Continue to express its resources, requirements, outputs, bindings, processes, and schedules through shared contracts.

## Design the resource contract

Define these before writing mutation code:

- Stable resource and definition IDs; version definitions when their public contract changes.
- Ownership: `Lumic` for objects created and governed by Lumic, `External` for adopted dependencies.
- Desired state and the evidence used to detect actual state and drift.
- Configuration schema, including defaults, allowed values/ranges, advanced fields, sensitivity, and whether a change requires reload, restart, or recreate.
- Capabilities and typed outputs, including which outputs are sensitive.
- Parent/child ownership for components and service resources.
- Binding roles and consumer inputs.
- Platform support and preconditions.
- Recovery behavior for every mutation boundary.

Configuration schemas are the public contract. The same schema must drive validation and presentation in CLI, UI, HTTP, and MCP. Put labels, descriptions, constraints, defaults, sensitivity, and apply behavior in the schema; do not recreate provider field lists in each adapter. Provider validation may add cross-field or host-aware checks, but it may not weaken schema validation.

## Implement the lifecycle pipeline

Every lifecycle must follow this shape:

```text
detect -> plan -> apply -> validate -> health -> commit state
                      \-> recovery on failure
```

### Detect

Inspect the actual host using platform adapters. Report versions, files, unit state, endpoints, and drift as evidence. Treat persisted state as intent/history, not proof that the host matches it. Detection is read-only.

### Plan

Resolve defaults and secret references, validate the desired configuration and policy, check ownership and reverse dependencies, calculate ordered typed actions, identify risk/preconditions, and classify recovery. Planning and dry-run must not mutate the host.

Prefer existing `PipelineAction` variants such as package, repository, artifact, directory, managed file, symlink, service, health, output, and state actions. If the domain lacks a primitive, add a narrow typed contract; do not fall back to arbitrary shell.

### Apply

Execute the reviewed plan with platform primitives and separated argument vectors. Use package-name validation, apt adapters, systemd adapters, atomic filesystem operations, artifact verification, locks, and typed `ProcessSpec` only where a native tool must be invoked. Never interpolate untrusted data into `sh -c`, `bash -c`, or a command string.

Apply must be idempotent and reconcilable:

- compare desired and actual state before changing anything;
- produce a no-op when already converged;
- use stable resource IDs and deterministic rendered content;
- hash or compare managed files;
- write atomically and preserve permissions;
- tolerate safe retries after interruption;
- persist state only after successful validation and health;
- emit before/after audit data and useful events.

### Validate

Validate the installed result, not merely command exit status. Re-detect package versions, file content/mode, unit enablement, artifact digest, resource existence, and output shape as appropriate.

### Health

Use the provider driver's typed health probe or protocol-aware check. A process being present is not sufficient when the service exposes a meaningful readiness check. Health failures prevent state commit and must include actionable evidence.

### Recovery

Classify steps as retryable, reversible, or manual. Snapshot or back up files before replacement, restore previous configuration or release pointers on failure, and restart/reload the previous healthy state when possible. Preserve data by default during service removal and explain manual recovery when automatic rollback is unsafe.

## Use outputs, bindings, ownership, and secrets together

Providers publish typed outputs such as runtime CLI paths, FPM sockets, connection endpoints, credentials, or database names. Applications declare capability requirements and consume those outputs through bindings.

- Publish outputs only after they have been detected or validated.
- Give outputs stable keys and capability types; mark credentials sensitive.
- Create a binding from `producer resource/output` to `consumer resource/input` rather than copying endpoint strings into app-specific orchestration.
- Resolve providers by capability, role, ownership, and explicit user choice—not by adding a provider name branch to application code.
- Validate the binding graph for missing outputs, duplicate inputs, and cycles.
- Record reverse dependencies. Refuse removal of a provider, child resource, runtime, process dependency, or binding target while consumers remain, unless an explicit safe detach plan is approved.

Secrets must use secret references such as `secret://...`; never store plaintext in resource attributes, catalog defaults, pipeline parameters, logs, audit events, generated plans, or ordinary outputs. Resolve secrets only at the narrow apply boundary, pass them through protected stdin or files when possible, redact diagnostics, and store generated secrets through the existing secret store.

## Provider-specific behavior belongs in drivers

A managed-service driver should own the provider-specific parts of:

- defaults and cross-field configuration validation;
- paths and deterministic configuration-file rendering;
- typed health probes and output discovery;
- log source metadata or journal unit selection;
- backup and restore plans when supported;
- service-resource operations;
- version discovery and upgrade compatibility/preconditions;
- post-upgrade validation and rollback/recovery guidance.

The application/service orchestration layer owns the generic ordering, locks, dry-run, policy, ownership checks, auditing, state persistence, and pipeline execution. Do not teach it that Redis, Meilisearch, Typesense, WordPress, or Laravel are special cases.

For upgrades, distinguish a native package upgrade from an artifact replacement or data-format migration. Pin the intended version, verify prerequisites, preserve previous config/artifact/release state, run the driver-specific compatibility checks, restart only when required, health-check, then commit. Never silently cross an incompatible major version.

## Expose one behavior through every adapter

CLI, UI, HTTP, and MCP are adapters over application/core orchestration. They may translate input and render status, plans, schema fields, and results, but must not install packages, render provider config, invent defaults, or perform provider-specific validation themselves.

When the integration changes public behavior:

- expose status, plan/dry-run, apply, health, logs, and removal consistently where relevant;
- render the shared configuration schema and sensitive-field metadata;
- update MCP schemas/descriptions so agents see capabilities, requirements, plans, and recovery implications;
- update the relevant public docs in `site/content/docs/` and internal architecture/specification docs in `docs/`;
- document actual implementation status, supported operating systems, permissions/policy, events, failure behavior, and recovery.

Do not add an adapter endpoint unless it delegates to shared orchestration.

## Test at the right boundaries

At minimum, add tests for:

- catalog parsing, schema versions, unknown drivers, configuration defaults, validation, redaction, and apply behavior;
- driver defaults, cross-field validation, deterministic config rendering, paths, health probe arguments, outputs, and upgrade/backup behavior;
- pipeline ordering, no-op plans, dry-run purity, failure classification, rollback, audit/state commit ordering, and retry/idempotency;
- ownership, child resources, binding cycles, reverse dependencies, secret-reference handling, and removal refusal;
- adapter serialization and delegation when CLI/UI/HTTP/MCP contracts change.

Add host-level integration coverage when behavior invokes apt, systemd, filesystem permissions, native services, artifacts, health endpoints, upgrades, or supported-OS assumptions. Run installation/reconciliation twice, introduce representative drift, verify recovery, and exercise removal boundaries. Add the OS to CI before claiming support.

Before finishing, run the repository-required checks from `AGENTS.md`:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

Run narrower tests during development, but do not substitute them for the required final checks.

## Completion checklist

- [ ] The integration uses the smallest correct resource kind and stable IDs.
- [ ] Catalog metadata is declarative; behavior lives behind an existing contract/driver.
- [ ] Detect, plan, apply, validate, health, and recovery are explicit and tested.
- [ ] Host changes use validated platform primitives and separated arguments—no arbitrary shell.
- [ ] Configuration schema drives validation and every interface; sensitive fields are marked.
- [ ] Outputs, capabilities, bindings, ownership, and reverse dependencies are recorded.
- [ ] Secrets remain references until the protected apply boundary and are redacted everywhere else.
- [ ] Reapplying converges to a no-op; drift and interruption can be reconciled safely.
- [ ] Health, logs, upgrades, backup/restore, and removal behavior are provider-appropriate.
- [ ] CLI, UI, HTTP, and MCP delegate to shared orchestration without duplicated business logic.
- [ ] Provider behavior is registered through traits/registries; no provider `match` or app infrastructure hack was added.
- [ ] Unit, lifecycle, binding, adapter, and host-level tests are present where applicable.
- [ ] Public and internal documentation matches the implemented behavior and OS support.
- [ ] Formatting, clippy, and workspace tests pass.

