# Service and Application Resource Framework Progress

This document tracks implementation of the service, resource, binding, and lifecycle pipeline redesign. It is the durable checklist for the feature bundle; update it whenever scope is completed, deferred, or materially redesigned.

## Status legend

- `[ ]` Not started
- `[~]` In progress
- `[x]` Completed and verified
- `[!]` Blocked or deliberately deferred; explain why in the decision log

## Baseline

- [x] Read the implementation brief and repository agent instructions.
- [x] Audit current architecture, documentation, tests, and the historical `v1` behavior.
- [x] Record compatibility constraints and map existing safety primitives to the new framework.
- [x] Establish a clean test baseline without overwriting unrelated worktree changes.

## Phase 1 — Domain foundation

- [x] Add stable catalog definition and instance identifiers.
- [x] Add validated service/runtime/application definitions and built-in TOML loading.
- [x] Add reusable configuration schemas, defaults, validation, and apply behavior.
- [x] Add service instances, ownership, management state, desired state, and outputs.
- [x] Add resources, endpoints, bindings, reverse-dependency checks, and cycle rejection.
- [x] Add typed pipelines, execution steps, recovery classifications, and journals.
- [x] Add cross-process resource/package/repository/nginx locks.
- [x] Add explicit persisted-state versioning and safe legacy migration.
- [x] Add focused unit tests and internal/public architecture documentation.

## Phase 2 — Existing providers

- [x] Introduce the driver registry without a closed provider-kind dispatch path.
- [x] Migrate Redis behavior and tests to a Redis driver.
- [x] Migrate PostgreSQL behavior and tests to a PostgreSQL driver.
- [x] Preserve compatible CLI/UI/MCP behavior during migration.
- [x] Remove obsolete provider branching and stale state paths.

## Phase 3 — Web and PHP foundation

- [x] Implement nginx as an independently managed service.
- [x] Implement owned web-host resources, validation, rollback, and reload.
- [x] Implement explicit, versioned PHP runtimes and extension components.
- [x] Publish PHP-FPM outputs and bind web hosts to a selected runtime.
- [x] Remove implicit nginx installation and arbitrary FPM socket discovery.

## Phase 4 — MySQL

- [x] Implement MySQL service lifecycle and health.
- [x] Implement database, user, grant, and credential resources.
- [x] Store secret references rather than plaintext credentials.
- [x] Support multiple application-owned databases and bindings.

## Phase 5 — Certificates

- [x] Add certificate resources and provider contracts.
- [x] Implement Certbot/Let's Encrypt planning, preflight, issue, inspect, renew, and detach.
- [x] Implement nginx certificate attachment and rollback.
- [x] Add a deterministic fake provider for CI.

## Phase 6 — Generic PHP applications

- [x] Refactor application orchestration onto resources, bindings, and pipelines.
- [x] Support domains, roots, runtimes, components, databases, packages, TLS, processes, schedules, and health.
- [x] Produce explicit install, reconcile, update, and removal plans.

## Phase 7 — WordPress proof

- [x] Add a validated built-in WordPress definition.
- [x] Add pinned, checksum-verified WordPress and WP-CLI artifacts.
- [x] Implement the complete idempotent WordPress lifecycle pipeline.
- [x] Persist ownership, resources, bindings, secrets, and operation progress.
- [x] Add failure/recovery and safe-removal tests.
- [x] Add and pass the Ubuntu 24.04 golden WordPress CI workflow.

## Phase 8 — Search services

- [x] Implement Typesense as a managed service with verified installation and secrets.
- [x] Implement Meilisearch as a managed service with verified installation and secrets.
- [x] Publish reusable search endpoints and protect bound services from removal.

## Phase 9 — Reusable application resources

- [x] Implement the verified artifact manager.
- [x] Implement pinned Gitea and Gogs managed-service installers over the shared repository root.
- [x] Implement systemd-backed application processes.
- [x] Implement application schedules with a backend-neutral domain model.
- [x] Integrate reviewed package requirements and policy-derived trust.

## Phase 10 — CLI, UI, and MCP completion

- [x] Expose generic catalog-driven service operations through the CLI.
- [x] Expose catalog, schema, plan/apply, inspection, binding, and operation tools through MCP.
- [x] Build catalog-driven service lists/forms/details in the existing UI shell.
- [x] Build application details and pipeline progress/failure views.
- [x] Remove dead tools and obsolete documentation.

