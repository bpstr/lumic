# Lumic implementation roadmap

Lumic development has two lanes:

1. **Fast-track epics** — manually triggered, high-intensity Codex runs that build major product capabilities and move Lumic rapidly toward the complete product.
2. **Nightly** — continuous hardening, test expansion, bug fixes, support breadth and additional integrations built on top of the established capabilities.

The fast track optimizes for a stable base and useful features, not speculative abstraction. Add architecture only where an actual capability needs a boundary.

## Development rule

A manual epic builds the **mechanism**. Nightly expands the **catalog**.

Examples:

- Manual epic: build the application recipe engine and one reference recipe.
- Nightly: add Drupal, Symfony and other recipes after the WordPress lifecycle proof.
- Manual epic: build managed-service lifecycle and one or two reference services.
- Nightly: add more services using the same contract.
- Manual epic: build framework-aware environment integration.
- Nightly: add more framework/service combinations.

Do not stall the fast track to achieve broad ecosystem coverage.

---

## Phase 0 — Foundation

Implemented: the foundation reports live distribution, node, kernel, CPU, memory/swap and root-filesystem facts and provides the shared core/platform contracts used by the CLI, daemon and MCP adapters.

- Rust workspace and stable crate boundaries.
- Host/OS detection.
- Safe process execution boundary.
- Capability/policy skeleton.
- CLI, daemon and MCP foundations.
- Installer and update channels.
- Multi-image CI.
- Agent/development contracts.

**Exit:** Lumic installs, starts, reports status, and has safe primitives suitable for real host operations.

## Phase 1 — Trusted host operations

Epic A mechanism implemented: apt and systemd use validated typed adapters; host status includes CPU/memory/swap/disk and diagnosis adds live load/process/failed-service evidence; mutations write persistent events and before/after audit records; material file updates use atomic writes with recovery copies.

- apt operations through typed policy-controlled capability.
- systemd lifecycle operations.
- package/component catalog.
- CPU, memory, disk, load, process and service inspection.
- filesystem/config atomic-write helpers.
- structured events and audit storage.
- initial `lumic diagnose`.

**Exit:** Lumic can safely operate the Linux host without exposing arbitrary shell execution.

## Phase 2 — Applications and runtimes

Epic A mechanism implemented with intentionally narrow catalogs: persistent static/PHP/Node application state, PHP-FPM and an explicit extension set, Node build/proxy foundation, validated nginx configuration/reload recovery, named SSH key references, Certbot TLS, and systemd workers/timers. Static Git and generic PHP Git are the only acceptance references.

- application model and persistent state.
- PHP runtime and extension/component management.
- Node runtime foundation.
- nginx managed service.
- domains and application environment handling.
- Git source/credential model.
- worker and scheduled-process model.
- TLS lifecycle.

**Exit:** Lumic can provision a normal web application stack on a clean VPS.

## Phase 3 — Deployment engine

Epic A release mechanism implemented: external Git mirror/fetch, isolated releases, typed build phases, entry-point validation, atomic activation, local HTTP health gates, automatic and manual rollback, retention, phase history, events and audits. Push/webhook triggers and runtime draining beyond the reference process model remain later work.

- immutable release directories.
- build lifecycle.
- atomic activation.
- long-running process activation/draining strategy.
- health gates.
- automatic rollback.
- deployment history and events.
- Git webhook trigger substrate.

**Exit:** a Git application can be deployed and rolled back without manually SSHing into the machine.

## Phase 4 — Managed services and data

Epic B mechanism implemented and extended by the resource-framework work: managed identity/configuration/secrets/dependencies, read-only native detection, plan/install/update/remove/lifecycle, health/log hooks, events/audits, MySQL/PostgreSQL database/user/grant primitives, MySQL/PostgreSQL/Redis local backup/restore, typed application references, and role-scoped database/credential bindings. The native catalog also includes Typesense, Meilisearch, Valkey, RabbitMQ, MinIO, OpenSearch, Memcached, MongoDB, ClickHouse, Prometheus, Grafana, and Loki with provider configuration and health gates.

- managed service lifecycle contract.
- service configuration/state/secrets.
- database/user primitives.
- backup/restore primitives.
- Redis/Valkey-style cache service reference.
- PostgreSQL reference.
- generic service health/log/event integration.

Nightly expands provider-native backup/restore, child resources, live-host coverage, and further catalog breadth such as Agnative and NATS.

**Exit:** services are first-class Lumic resources rather than ad-hoc packages.

## Phase 5 — Operator UI

Epic B initial UI implemented as a loopback-only server-rendered Rust adapter with hashed admin-token authentication, in-memory sessions, CSRF-protected confirmations, security headers and shared-service actions.

- Rust UI shell and authentication.
- minimal black/white Lumic design system.
- server overview.
- applications.
- services.
- deployments.
- logs/events.
- progressive expert details.
- policy/approval views.

