# Lumic v2 capability specification

This document is the implementation-level product contract. It is intentionally broader than the current code and narrower than a wishlist: new features should fit these domains instead of creating parallel abstractions.

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

Initial direction: nginx, PostgreSQL, MariaDB, Redis/Valkey. Later: Typesense, Meilisearch, MinIO, queues and Agnative.

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
2. Lumic-hosted bare repositories exposed through controlled SSH/HTTP later;
3. local mirror/cache used as deployment source for multiple nodes.

Credentials are secret references and must never be casually surfaced through status/log/MCP output.

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

Lumic should model long-running workers and scheduled jobs as application resources. Prefer systemd units/timers where appropriate rather than inventing a supervisor. Application processes expose desired count, command/working directory/environment, restart policy and health.

## 10. TLS and web routing

Lumic manages domain routing and certificate lifecycle as application capabilities. nginx configuration must be generated or transformed through typed configuration, validated (`nginx -t`) before reload, and recoverable. Certificate issue/renew/failure events are first-class.

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

## 14. Plans and approvals

Material changes should support plan/apply. A plan communicates current/desired state, changes, preconditions, risk, validation, expected service impact and rollback/recovery availability.

Approval policy is capability/risk based. Routine deploy/restart may be allowed while database deletion, firewall changes, OS upgrades or raw execution require approval.

## 15. MCP

MCP is a first-class interface, not a CLI wrapper.

Resources should expose server/app/service/deployment/events/diagnostic state. Tools expose typed capabilities. Avoid `execute_shell(command)`.

Representative tools:

```text
inspect_server
inspect_application
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

## 16. UI

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

## 17. Recipes and catalog

Recipes provide modern Installatron-style installation without bloating core.

Catalog layers:

- packages;
- components;
- services;
- runtimes;
- application recipes;
- node roles.

Prefer declarative signed/versioned metadata for simple composition. Rust adapters are justified when lifecycle/config logic is genuinely behavioral.

Candidate recipes after primitives are mature: Laravel, Symfony, WordPress, Drupal, Forgejo/Gitea, Matomo/analytics apps and other commonly self-hosted software.

## 18. Infrastructure and multiple nodes

Primary scale is ordinary VPS infrastructure, roughly one to ten nodes. Roles may include app, worker, database, cache, Git, media/storage, backup and edge. Nodes remain autonomous; Codex/Claude can initially be the orchestrator across multiple MCP endpoints.

A future optional Lumic Hub may provide inventory, central UI, policies/identity and notification aggregation. It must not become required to use a Lumic node.

## 19. Containers

Container support is deliberately subordinate. Lumic may manage a containerized application, inspect common container problems, or run in a constrained container mode. Host Lumic remains canonical. Do not create top-level architecture where every service/application is assumed to be Docker/Compose.

## 20. Releases and nightly

Nightly is a branded early-access channel. A nightly artifact is published only after quality gates and supported-image install smoke tests pass. Stable is intentionally conservative. Update/channel changes are explicit and auditable.