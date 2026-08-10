# Lumic capability specification

This document is the implementation-level product contract. It is intentionally broader than the current code and narrower than a wishlist: new features should fit these domains instead of creating parallel abstractions.

## Current implementation boundary

Epics A–F implement the initial vertical slice for x86_64 Debian/Ubuntu: single-node host/application/service/recipe/operator capabilities; native Git hosting and mirrors; portable environments; public-key node identity/trust; explicit topology; replay-protected remote operations; externally orchestrated coordinated deployment; evidence-backed application and incident intelligence; installed stdio MCP; and optional bearer-authenticated loopback Streamable HTTP. MySQL, PostgreSQL and Redis prove managed-service behavior, `static-git` and a two-node static application prove composition and environment transformation, and Laravel + Redis proves safe framework/service integration.

The service/resource redesign includes versioned trusted catalog definitions, reusable configuration schemas, stable resource references, explicit ownership/management state, typed outputs and bindings, typed lifecycle pipelines and recovery journals, cross-process locks, and schema-v2 state with backup-first migration. Catalog-driven CLI, UI, and MCP interfaces share those definitions and expose redacted inspection, validated bindings, and durable operation progress. Provider behavior is selected through the built-in driver registry and compatibility state is stored in the authoritative schema-v2 resource store. Provider breadth, OAuth/per-identity MCP policy, and automatic public TLS setup are not yet complete; progress is tracked in `docs/RESOURCE_FRAMEWORK_PROGRESS.md`.

## 1. Node and host

A Lumic node is one Linux host running Lumic. The node reports live facts rather than relying on a stale inventory copy.

Required fact families:

- distribution/version and architecture;
- hostname, kernel and boot identity;
- CPU count/load;
- memory/swap;
- filesystems, mounts, capacity and IO where available;
- interfaces/routes/listeners;
- processes and systemd units;
- installed packages and pending security/system updates;
- Lumic version/channel/health.

Supported OS policy begins with Debian and Ubuntu. Detection is explicit; unsupported systems fail clearly rather than executing guessed commands.

## 2. Packages

Lumic wraps native package managers. It does not become one.

Operations:

- search/list/version;
- plan install/remove/upgrade;
- install/remove/upgrade approved package;
- repository/source inspection;
- trusted repository management later.

Security:

- package identifiers are validated as data, never shell fragments;
- policy must authorize package/repository;
- exact allowlists first; reviewed prefixes/version constraints may follow;
- apt is invoked via executable + argv;
- operation result captures exit status and bounded output;
- mutations emit audit/event records.

## 3. Components

Components are dependencies attached to another managed capability, not standalone services.

Examples:

- PHP extensions (`intl`, `redis`, `imagick`);
- PostgreSQL extensions (`pgvector`, `postgis`);
- runtime libraries/drivers.

Contract: detect compatibility -> plan dependencies -> install -> configure/enable -> validate -> remove/disable where safe.

## 4. Managed services

A managed service owns meaningful lifecycle/configuration beyond package installation.

Contract:

- detect;
- plan/install;
- inspect config/state/version;
- start/stop/restart/reload;
- enable/disable startup;
- validate config;
- health;
- logs;
- upgrade plan/apply;
- backup hooks when applicable;
- events;
- uninstall/recovery rules.