## Final verification

- [x] `cargo fmt --check`
- [x] `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`
- [x] `cargo test --locked --workspace --all-features`
- [x] `cargo build --locked --workspace --release`
- [ ] Existing smoke/installer coverage remains passing.
- [ ] WordPress E2E passes twice without duplicating resources.
- [ ] All architecture acceptance statements in the implementation brief are satisfied.

## Decision log

### 2026-08-10 — Tracker created

Implementation follows the brief's phased order. The worktree already contains unrelated in-progress changes, so framework changes must be additive and must not discard or rewrite those edits without first reconciling their intent.

### 2026-08-10 — Foundation and migration boundary

The schema-v2 store is additive while PostgreSQL and Redis still use the compatibility manager. Legacy migration is backup-first and does not delete `managed-services.json`; the compatibility path remains authoritative until Phase 2 is complete. Existing atomic file writes, secret references, argument-vector process execution, apt allowlists, systemd operations, audit records, and events remain the required lower-level safety primitives.

Provider configuration validation, paths, and health probes now resolve through the trusted driver registry. Configuration rendering, backups/restores, and PostgreSQL child-resource operations still contain legacy dispatch and therefore keep Phase 2 in progress.

### 2026-08-10 — Existing-provider migration complete

PostgreSQL and Redis platform mapping, configuration validation/rendering, health probes, backup/restore planning, and child-resource commands now resolve through the built-in Rust driver registry. The compatibility manager executes typed driver plans and no longer selects operational behavior with provider-kind branches. Existing CLI, UI, and MCP command and response shapes remain unchanged; the compatibility enum is retained only at those adapter and state-conversion boundaries until the generic interfaces in Phase 10 replace it.

`resources.json` is now the authoritative store for compatibility operations as well as framework-native resources. Lumic reads `managed-services.json` only for the one-time backup-first migration, preserves its exact bytes as `managed-services.v1.json`, and does not write new state to the legacy path. Compatibility saves replace only PostgreSQL/Redis-owned records and preserve bindings, pipelines, and resources belonging to other drivers.

### 2026-08-10 — Web and PHP foundation complete

nginx is installed and persisted as the independently owned `nginx.main` catalog service rather than as a static/PHP/Node runtime package side effect. A successfully validated and activated site becomes an owned `nginx.web-host.<application>` service resource with explicit nginx-to-host and host-to-application bindings. Configuration, symlink, validation, service activation, and framework-state failures restore the prior file/link and reload the known-good configuration where applicable.

PHP provisioning now requires an allowlisted version (`8.1`, `8.2`, `8.3`, or `8.4`) and installs version-qualified FPM, CLI, and extension packages. Availability still follows the configured Debian/Ubuntu apt repositories. The persisted `php.<version>` runtime publishes typed `fpm` and `cli` outputs; PHP web hosts bind to that exact runtime and render its deterministic `/run/php/php<version>-fpm.sock` output. Runtime provisioning no longer installs nginx and nginx no longer scans `/run/php` for an arbitrary socket.

### 2026-08-10 — MySQL phase complete

MySQL is now a built-in trusted driver on the compatible managed-service surface. It installs the catalog-selected native package, writes a loopback-only owned configuration fragment, controls `mysql.service`, validates health with the local socket, and implements native SQL backup/restore. Database, user, and grant operations use validated identifiers and direct process execution. Generated passwords are written only to the private secret store, passed to the native client over stdin, and published to the resource graph only as sensitive `secret://` references.

Database and credential child resources publish separate typed outputs. Application attachment requires that the selected user has an explicit grant, then records independent role-scoped database and credential bindings. Replacing one role does not disturb other roles, so one application can own multiple isolated MySQL databases and users. Unit coverage verifies driver command construction, secret-reference outputs, and multiple bindings; the existing Ubuntu 24.04 managed-service gate now exercises live MySQL lifecycle, two databases/users/grants, backup verification, and application attachment alongside PostgreSQL and Redis.

### 2026-08-10 — Certificate phase complete

