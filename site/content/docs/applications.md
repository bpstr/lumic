+++
title = "Applications"
description = "Applications own runtime, Git, environment, web routing, workers, jobs, TLS and deployments."
weight = 40
[extra]
kicker = "APPLICATIONS"
status = "application lifecycle and intelligence implemented"
+++

In Lumic an application is more than an nginx site. It is the lifecycle boundary for deployable software.

The nightly CLI persists application identity, domain, static/PHP/Node runtime, Git repository and branch, health configuration/state, worker/schedule definitions, typed managed-service references, nginx/TLS state, release retention, timestamps, and deployment history. It creates `releases/`, `shared/`, `repository/`, and an atomic `current` symlink under `/var/lib/lumic/apps/<name>`.

Managed PostgreSQL/Redis resources can be attached with a semantic role plus optional database/user. Application metadata keeps only the secret reference; it does not copy credential values. Portable environment bundles clone this typed application definition between nodes with an explicit target tier, domain and secret/service-reference transforms. Application intelligence now discovers deployed Laravel/dotenv evidence and safely wires the single reference Redis integration; broader framework/service combinations remain nightly catalog work.

See [Server intelligence](@/docs/server-intelligence.md) for fingerprint, plan/apply, rollback, dependency graph and incident workflows.

Exports never include secret values. Before import creates or updates an application, Lumic verifies that every final environment, service and repository credential reference exists in private state on the target node. `lumic environment diff` redacts sensitive references while still showing whether the source and target are configured differently.

Repository URLs must use HTTPS, SSH/Git scp syntax, or `file://`. HTTPS credentials embedded in URLs are rejected; metadata stores only an optional credential reference. `app credential import` copies a validated private key into the private state directory with mode `0600`; Git receives the resolved key path through a scoped process environment and status/audit output remains redacted.

An application can own:

- domain and web routing;
- runtime and runtime components;
- source repository;
- environment values and secrets;
- persistent/shared storage;
- database/service relationships;
- workers and scheduled jobs;
- TLS certificates;
- deployments, health checks and rollback history;
- application logs and events.

## Repository application intent with `lumic.yaml`

Applications may include an optional [`lumic.yaml`](./lumic-yaml/) file at the repository root. It gives Lumic and coding agents durable context about the runtime, backing services, processes, build commands, domains, health checks and deployment needs of the application.

The manifest is deliberately pragmatic rather than a rigid infrastructure DSL. Developers describe what the application needs and may omit obvious details that Lumic or a coding agent can safely infer from the repository. Lumic still inspects the application and target host and produces a plan before material changes.

The goal is simple: describe a stack such as Node + PostgreSQL + Redis + workers once instead of explaining the full production environment every time an agent prepares a VPS.

## Runtimes

The reference deployment types are static repositories with a root `index.html`, and generic PHP repositories with a root `index.php`. PHP runs production Composer install flags when `composer.json` exists. Lumic installs nginx/PHP-FPM/PHP CLI/Composer through its apt policy, discovers the versioned PHP-FPM socket, writes a validated nginx configuration, and reloads the service. The Node foundation installs Node/nginx, runs `npm ci --omit=dev` when a lockfile is present, requires `package.json`, and proxies nginx to port 3000; richer Node process configuration is deliberately deferred.

## Peripheral dependencies

Runtime components are first-class. The current deliberately small PHP catalog is `curl`, `intl`, `mbstring`, `xml`, and `zip`. Lumic resolves each to an approved native package; unknown component names are denied.

## Safe activation and recovery

Each deployment records source, checkout, build, pre-activation, activation and health phases. Activation is an atomic symlink replacement. When an enabled local HTTP health check does not return the configured successful status range, Lumic immediately restores the previous release, records `failed_rolled_back`, emits deployment events and retains the evidence in the audit trail. Manual rollback uses the same activation primitive.

nginx files use atomic sibling writes and retain a `.lumic-backup`. Lumic validates before reload and restores the previous file/link if validation or reload fails. Workers run as systemd services with restart-on-failure; schedules use persistent systemd timers.

## Application recipes

Installatron-style recipes provide modern stack installation without hard-coding applications into core. Examples include Laravel, WordPress, Drupal, Ghost, Forgejo and other self-hosted software.

Recipes compose existing runtimes, components, services and setup actions and remain declarative wherever possible.