Implemented reference direction: MySQL, PostgreSQL, Redis, Typesense, and Meilisearch use allowlisted native Debian/Ubuntu apt packages, systemd, loopback-only validated configuration, provider health/log hooks, and persistent state/events. MySQL, PostgreSQL, and Redis additionally implement local backup/restore. Trusted Rust drivers referenced by built-in catalog definitions own their platform mapping, validation/rendering, paths, health, supported backup/restore plans, and child-resource commands. The compatibility manager executes those typed plans without closed provider dispatch, and persists through the schema-v2 resource store while preserving its existing CLI/UI/MCP contract. Relational database/user/grant resources publish database and sensitive credential-reference outputs; applications can bind multiple role-owned database pairs after grants are recorded. Typesense and Meilisearch generate private service credentials and publish reusable HTTP endpoint plus sensitive credential-reference outputs; application bindings protect a consumed search service from removal. Their third-party apt sources remain an explicit operator prerequisite. nginx is independently installed and persisted as a singleton managed service; its owned web-host resources use atomic validated configuration and explicit bindings. PHP runtimes are versioned resources that publish deterministic FPM/CLI outputs and own version-qualified extension components. The reviewed WordPress recipe implements a checksum-pinned, idempotent PHP/MySQL/nginx lifecycle with durable operation state, rollback, secret references, health, and safe removal through the shared verified artifact manager. Application lifecycle plans expose package requirements only after policy review and identify the source from which trust was derived. The fixed software-installer catalog separately exposes package setup through UI and MCP. NVM uses an explicit account, a pinned official Git tag and managed profile activation; other installer entries remain native package setup rather than managed-provider configuration.

Managed resources may declare typed dependencies on another managed resource. MySQL and PostgreSQL implement validated database/user/grant primitives; generated passwords are private secret references and are passed to the native client via stdin. Applications store typed service/database/user references plus explicit database and credential bindings without copying secret values. Automatic environment wiring is a later integration contract.

## 5. Applications

An application is a first-class managed resource, not merely an nginx virtual host.

Possible properties:

- identity/name;
- Git source or local hosted Git repository;
- runtime and components;
- system packages;
- environment/secrets references;
- web/domain routing;
- TLS;
- persistent/shared storage;
- databases/services it consumes;
- workers/processes;
- scheduled jobs;
- health checks;
- deployment strategy/history;
- logs/events.

Initial runtime families: PHP, Node, Python, static/custom process. Containers are an additional workload type, not the base runtime model.

## 6. Git

Git is first-class infrastructure.

Modes:

1. external deployment source (GitHub/GitLab/generic Git);
2. Lumic-hosted native bare repositories, currently consumed through the host's explicitly configured Git transport;
3. local mirror/cache used as deployment source for multiple nodes.

Credentials are secret references and must never be casually surfaced through status/log/MCP output.

The hosted repository hook executes only `lumic git receive <validated-repository>` and accepts only the configured branch mapping. It does not accept a command string. Mirror synchronization invokes Git with separated arguments and resolves an optional credential reference from private node state.

## 7. Deployment

Zero-downtime is a core application property where the runtime permits it.

Canonical lifecycle:

```text
resolve source
  ↓
prepare isolated release
  ↓
install dependencies / build
  ↓
link shared resources
  ↓
pre-activation tasks + validation
  ↓
runtime-specific activation
  ↓
health checks
  ↓
post-activation tasks
  ↓
retention / cleanup
```

Release layout may follow:

```text
/var/lib/lumic/apps/<app>/
├── releases/<release-id>/
├── shared/
└── current -> releases/<release-id>
```

Activation examples:

- PHP/static: atomic symlink switch + graceful web/runtime reload as required;
- long-running service: start new version -> health -> route/switch -> drain old -> stop old.

Migrations must be modeled explicitly because not every migration is backward compatible. Lumic should surface risk rather than promise impossible zero downtime.

Deployment records include source commit, actor, timings, build/activation health, events, current/previous release and rollback eligibility.

## 8. Databases

Database management must separate server/service lifecycle from individual databases/users.

Capabilities eventually include:

- create/drop database (drop high-risk);
- create/rotate/revoke user credentials;
- grants;
- connection/status facts;
- backup/restore;
- size/connections/health;
- extension components.

Secrets are generated/handled without leaking into routine audits.

## 9. Jobs and processes

Lumic models long-running workers and scheduled jobs as application resources. The current process contract carries a validated direct argument vector and enabled state. Schedule timing is backend-neutral and supports calendar or interval intent, missed-run behavior and bounded jitter; the Linux adapter maps it to systemd services and timers. Workers restart on failure. Desired count, per-process environment, configurable working directory, restart policy and process health remain future contract extensions rather than implied current behavior.