Certificates are validated `certificate.<application>` resources with provider-neutral request, plan, preflight, inspection, and lifecycle contracts. The built-in Certbot/Let's Encrypt provider uses direct argument-vector execution for preflight, `certonly` issuance, inspection, named renewal, and deletion. Plans contain domains and non-secret steps but omit the contact email. Provider selection is explicit and restricted to the trusted built-in provider.

Certificate issuance no longer gives Certbot ownership of Lumic's nginx configuration. Lumic attaches the inspected live certificate paths to the owned web-host configuration, runs `nginx -t`, reloads nginx, and persists an explicit certificate-to-web-host binding only after native activation succeeds. Failed validation, reload, or state persistence restores the known-good HTTP configuration and cleans up a newly issued certificate where safe. Detach restores that saved configuration before deleting the named Certbot certificate. Certificate and nginx locks serialize cross-process reconciliation.

A deterministic fake provider and fake consumer adapter exercise issue, renewal, detach, resource persistence, and binding removal in ordinary CI without network, DNS, root, Certbot, or nginx dependencies. Phase completion passes the focused certificate tests plus workspace formatting, clippy with warnings denied, and the full workspace test suite. The final-verification checklist remains an end-of-program acceptance gate because later phases are still open.

### 2026-08-10 — Generic PHP application phase complete

Generic PHP intent is now one validated desired-state contract covering the managed application identity and root, primary domain and optional `www` alias, explicit PHP version and components, trusted native packages, repository, role-scoped database references, TLS intent, workers, schedules, and HTTP health. Validation rejects unmanaged roots, unsupported PHP versions/components, policy-denied packages, duplicate database roles/processes, plaintext secret references, unsafe commands/schedules, and malformed health checks before a plan reaches an apply boundary.

Every install, reconcile, update, and removal request produces both a human-readable Lumic plan and a validated typed pipeline. Update alone includes release deployment; reconcile is idempotent desired-state repair. Removal reverses owned certificate, process/schedule, database-binding, web-host, and root relationships without uninstalling shared packages, runtimes, databases, or managed services. Plans contain secret references where required but never secret values or certificate contact email.

Application creation now persists the application as a first-class resource. Configured workers and schedules are owned process/schedule resources that publish their systemd units and bind explicitly to the application. The application apply boundary takes the application resource lock and persists a running pipeline journal in authoritative schema-v2 state before native work proceeds. Existing nginx, versioned PHP/component, database attachment, package-policy, certificate, systemd process, release, and health operations remain the native executors composed by this contract; catalog-driven CLI, UI, and MCP entry points remain Phase 10 rather than creating a second orchestration path.

### 2026-08-10 — WordPress proof complete

The reviewed `wordpress@1.0.0` recipe composes PHP 8.3 with the `curl`, `mbstring`, `mysql`, `xml`, and `zip` components, an isolated MySQL database/user/grant pair, an owned nginx web host, generated administrator credentials, optional TLS, and a local login health check. Its required site title, administrator user, and administrator email inputs are validated before apply.

WordPress 6.8.2 and WP-CLI 2.12.0 use immutable HTTPS release URLs and pinned SHA-256 digests. Downloads enter an atomic private artifact cache only after streaming checksum verification. WP-CLI receives database and administrator passwords over stdin; persisted application and recipe state contains private-store references rather than plaintext values.

Recipe state records owned resources, binding identifiers, resolved artifact versions, and a durable step journal. Deployment extracts into a staging release, activates `current` atomically, restores the prior release if WordPress configuration or installation fails, and leaves failed progress available for an explicit retry.

Uninstall removes only the recipe-owned nginx configuration, application tree through the recoverable trash path, generated secrets, application/web-host resources, and their bindings. It retains the verified artifact cache, native packages, PHP runtime, MySQL service, database, user, and grant. Unit coverage exercises checksum rejection, activation rollback, and safe removal; the Ubuntu 24.04 golden workflow exercises live install, HTTP/WP-CLI health, a second convergent install with no duplicate resources, and removal boundaries.

### 2026-08-10 — Search services complete

Typesense and Meilisearch are now trusted built-in drivers on the existing managed-service surface. Installation remains constrained to the allowlisted native package and requires an apt candidate from a source the operator has already configured; Lumic verifies the installed package before writing configuration. Typesense uses its packaged systemd service and a private owned configuration file. Meilisearch uses a private environment file plus a hardened Lumic-owned systemd unit, with daemon reload and configuration rollback included in the mutation boundary.

