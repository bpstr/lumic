# Lumic implementation roadmap

Lumic development has two lanes:

1. **Fast-track epics** — manually triggered, high-intensity Codex runs that build major product capabilities and move Lumic rapidly toward the complete product.
2. **Nightly** — continuous hardening, test expansion, bug fixes, support breadth and additional integrations built on top of the established capabilities.

The fast track optimizes for a stable base and useful features, not speculative abstraction. Add architecture only where an actual capability needs a boundary.

## Development rule

A manual epic builds the **mechanism**. Nightly expands the **catalog**.

Examples:

- Manual epic: build the application recipe engine and one reference recipe.
- Nightly: add Drupal, WordPress, Symfony and other recipes.
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

Epic B mechanism implemented: managed identity/configuration/secrets/dependencies, read-only native detection, plan/install/update/remove/lifecycle, health/log hooks, events/audits, PostgreSQL database/user/grant primitives, PostgreSQL/Redis local backup/restore and typed application references. PostgreSQL and Redis are the deliberately minimal reference set.

- managed service lifecycle contract.
- service configuration/state/secrets.
- database/user primitives.
- backup/restore primitives.
- Redis/Valkey-style cache service reference.
- PostgreSQL reference.
- generic service health/log/event integration.

Nightly expands the catalog to MariaDB, Typesense, Meilisearch, Agnative, MinIO, RabbitMQ, NATS and others.

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

Build the Installatron-like application provisioning substrate.

- declarative recipe schema.
- dependency/capability composition.
- runtime/service/domain/TLS composition.
- setup lifecycle and validation.
- versioned/signed recipe distribution design.
- one simple reference recipe proving the mechanism.

Nightly owns breadth: Laravel, Symfony, Drupal, WordPress, Forgejo/Gitea, Ghost, Matomo, etc.

**Exit:** new application installers can usually be added as data/recipe work rather than core Rust changes.

## Phase 7 — Complete host operator

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

- hosted bare Git repositories.
- deploy keys and credentials.
- repository mirrors/caches.
- push-to-deploy substrate.
- environment export/import.
- environment cloning and transformation.
- application configuration diffing.

**Exit:** Lumic can host source when desired and reproduce/clone managed application environments.

## Phase 9 — Multi-node foundation

Keep this conventional and simple; do not build Kubernetes.

- node identity and trust.
- node discovery/registration.
- explicit relationships between Lumic nodes.
- node roles: app, worker, database, cache, Git, media, backup, edge.
- infrastructure read model.
- safe cross-node operation orchestration.

**Exit:** multiple autonomous Lumic nodes can be reasoned about and operated as one explicit infrastructure topology.

## Phase 10 — Environments and coordinated deployment

- production/staging/development environment model.
- clone/transform environment workflows.
- coordinated multi-node application deployments.
- load-balancer/reverse-proxy membership substrate.
- worker-node addition/removal.
- service endpoint propagation.
- cross-node health verification and rollback boundaries.

**Exit:** Codex can take two clean Lumic VPSs and construct separate environments without manual SSH configuration.

## Phase 11 — Observability

- durable time-series-ish operational snapshots appropriate for a lightweight node agent.
- CPU/memory/disk/network/process/service history.
- application and deployment health history.
- nginx/runtime/database/cache signals through provider hooks.
- OOM/kernel/system event ingestion.
- correlated operational timeline.
- `lumic diagnose` evidence reports.

**Exit:** Lumic can explain what changed around an incident instead of showing only current metrics.

## Phase 12 — Notifications and automation

- event subscriptions.
- generic signed outbound webhooks.
- delivery retry/history.
- notification destinations substrate.
- deterministic rules: condition -> typed Lumic action -> verification -> notification.
- escalation and cooldown protections.

Nightly may add Slack, Discord, email and other destinations.

**Exit:** Lumic actively participates in operating the server rather than waiting for commands.

## Phase 13 — Advanced operational safety

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

# Fast-track target

The manually triggered epic sequence is defined in `docs/CODEX_FAST_TRACK.md`.

The goal is to reach Phase 15 quickly with:

- narrow but real reference implementations;
- stable reusable capability contracts;
- excellent end-to-end behavior;
- strong security boundaries;
- CI coverage for every mechanism.

Do **not** delay Phase 15 for broad integration support. Once the mechanism works, open backlog items and let nightly expand it.

# Nightly after the fast track

Nightly should primarily:

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

Nightly should not casually invent a new core architecture or start a large roadmap epic when a manual fast-track epic is intentionally pending.

# Definition of done for any capability

Implementation + tests + CLI/API/MCP mapping where relevant + policy + audit/events + documentation + supported-OS CI. No feature is complete because a command worked once on one machine.