## 10. TLS and web routing

Lumic manages domain routing and certificate lifecycle as application capabilities. Certificates have stable resource identity, an explicit provider, validated DNS names, non-secret inspection data, and bindings to owned web-host consumers. Provider planning is read-only; preflight proves provider availability and nginx validity before issue or renewal.

The built-in Certbot/Let's Encrypt provider uses named `certonly` issuance so Certbot does not become the configuration owner. Lumic attaches the inspected live paths to nginx itself, validates (`nginx -t`) before reload, and saves the prior HTTP configuration for detach and rollback. Provider, attachment, and state failures do not leave a newly issued certificate silently active. The existing application TLS adapter emits issuance and failure audit/event evidence; broader lifecycle adapter exposure is tracked separately from the underlying provider contract.

## 11. Observability and diagnosis

Lumic is an active server participant. It continuously records selected operational events and facts without trying to replace a full telemetry platform.

Useful signals:

- CPU/load/memory/swap/disk;
- process/service state;
- OOM/kernel/system events;
- web 4xx/5xx and upstream failures where configured;
- application health;
- database/cache health;
- deployment/configuration events.

`lumic diagnose` returns structured evidence and detected correlations, e.g. deployment timing near a worker spike or OOM event. AI clients can reason over this evidence.

Implemented reference mechanism: `lumicd` captures host/process/application/managed-service state every five minutes, imports new Lumic events, ingests selected kernel/OOM journal lines and stores typed provider hooks in a private append-only operations timeline. Timeline and incident queries filter by time, event and resource. Incident output is an evidence package, not a guessed root cause. Broader network/runtime/nginx/provider metrics remain nightly expansion.

### Application and incident intelligence

`ApplicationIntelligence` is the implemented Phase 14 application service. A fingerprint reports recognized framework/CMS, declared runtime, environment files, manifests, worker/scheduler hints, configured key names, likely health endpoints, evidence and confidence. Detection reads the active managed deployment and requires both `composer.json`'s `laravel/framework` dependency and `artisan` for high-confidence Laravel.

Dotenv inspection returns key names, sensitivity and duplicates without values. Integration planning returns only configured/unset state. Apply preserves unrelated lines/comments, rejects duplicate target keys, snapshots the original file privately, uses atomic writes, restarts only affected worker units and verifies managed-service plus application health. Snapshot rollback validates application ownership, target path and SHA-256 content integrity.

The versioned compiled integration catalog currently contains only `laravel-redis@1`. Its plan selects an existing managed Redis service or includes installation, resolves loopback host/port, updates Redis/cache/session/queue settings, adds an application service reference and models `application -> Redis -> queue/Horizon worker` plus `application -> runtime -> nginx`. A password is intentionally unnecessary for the loopback-only reference service; future remote/authenticated adapters must use secret references rather than clear state.

Incident context combines the existing factual operations report with affected dependency nodes and evidence IDs, truncates excessive signals and recursively redacts sensitive payload fields. Optional analysis sends this bounded context to an enabled signed webhook destination. It accepts only a closed structured response containing diagnosis, supplied evidence references and typed restart/configuration-rollback proposals. All output is advisory; execution remains a separate normal plan/apply operation under policy.

## 12. Events, notifications and webhooks

Example events:

- `server.booted`, `server.updated`;
- `service.failed`, `service.recovered`;
- `deployment.started|succeeded|failed|rolled_back`;
- `certificate.issued|renewed|renewal_failed|expiring`;
- resource threshold events;
- backup success/failure;
- repository update;
- security updates available.

Generic signed HTTP webhooks are the foundational outbound integration. Email/Slack/Teams/etc. can be adapters later. Rules may perform narrow deterministic remediation (for example restart a failed known service twice, verify, then notify). Never give an LLM autonomous arbitrary remediation because a threshold fired.