Each service generates a 256-bit credential in Lumic's mode-0600 secret store. Persisted service and application state contains only secret references. Both providers bind to loopback, publish a reusable `http` output and a sensitive provider-specific credential output, and validate their unauthenticated `/health` endpoint without exposing credentials in process arguments. Application attachment records role-scoped endpoint and credential bindings; reverse-dependency validation rejects removal while either output is consumed. Search database primitives and backup/restore remain explicitly unsupported rather than emulating provider behavior through shell hooks.

### 2026-08-10 — Native service catalog expanded

Valkey, RabbitMQ, MinIO, OpenSearch, Memcached, MongoDB, ClickHouse, Prometheus, Grafana, and Loki now have reviewed built-in catalog definitions, allowlisted packages, registered Rust drivers, provider-specific configuration, health gates, and Debian/Ubuntu platform mappings. The existing status/plan/apply, atomic configuration rollback, systemd lifecycle, audit/event, and schema-v2 state paths are reused without adding provider branches to orchestration.

Every new service binds to loopback. MinIO uses generated root credentials and a hardened Lumic-owned dynamic-user unit; Grafana receives a generated administrator password; Prometheus receives a bind-aware systemd override; OpenSearch disables packaged demo security during installation and runs single-node with its security plugin disabled, so it cannot be exposed beyond loopback. External package sources must already be configured by the operator. Backup/restore and child-resource operations return explicit unsupported errors. Catalog and driver behavior have workspace unit coverage; live provider lifecycle coverage remains future work and is not included in the supported-host claim.

### 2026-08-10 — Reusable application resources complete

Immutable artifacts now use a shared manager rather than recipe-local download code. The manager validates versioned HTTPS definitions, serializes concurrent acquisition with an artifact lock, rejects symlink/non-file cache entries, streams SHA-256 verification, uses a private temporary download and atomically commits only verified bytes. WordPress consumes this manager for both apply and inspection, so cached bytes are reverified before use.

Application processes validate in the core domain and render direct argument-vector systemd services in the platform adapter. Enabled workers and jobs are enabled and started; disabled definitions are stopped and disabled. Schedule intent is backend-neutral: calendar or interval timing, missed-run behavior, and optional jitter are represented without systemd vocabulary, then mapped to timers by the Linux adapter. Legacy persisted schedule strings deserialize as calendar schedules and are rewritten in the structured representation.

Application package requirements become trusted only after review by `PackagePolicy`. Lifecycle plans expose the reviewed requirement, its operational reason, and its built-in policy trust source; syntactically valid but unreviewed packages remain denied. Focused unit coverage and the full workspace quality gates verify the Phase 9 boundary. The final-verification checklist remains open until Phase 10 and end-to-end acceptance are complete.

### 2026-08-10 — Catalog-driven interfaces complete

CLI service discovery, detection, install planning, and apply now accept stable catalog definition IDs rather than a provider enum. The CLI publishes the trusted catalog and individual schemas, and every lifecycle action supports a non-mutating dry run. Provider-specific database, backup, and configuration commands remain capability adapters over the same managed instances rather than being generalized into arbitrary commands.

`ResourceFramework` is the shared adapter boundary for catalog/schema reads, secret-redacted resource inspection, bindings, and durable pipeline operations. MCP exposes that boundary through `resource_catalog`, `resource_schema`, `resource_plan`, `resource_apply`, `resource_inspect`, binding create/remove/list, and operation list/detail tools. Binding writes take the global binding resource lock and validate referential integrity, producer outputs, unique consumer inputs, and cycles before the schema-v2 state is committed. Existing `managed_service_*` data-operation tools remain documented compatibility aliases; the closed CLI provider argument and obsolete standalone MCP executable/documentation are removed.

The existing authenticated UI now renders service catalog cards, schema metadata, CSRF-protected catalog installs, managed instance details, and application lifecycle journals. Application pages link to operation details showing every step, progress count, failure message, and recovery outcome. CLI, UI, and MCP therefore consume one catalog/resource contract and continue to delegate native mutations to the existing drivers and application pipelines.
