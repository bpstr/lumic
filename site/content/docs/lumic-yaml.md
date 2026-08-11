+++
title = "lumic.yaml"
description = "The versioned repository-to-server application and deployment contract."
weight = 45
[extra]
kicker = "APPLICATIONS"
status = "schema version 1 implemented"
+++

`lumic.yaml` is Lumic Control Center's repository-owned application contract. Put it at the repository root. Lumic validates it before changing server state and resolves it into the same typed application, deployment, process, schedule, health, and managed-service contracts used by CLI, UI, and MCP.

The file describes application intent, not Linux commands or secret values. Unknown fields are rejected so misspellings cannot silently change a deployment. Executable fields are argument arrays and are launched directly without a shell.

## Complete schema version 1 example

```yaml
schema_version: 1
name: billing-api

source:
  branch: main
  subdirectory: apps/api

runtime:
  node: 24
  package_manager: pnpm

build:
  - ["pnpm", "run", "build"]
output: dist

web:
  command: ["node", "dist/server.js"]
  port: 3100

workers:
  queue:
    command: ["node", "dist/worker.js"]
    instances: 2
    environment:
      QUEUE: default
    working_directory: apps/worker
    restart: always
    health:
      command: ["node", "dist/worker-health.js"]
      interval_seconds: 30
      timeout_seconds: 5

cron:
  cleanup:
    command: ["node", "dist/cleanup.js"]
    schedule: "0 2 * * *"

services:
  database:
    type: postgresql
    database: billing
    user: billing
  cache: redis

migrations:
  - ["pnpm", "prisma", "migrate", "deploy"]

deployment:
  before:
    - ["pnpm", "run", "predeploy"]
  after:
    - ["node", "dist/warm-cache.js"]
  deploy_on_push: true
  retain_releases: 7
  drain_seconds: 10

shared:
  directories: [storage, uploads]
  files: [.env]

health:
  path: /health
  port: 3100
  expect: 200
  timeout_seconds: 10
```

Static and PHP runtimes use the same contract:

```yaml
runtime:
  static: true
```

```yaml
runtime:
  php: "8.4"
  extensions: [curl, intl, mbstring, mysql, xml, zip]
public: public
```

Exactly one of `static`, `node`, or `php` is required. Node requires major `20`, `22`, or `24`; PHP requires `8.1`, `8.2`, `8.3`, or `8.4`. Manifest apply installs trusted packages and then verifies the actual executable version, loaded PHP extensions, and selected Node package manager. Deployment performs the same checks read-only and stops on drift.

## Semantics

- `name` is a lowercase DNS-style slug and must match the target application ID.
- `source.branch` selects the deployment branch. A branch change requires an explicit manifest plan and apply before deployment.
- `source.subdirectory` selects the application working directory in a monorepo.
- `output` and `public` are mutually exclusive paths below the working directory. Static and PHP entry-point validation and nginx document roots use this directory.
- `build` and `migrations` each accept one argv command in schema version 1. `deployment.before` and `deployment.after` accept multiple argv commands.
- Node `web.command` and `web.port` compile to the existing blue/green release handoff. The secondary port is the next port and `drain_seconds` is bounded to 300 seconds.
- Each worker instance becomes an owned systemd process. Workers can declare bounded environment values, a release-relative or absolute working directory, `no`, `on_failure`, or `always` restart behavior, and an argv health command supervised by a systemd timer. Each cron entry becomes an owned systemd timer after a healthy deployment.
- Schema version 1 cron accepts five fields containing a wildcard or one number. Day-of-month and day-of-week cannot both be constrained because their cron and systemd semantics differ.
- Service entries are requirements, not implicit package installation. Deployment stops until every named role is bound to the declared managed-service type; a Redis cache requirement cannot be satisfied by a differently typed cache binding. Optional `instance`, `database`, and `user` values must also match.
- `shared.directories` and `shared.files` are persistent application paths materialized below the app's shared root and symlinked into every release. Overlapping declarations, traversal, and release collisions are rejected; a first file declaration seeds its shared copy from the repository when present.
- A health check gates activation and automatic release rollback. `expect` is an exact HTTP status.
- `retain_releases` is bounded from 1 to 100. `deploy_on_push` gates the native Git `post-receive` deployment path after an operator connects a hosted repository to the application with `lumic git trigger <repository> <application>`.

Secrets, arbitrary operating-system packages, shell strings, and unrestricted host commands are deliberately outside schema version 1. Worker environment values are non-secret configuration; application secrets remain in Lumic's encrypted environment store.

## Inspect, plan, apply, deploy

Inspect parsing and schema validity without application state:

```bash
lumic app manifest inspect --repository-root .
```

Resolve the contract against an existing application and its managed-service bindings:

```bash
lumic app manifest plan billing-api --repository-root .
```

Apply the reviewed intent, then deploy:

```bash
lumic app manifest apply billing-api --repository-root .
lumic app deploy billing-api
```

Every deployment also reads `lumic.yaml` from the exact checked-out Git commit and requires it to equal the reviewed, applied contract. Changing or removing the committed contract blocks deployment until a new manifest plan is applied; deployment never silently mutates runtime or application intent.

For a Lumic-hosted repository, configure the repository-to-application mapping once:

```bash
lumic git trigger billing-api billing-api --branch main
```

The fixed `post-receive` hook accepts Git's three-field update records through stdin, validates their size and object IDs, and deploys only the contract branch when `deploy_on_push` is enabled. A normal `git push` then enters the same locked, audited deployment state machine as CLI, UI, and MCP apply; it does not execute repository-provided shell.

MCP exposes the same separation through `application_manifest_inspect`, `application_manifest_plan`, and approved `application_manifest_apply`. `application_deploy` remains the distinct mutation that fetches and activates a release.

## Safety and recovery

`lumic.yaml` must be a non-symlink regular file no larger than 256 KiB. Repository paths must be normalized relative paths without parent traversal. Commands must be non-empty argv arrays without control characters. Service bindings and runtime identity are validated before deployment work begins.

Build and migration run in the selected source directory. Migration completes before atomic activation. After health succeeds, Lumic runs post-deploy commands and configures declared workers and schedules. A failed phase follows the normal deployment recovery path: the previous release and Node upstream are restored when available, and the rejected release is not retained as current.