The implemented webhook uses HTTPS (loopback HTTP only for tests), a secret reference, HMAC-SHA256 headers, a 256 KiB envelope bound, per-destination timeout, exponential bounded retry and retained outcome history. Subscriptions match exact event/entity filters. The reference rule handles `service.failed` with a validated `.service` target, cooldown, at most three attempts and `active_state=active` verification. Rule/destination configuration follows plan/apply and retains a rollback snapshot. MCP additionally requires an explicit process scope and per-call approval.

## 13. Audit

Every mutation records at minimum:

- timestamp;
- node;
- actor/interface;
- capability/operation;
- validated arguments with secrets redacted;
- correlation/request ID;
- before/after summary where practical;
- result/duration/failure.

## 14. Status, suggest, plan, apply

Lumic's core interaction model is deliberately explicit:

```text
status  -> what exists now?
suggest -> what would make sense?
plan    -> what exactly will change?
apply   -> perform the approved change
```

`status` and `suggest` are read-only. `plan` is read-only unless an explicit persistent-plan feature is introduced later. `apply` is the mutation boundary.

### Suggest

Suggestion is first-class reasoning support for humans and coding agents. It is not a replacement for agent reasoning and must not mutate infrastructure.

Representative CLI shapes:

```text
lumic suggest laravel
lumic suggest nextjs
lumic suggest --path /srv/app
```

Representative MCP tool:

```text
suggest_application_setup
```

Possible inputs:

- explicit stack/framework;
- repository/application path;
- desired application role;
- optional detected repository metadata supplied by the caller.

Repository inspection may use framework and manifest signals such as `composer.json`, `package.json`, lock files, runtime version files, `pyproject.toml`, `Cargo.toml`, environment examples, migrations, queue/worker dependencies, scheduler configuration and persistent storage conventions.

Structured output should include where applicable:

- detected stack/framework/runtime/package manager;
- required runtime versions/components/extensions;
- recommended backing services;
- web/process model;
- workers and scheduler requirements;
- persistent/shared paths;
- recommended deployment strategy;
- warnings/ambiguities;
- source evidence for every significant inference.

Example evidence should say that Redis is recommended because queue/cache usage was detected, or that a Laravel public directory is inferred from framework conventions plus repository structure. Do not emit opaque recommendations.

Suggestion can use known stack/recipe knowledge, but a recipe and a suggestion remain distinct: a recipe describes a reusable known pattern; a suggestion adapts that knowledge to the inspected project and returns evidence.

The coding agent combines suggestion output with live `server.status` facts to choose sizing, topology and final desired state. Lumic does not need prompts like “configure this appropriately for an 8 GB server” as part of its own suggestion contract.

## 15. Plans and approvals

Material changes should support plan/apply. A plan communicates current/desired state, changes, preconditions, risk, validation, expected service impact and rollback/recovery availability.

Approval policy is capability/risk based. Routine deploy/restart may be allowed while database deletion, firewall changes, OS upgrades or raw execution require approval.

Suggestions inform; plans execute. Never allow `suggest` to become an implicit apply path.

## 16. MCP

MCP is a first-class interface, not a CLI wrapper.

Resources should expose server/app/service/deployment/events/diagnostic state. Tools expose typed capabilities. Avoid `execute_shell(command)`.

Representative tools:

```text
inspect_server
inspect_application
suggest_application_setup
install_package
install_runtime
install_component
install_service
restart_service
plan_application
create_application
plan_deployment
deploy_application
rollback_deployment
search_logs
diagnose_server
```

A coding agent connected to two autonomous Lumic nodes must be able to inspect each node and orchestrate staging/production by calling these structured tools independently.

Lumic must not require a separate AI skills package for agents to understand it. MCP tool/resource descriptions, public documentation, catalogs/recipes, inspection and `suggest_application_setup` are the authoritative agent-facing knowledge surfaces.

## 17. UI

The UI is a Rust operational console over the same application services.

Primary information architecture:

```text
Servers
Applications
Services
Infrastructure
Automation
```

Design: black/white/grays, excellent typography, minimal status markers, no decorative hosting-panel clutter. Default views answer “is it healthy and what can I do?” Expanded views reveal systemd unit, config path, PID, ports, versions, logs, metrics and raw facts for experts.