The UI remains an adapter over the same application services used by CLI/MCP.

**Exit:** Lumic is pleasant to operate without a terminal while preserving expert transparency.

## Phase 6 — Application recipes

Implemented: a compiled-in, versioned declarative schema composes runtimes/components, managed services, domain/TLS, declared environment inputs, private generated secrets and fixed setup operations. Catalog/list/inspect/plan/install/update/uninstall are shared by CLI/MCP, the UI shows catalog/installations, and `static-git@1.0.0` proves the generic mechanism. `wordpress@1.0.0` proves a complete checksum-pinned, idempotent PHP/MySQL/nginx application lifecycle with durable progress, rollback, secrets, health, and safe removal. Remote signed catalog distribution and further ecosystem breadth remain nightly work.

Build the Installatron-like application provisioning substrate.

- declarative recipe schema.
- dependency/capability composition.
- runtime/service/domain/TLS composition.
- setup lifecycle and validation.
- versioned/signed recipe distribution design.
- one simple reference recipe proving the mechanism.

Nightly owns further breadth: Laravel, Symfony, Drupal, Forgejo, Ghost, Matomo, etc. Gitea and Gogs are implemented as artifact-backed managed services sharing Lumic's repository root.

**Exit:** new application installers can usually be added as data/recipe work rather than core Rust changes.

## Phase 7 — Complete host operator

Implemented: one shared typed host operator covers accounts/groups, safe path permissions, UFW inspection/rules, listeners, mounts/capacity, process inspection/fixed signals, systemd timers, pending/security updates, bounded journal search, managed-service backup timers and a narrow deterministic remediation catalog. CLI and MCP expose the full surface; the UI exposes the important read model and confirmed security updates. Diagnosis now correlates filesystem pressure and pending security updates.

Eliminate the remaining common reasons to SSH into the VPS.

- users and permissions.
- firewall and listening-port inspection.
- storage/filesystem management.
- timers/jobs.
- package/security updates.
- process inspection/control.
- journal/system/application log search.
- richer diagnostics.
- deterministic remediation actions.
- backup scheduling.

**Exit:** normal operation and troubleshooting are possible through Lumic CLI/UI/MCP.

## Phase 8 — Git and environment management

Implemented: Lumic creates native bare repositories, refreshes native mirrors with optional imported credential references, installs a fixed validated push-to-deploy hook, and exports versioned portable application definitions. Import requires an explicit tier/domain plus target-local secret and service transforms; configuration diff redacts secret references. The static Git workflow is the reference integration.

- hosted bare Git repositories.
- deploy keys and credentials.
- repository mirrors/caches.
- push-to-deploy substrate.
- environment export/import.
- environment cloning and transformation.
- application configuration diffing.

**Exit:** Lumic can host source when desired and reproduce/clone managed application environments.

## Phase 9 — Multi-node foundation

Implemented: each node has a persistent Ed25519 identity and public enrollment document, explicit trust/revocation, typed roles, peer health evidence, resource endpoints, worker/reverse-proxy memberships, and a consolidated infrastructure read model. Signed remote requests are short-lived, target-bound, allowlisted and replay-protected; nodes remain autonomous and the agent transports requests between endpoints.

Keep this conventional and simple; do not build Kubernetes.

- node identity and trust.
- node discovery/registration.
- explicit relationships between Lumic nodes.
- node roles: app, worker, database, cache, Git, media, backup, edge.
- infrastructure read model.
- safe cross-node operation orchestration.

**Exit:** multiple autonomous Lumic nodes can be reasoned about and operated as one explicit infrastructure topology.

## Phase 10 — Environments and coordinated deployment

Implemented: production/staging/development bundles share application configuration while requiring transformed domains and target-local secret/service references. Coordination records an explicit member plan, node-local outcomes, health and a stop-on-first-failure/targeted-rollback boundary; it is not a distributed scheduler. The CI smoke test constructs production and staging nodes, deploys both, and completes coordination without SSH.

- production/staging/development environment model.
- clone/transform environment workflows.
- coordinated multi-node application deployments.
- load-balancer/reverse-proxy membership substrate.
- worker-node addition/removal.
- service endpoint propagation.
- cross-node health verification and rollback boundaries.

**Exit:** Codex can take two clean Lumic VPSs and construct separate environments without manual SSH configuration.

## Phase 11 — Observability

Implemented in Epic E: private five-minute host/process/application/managed-service snapshots, durable event folding, selected kernel/OOM ingestion, typed provider hooks, correlated filters and evidence-only incident reports. The reusable mechanism is complete; additional metrics and provider collectors are nightly breadth.

- durable time-series-ish operational snapshots appropriate for a lightweight node agent.
- CPU/memory/disk/network/process/service history.
- application and deployment health history.
- nginx/runtime/database/cache signals through provider hooks.
- OOM/kernel/system event ingestion.
- correlated operational timeline.
- `lumic diagnose` evidence reports.

