# Codex fast-track: reach Lumic Phase 15 quickly

This is the manual development lane for Lumic. Run these epics deliberately, one after another, with Codex against `main` or a fresh feature branch. They are intentionally larger than nightly work.

The objective is **feature-complete product machinery**, not broad integration coverage. Build a stable mechanism, prove it with a narrow reference implementation, merge it, then move on. Nightly expands support breadth afterward.

## Rules for every epic

Before work:

- Read `AGENTS.md`, `docs/PRODUCT.md`, `docs/ARCHITECTURE.md`, `docs/SPECIFICATION.md`, `docs/SECURITY.md`, `docs/ROADMAP.md`.
- Inspect current code, open issues/PRs and CI.
- Preserve host-native Linux focus, Rust-only stack, safe typed operations, shared CLI/UI/MCP services and no generic shell MCP tool.
- Do not create an elaborate plugin framework just because more integrations will exist later.
- Prefer a simple trait/adapter or declarative definition only when an actual second implementation proves it useful.
- Every mutation must be auditable; material configuration writes must be recoverable.
- Keep CI green throughout the epic.

At the end:

- run fmt, clippy, tests, audit and relevant supported-OS scenarios;
- add regression/e2e coverage;
- update docs to match reality;
- create issues for integration breadth deliberately deferred to nightly;
- summarize what is now possible end to end and the next epic.

---

# Epic A — Make one VPS complete

**Covers Phases 1–3.**

Goal: a clean Debian/Ubuntu VPS can be installed once and then fully provisioned/deployed through Lumic.

Build the complete vertical mechanism for:

- rich host status and diagnostics;
- apt and systemd typed adapters;
- persistent event/audit state;
- atomic configuration writes;
- application persistent model;
- PHP runtime + extension management;
- Node runtime foundation;
- nginx lifecycle/config validation;
- domains and TLS;
- Git source credentials;
- release directories;
- build phases;
- atomic activation;
- health checks;
- automatic rollback;
- workers/scheduled processes;
- deployment history/events;
- nightly self-update safety;
- useful CLI mappings;
- MCP mappings for the same services.

Prove with two reference applications only:

1. static Git application;
2. generic PHP Git application.

Do not spend the epic on Laravel/Drupal/WordPress-specific behavior.

Acceptance demo:

`fresh VPS -> install Lumic -> create app -> attach Git -> deploy -> TLS -> health -> redeploy -> rollback`, without manual Linux configuration.

---

# Epic B — Managed services + operator UI

**Covers Phases 4–5.**

Goal: services become first-class resources and Lumic becomes pleasant to operate visually.

Build generic managed-service machinery:

- service identity/state/configuration/secrets;
- install/detect/update/remove/lifecycle;
- health and log hooks;
- dependency declaration;
- service events;
- database/user primitives;
- backup/restore interfaces and local reference implementation;
- application-to-service references.

Prove it with only:

- PostgreSQL;
- Redis or Valkey.

Then implement the initial Rust UI using existing shared services:

- authentication/session;
- black/white minimal design system;
- server overview;
- application list/detail;
- service list/detail;
- deployment history/detail;
- events/logs;
- safe actions such as restart/deploy/rollback with clear confirmation where material;
- progressive expert details exposing actual systemd paths/config/version/ports rather than hiding Linux.

Do not add a large frontend framework or a separate business-logic stack.

Acceptance demo:

`install PostgreSQL + Redis -> create database -> inspect in UI -> deploy app -> view deployment/events -> restart service -> observe health`.

Nightly follow-up issues: MariaDB, Typesense, Meilisearch, Agnative, MinIO, RabbitMQ, NATS, UI polish.

---

# Epic C — Recipes + complete host operator

**Covers Phases 6–7.**

Goal: create the machinery that makes Lumic both Installatron-like and a practical OS manager.

Build a simple declarative recipe system capable of composing existing Lumic capabilities:

- required runtimes/components;
- required managed services;
- domains/TLS;
- environment values and generated secrets;
- setup steps represented as safe known operations where possible;
- validation;
- idempotent install/update/uninstall semantics;
- versioned recipe metadata;
- one simple reference application recipe.

Do not build a general programming language in YAML.

Complete common host-management capabilities:

- users/groups/permissions;
- firewall and port inspection/rules;
- filesystem/storage inspection;
- process inspection/control;
- timers/jobs;
- package/security updates;
- journal/log search;
- backup scheduling;
- deterministic remediation actions;
- richer `lumic diagnose`.

CLI/UI/MCP should all expose the important operations through the same services.

Acceptance demo:

A non-DevOps developer can inspect, maintain and troubleshoot a VPS without needing ordinary SSH administration.

Nightly follow-up issues: Laravel, Drupal, WordPress, Symfony, Ghost, Forgejo and other recipes; additional remediation rules.

---

# Epic D — Git, environments and multi-node infrastructure

**Covers Phases 8–10.**

**Status:** implemented on `main` with the intentionally narrow two-node static Git reference workflow. Provider breadth and network transport hardening remain nightly follow-ups.

Goal: two or more Lumic nodes can be treated as explicit infrastructure without Kubernetes-like complexity.

Build:

- hosted bare Git repositories;
- deploy keys/credential management;
- repository mirrors/caches;
- push-to-deploy trigger substrate;
- environment export/import;
- application/service configuration diff;
- clone + transform workflows;
- node identity/trust relationship;
- node registration/discovery;
- infrastructure read model;
- node roles;
- explicit service/application endpoint relationships;
- safe remote Lumic-to-Lumic operations;
- production/staging/development environment model;
- coordinated deployment primitive;
- worker membership primitive;
- reverse-proxy/load-balancer membership primitive;
- cross-node health checks and failure boundaries.

Keep nodes autonomous. Do not create consensus systems, schedulers, overlay networks or Kubernetes abstractions.

Acceptance demo:

Give Codex access to two fresh Lumic nodes and ask it to create production on one and staging on the other, sharing the same application definition with transformed domains/secrets/resources. No manual SSH configuration after Lumic installation.

---

# Epic E — Observability, events and active operations

**Covers Phases 11–13.**

Goal: Lumic becomes an active server participant and can explain incidents.

Build lightweight durable operational history:

- host resource snapshots;
- process/service history;
- application health history;
- deployment markers;
- system/kernel/OOM event ingestion;
- provider hook points for service-specific signals;
- correlated timeline queries;
- evidence-based diagnostic reports.

Build active automation:

- generic signed outbound webhook destination;
- subscriptions/filters;
- delivery queue, timeout, bounded retry and history;
- typed deterministic rules;
- cooldown/retry protections;
- verify-after-remediation;
- notification substrate.

Build safety improvements:

- plan/apply for material changes;
- dependency-impact preview hooks;
- configuration snapshots/rollback;
- stronger update recovery;
- backup verification;
- MCP permission scopes/approval needs suitable for autonomous agents.

Acceptance demo:

Cause a controlled service/application failure. Lumic should record the timeline, identify affected resources, optionally perform a predefined safe remediation, verify recovery, and emit a signed webhook with useful structured context.

Nightly follow-up issues: notification destinations, more metrics/providers, more deterministic rules.

---

# Epic F — True server intelligence

**Covers Phase 14. This is a major product feature.**

Goal: Lumic understands the relationship between hosted applications and infrastructure and can safely wire them together.

Do not build an unrestricted LLM agent inside the daemon. Intelligence must operate over deterministic discovery, dependency models and normal typed Lumic operations.

Build:

## Application fingerprinting

Detect useful application facts from deployed source/configuration:

- framework/CMS where confidently identifiable;
- runtime;
- environment file locations;
- dependency manifests;
- worker/scheduler hints;
- existing service configuration;
- health endpoints where discoverable.

Keep detection evidence/confidence available.

## Configuration discovery

Create adapters for environment/config sources, beginning with dotenv-style files:

- inspect keys without exposing secret values unnecessarily;
- safe mutation;
- backup/snapshot;
- diff preview;
- rollback;
- preserve comments/format where practical, but correctness is more important than perfect formatting.

## Dependency graph

Represent relationships such as:

`application -> Redis -> Horizon workers`

`application -> PostgreSQL`

`application -> Typesense`

`application -> runtime -> nginx`

Use this graph for impact previews, diagnostics and integration actions.

## Integration definitions

Build a deliberately small mechanism that describes how a managed service is connected to a recognized application/framework.

Prove the mechanism with one high-value reference integration:

**Laravel + Redis**

Expected behavior:

- detect Laravel;
- locate `.env`;
- inspect relevant existing variables/dependencies;
- install or select Redis;
- generate/use safe credentials if needed;
- preview environment changes;
- apply changes;
- determine affected workers/processes;
- reload/restart only what is required;
- verify service connectivity/application health;
- rollback on failed verification where feasible;
- record every change/event.

Do not hardcode this workflow in unrelated core modules. The point is to prove a reusable integration mechanism.

Nightly will add `Laravel + Typesense`, additional Laravel services, Drupal integrations and other framework/service combinations.

## Incident intelligence

Build incident aggregation:

- group temporally/causally related health failures, deployment events, resource spikes, service failures and logs;
- identify affected dependency graph nodes;
- generate a structured incident context package containing evidence, not conclusions.

Support an optional LLM/webhook analysis adapter:

`incident context -> configured analysis webhook/LLM -> structured diagnosis + proposed remediation`

Requirements:

- secrets/redaction policy;
- bounded payloads;
- evidence references;
- AI output is advisory by default;
- any remediation must resolve into normal typed Lumic operations and policy checks;
- no arbitrary root shell returned to an LLM.

Acceptance demos:

1. Ask Codex/Lumic to add Redis to a recognized Laravel application and see it correctly wire and verify the app.
2. Cause a controlled failure after a deployment and obtain one correlated incident containing timeline, affected dependencies, evidence and an optional AI diagnosis proposal.

---

# Epic G — Personality + publishable product pass

**Covers Phase 15.**

Goal: make Lumic memorable without corrupting operational truth, then perform the first publishability pass.

Build a canonical status/attention summary model containing:

- health severity;
- facts;
- changes since last relevant period;
- active incidents;
- upcoming attention (disk pressure, expiring cert, update, failed backup, etc.);
- recommendations.

Personality is only a renderer on top of this model.

Add optional node personality:

- professional;
- dry;
- grumpy;
- paranoid;
- cheerful;
- idiot.

Support conversational/status surfaces such as:

- `lumic how-are-you` or equivalent natural operator summary;
- UI status copy;
- MCP `server.attention` / status-summary resource/tool;
- personality-aware deployment/recovery/event messages.

Hard rules:

- critical severity remains obvious;
- facts/numbers are not invented;
- personality cannot suppress warnings or recommendations;
- structured MCP fields remain factual regardless of rendered copy;
- deterministic templates should work without an LLM; an optional language model renderer may enhance phrasing later.

Example acceptable output:

> Redis fell over at 03:14. I restarted it, checked connectivity, and everything recovered six seconds later. It has been warned.

Then execute a publishable-product pass:

- one-command install tested on clean supported systems;
- nightly update path tested on a real VPS lifecycle;
- stable rollback/recovery paths;
- MCP onboarding documented;
- first-VPS guide;
- two-node guide;
- application deploy guide;
- incident/intelligence demo guide;
- security limitations clearly documented;
- remove dead/placeholder code and stale docs;
- produce a concise feature matrix of implemented vs nightly-expansion support.

Acceptance demo:

Ask a Lumic-managed node: `How are you doing? Feeling good? Anything need attention?` and receive a funny but operationally accurate answer backed by real status, recent events and recommendations.

---

# Recommended manual trigger prompt

For each epic, tell Codex:

> Read `docs/CODEX_FAST_TRACK.md`. Execute **Epic X** as an intense feature-development mission. First inspect current implementation and skip anything already complete. Work in coherent vertical increments and keep CI green, but continue through the entire epic rather than stopping after one small change. Build the reusable product mechanism and only the minimum reference integrations required to prove it. Explicitly defer ecosystem breadth to nightly GitHub issues. Do not over-architect; prefer the simplest stable design that supports the demonstrated feature. At completion, run the full relevant CI/test suite, reconcile docs, file nightly follow-ups, and give me the acceptance demo commands/workflow.

Do not ask for permission between substeps unless a destructive external action genuinely requires it.

# After Phase 15

Manual epics should become rare. Nightly becomes the main development engine and expands:

- OS/version support;
- runtimes/components;
- managed services;
- app recipes;
- framework/service intelligence definitions;
- observability adapters;
- notifications;
- UI polish;
- tests/security/performance;
- real-host issues discovered in production-like usage.

At that point, new large manual epics should be justified by a genuinely new product capability rather than an integration request.