## 18. Recipes and catalog

Recipes provide modern Installatron-style installation without bloating core.

Catalog layers:

- packages;
- components;
- services;
- runtimes;
- application recipes;
- node roles.

Prefer declarative signed/versioned metadata for simple composition. Rust adapters are justified when lifecycle/config logic is genuinely behavioral.

The implemented recipe schema is compiled-in reviewed data and contains version metadata, runtime/components, managed-service requirements, domain/TLS inputs, declared environment sources, generated secret references and fixed health/process setup steps. It is intentionally not a YAML programming language. Recipe plan is read-only; install/update are idempotent compositions over existing services; uninstall removes recipe/application state recoverably while retaining native service data. Remote signed distribution remains a later catalog concern.

Candidate recipes after primitives are mature: Laravel, Symfony, WordPress, Drupal, Forgejo/Gitea, Matomo/analytics apps and other commonly self-hosted software.

## 19. Infrastructure and multiple nodes

Primary scale is ordinary VPS infrastructure, roughly one to ten nodes. Roles may include app, worker, database, cache, Git, media/storage, backup and edge. Nodes remain autonomous; Codex/Claude can initially be the orchestrator across multiple MCP endpoints.

The implemented node identity is an Ed25519 signing key stored as a mode-`0600` target-local secret plus a public enrollment document containing node identity, roles, endpoint, public verification key and fingerprint. Registration is explicit and revocable. Remote requests are signed by a trusted origin, bound to one trusted target, expire within five minutes, use one-time nonces and permit only typed application deploy/rollback operations. Carrying the request between nodes does not grant arbitrary command execution.

Portable environment bundles contain application configuration and secret references, never secret values. Import is an apply boundary: it requires an explicit production/staging/development target, domain, resource transforms and the existence of every final credential/secret reference on the target. The diff read model reports secret presence changes as redacted sensitive fields.

Coordinated deployment is deliberately a durable plan and result ledger. Each node performs its normal node-local deployment and health gate, then reports a typed result. After a failure, unstarted members stop and only releases changed by that coordination are candidates for normal node-local rollback. Lumic does not implement consensus, leader election, overlay networking or a cluster scheduler.

A future optional Lumic Hub may provide inventory, central UI, policies/identity and notification aggregation. It must not become required to use a Lumic node.

There is intentionally no separate Lumic remote client binary. Remote humans may use the UI/API as they mature; coding agents connect to MCP endpoints. Avoid introducing another client release lifecycle unless a concrete unsolved requirement justifies it.

## 20. Containers

Container support is deliberately subordinate. Lumic may manage a containerized application, inspect common container problems, or run in a constrained container mode. Host Lumic remains canonical. Do not create top-level architecture where every service/application is assumed to be Docker/Compose.

## 21. Releases and nightly

Nightly is a branded early-access channel. A nightly artifact is published only after quality gates and supported-image install smoke tests pass. Stable is intentionally conservative. Update/channel changes are explicit and auditable.

## 22. Server attention and personality

`AttentionService` is the canonical application service for the conversational server summary. It folds live host diagnostics, managed application/service state, latest backup outcomes and bounded recent events into a structured `AttentionSummary` containing severity, facts, changes, active incidents, upcoming attention and recommendations. Only current evidence contributes to current severity; a historical failure remains a change and cannot invent an active incident.

The six node personalities (`professional`, `dry`, `grumpy`, `paranoid`, `cheerful` and `idiot`) are deterministic renderers over that summary. They may change introductions and event phrasing, but cannot alter severity, omit facts/evidence/actions, or mutate structured MCP fields. `lumic how-are-you`, the UI overview, MCP `server_attention` and `lumic://server/attention` all call the same service.

Personality selection is a node mutation: state is validated, written atomically as private JSON, audited on every request and emits an event only when it changes. The initial evidence set deliberately reuses established host, application, service, backup and event mechanisms. Certificate-expiry collection and configurable backup-age thresholds are deferred provider breadth rather than guessed data.