**Exit:** Lumic can explain what changed around an incident instead of showing only current metrics.

## Phase 12 — Notifications and automation

Implemented in Epic E: signed generic webhook destinations, exact subscriptions, bounded queue/retry/history and the reference `service.failed -> typed restart -> verify` rule with cooldown/attempt protection. Destination adapters and rule breadth are nightly work.

- event subscriptions.
- generic signed outbound webhooks.
- delivery retry/history.
- notification destinations substrate.
- deterministic rules: condition -> typed Lumic action -> verification -> notification.
- escalation and cooldown protections.

Nightly may add Slack, Discord, email and other destinations.

**Exit:** Lumic actively participates in operating the server rather than waiting for commands.

## Phase 13 — Advanced operational safety

Implemented in Epic E: new material operations configuration follows plan/apply with recoverable snapshots, automation plans expose impact hooks, existing update recovery is retained, managed backups gain SHA-256/native-format verification, and MCP mutations require scope plus approval. Rich dependency expansion, hardening profiles and per-identity remote authorization remain later work.

- plan/apply for material changes.
- dependency impact previews.
- richer audit correlation.
- configuration rollback snapshots.
- update safety and recovery.
- backup verification.
- security hardening profiles.
- policy scopes suitable for autonomous MCP agents.

**Exit:** increasingly autonomous operation remains understandable and reversible.

## Phase 14 — Context & server intelligence

Lumic learns what the application and server mean together.

Implemented in Epic F: `ApplicationIntelligence` provides evidence/confidence-based deployed-source fingerprinting, key-only dotenv inspection and comment-preserving mutation, integrity-checked snapshots/rollback, a typed dependency graph, compiled integration definitions, bounded/redacted incident context and an optional signed analysis adapter whose output is advisory and restricted to typed remediation proposals. `laravel-redis@1` is the deliberately narrow reference: it selects or installs managed Redis, previews redacted environment changes, restarts only detected queue/Horizon workers, verifies Redis and application health, attaches the typed service reference and records the mutation. Laravel/Typesense, Drupal and further combinations remain nightly catalog breadth.

- framework/application fingerprinting.
- environment/configuration discovery.
- application <-> runtime <-> service dependency graph.
- integration recipe substrate.
- safe environment mutation with backup/diff/rollback.
- post-integration verification.
- dependency-impact reasoning.
- incident aggregation and correlation.
- structured incident context packages for LLM/webhook analysis.
- AI-generated diagnosis/remediation proposals executed only through normal typed Lumic operations.

Reference behavior should prove flows such as:

`Laravel app + install Redis` -> discover the app -> discover `.env` -> provision service -> configure connection -> update relevant environment -> restart affected workers -> verify app/service connectivity -> record the changes.

`Laravel app + install Typesense` should be achievable by the same generic mechanism once a Typesense integration definition exists; nightly can add the breadth of such definitions.

**Exit:** Lumic understands relationships and can safely wire common infrastructure into applications instead of merely installing packages next to them.

## Phase 15 — Personality

A small, deliberately fun presentation layer over factual Lumic state.

Implemented in Epic G: the canonical attention model combines live host diagnostics, managed application/service state, backup results and recent events. CLI, UI and MCP share that model; six deterministic personalities affect wording only. Personality configuration is private, atomic and audited. Clean-image acceptance coverage and the first publishability documentation pass are complete; additional evidence providers are nightly breadth.

- optional per-node personality.
- factual status summary model separated from presentation.
- personality-aware CLI/UI conversational summaries.
- MCP resource/tool for "how are you doing?" style status.
- personalities such as professional, dry, grumpy, paranoid, cheerful and `idiot`.
- event/recovery/deployment messages may use personality.
- severity, evidence and recommended actions must never be hidden or altered by personality.

Example:

> Redis fell over at 03:14. I restarted it, checked connectivity, and everything recovered six seconds later. It has been warned.

**Exit:** servers can feel alive and memorable without compromising operational truth.

---

# Delivery target

The goal is to deliver the roadmap with:

- narrow but real reference implementations;
- stable reusable capability contracts;
- excellent end-to-end behavior;
- strong security boundaries;
- CI coverage for every mechanism.

Do **not** delay a coherent capability for broad integration support. Once the mechanism works, track support-matrix expansion separately.

# Continuous hardening

Ongoing work should primarily:

- fix bugs/regressions/security findings;
- strengthen tests and supported-OS behavior;
- add runtime versions and components;
- add managed service definitions;
- add application recipes;
- add framework/service intelligence integrations;
- add notification destinations;
- improve UX/docs;
- improve performance and reliability;
- respond to real VPS findings.

Ongoing work should not casually invent a new core architecture when a smaller additive mechanism satisfies the product contract.

# Definition of done for any capability

Implementation + tests + CLI/API/MCP mapping where relevant + policy + audit/events + documentation + supported-OS CI. No feature is complete because a command worked once on one machine.
