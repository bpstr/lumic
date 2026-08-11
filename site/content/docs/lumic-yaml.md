+++
title = "lumic.yaml"
description = "Record application runtime, service, and deployment intent once."
weight = 45
[extra]
kicker = "APPLICATIONS"
status = "planned contract"
+++

`lumic.yaml` is an optional repository-level application intent file for Lumic Control Center.

Put it in the root of an application repository:

```text
my-app/
├── lumic.yaml
├── package.json
└── src/
```

The file exists primarily so humans and coding agents do not have to describe or rediscover the complete production stack on every deployment.

It is intentionally **not a rigid infrastructure language**. It should describe what the application needs; Lumic decides how to provide those requirements safely on the target server.

A coding agent should be able to inspect a repository, read `lumic.yaml`, inspect the target Lumic node and then use Lumic plans and capabilities to prepare the machine.

## Design principles

- Keep the file short, readable and repository-owned.
- Describe application requirements, not low-level Linux implementation details.
- Omit obvious values when Lumic or the coding agent can infer them safely.
- Prefer named services and references over hard-coded localhost URLs and generated passwords.
- Keep secrets out of Git.
- Native Linux services are the default. Containers are an explicit workload choice, not the default architecture.
- Application commands run as the application's unprivileged user, never as root unless a specific trusted Lumic capability requires privilege.
- The file is a hint and desired-intent document. Lumic may inspect the repository and server before producing the final plan.

## Minimal example

A small Node application can be as short as:

```yaml
name: example-api

runtime:
  node: 24

services:
  database: postgres
  cache: redis

web:
  command: node dist/server.js
  port: 3000

build:
  - npm ci
  - npm run build

health:
  path: /health
```

This is enough context for an agent to understand that the VPS probably needs Node.js, PostgreSQL, Redis, an application process, a web proxy, environment wiring and a health check.

Lumic should still inspect the repository before applying changes.

## Reference

The sections below are the recommended vocabulary. They are deliberately small and may grow as real application deployments reveal useful patterns.

### `name`

Human-readable application identifier.

```yaml
name: billing-api
```

Prefer a stable, filesystem-safe name.

### `source`

Optional Git source hints.

```yaml
source:
  branch: main
```

The repository URL normally does not need to be repeated when Lumic is operating from an already checked-out repository.

Possible fields:

- `branch` — deployment branch.
- `subdirectory` — application root inside a monorepo.

### `runtime`

Primary application runtime.

Node example:

```yaml
runtime:
  node: 24
  package_manager: pnpm
```

PHP example:

```yaml
runtime:
  php: "8.4"
  extensions:
    - bcmath
    - intl
    - mbstring
    - pdo_pgsql
    - redis
    - zip
```

Python example:

```yaml
runtime:
  python: "3.13"
```

The runtime section describes what the application expects. Lumic resolves the appropriate host-native installation for the target operating system.

### `tools`

Development or build tools required by the application in addition to its main runtime.

```yaml
tools:
  composer: true
  node: 24
```

Typical uses include Composer for PHP applications or Node.js for frontend asset builds in Laravel applications.

### `packages`

Extra trusted operating-system packages required by the application.

```yaml
packages:
  - imagemagick
  - ffmpeg
```

Package installation must go through Lumic's package policy and allowlist. `lumic.yaml` is not an escape hatch for arbitrary root shell execution.

Do not list ordinary dependencies that belong in `package.json`, `composer.json`, `requirements.txt` or equivalent application manifests.

### `services`

Backing services the application requires.

Simple form:

```yaml
services:
  database: postgres
  cache: redis
  search: typesense
```

Expanded form:

```yaml
services:
  database:
    type: postgres
    database: app
    user: app
    storage: 20GB
    backups:
      schedule: "0 3 * * *"
      retain: 7

  cache:
    type: redis
    persistence: false

  search:
    type: typesense
    storage: 10GB
```

Service keys such as `database`, `cache` and `search` are local names chosen by the application. `type` identifies the actual service Lumic should provide.

Likely initial service types include:

- `postgres`
- `mysql`
- `mariadb`
- `redis`
- `valkey`
- `typesense`
- `meilisearch`
- `minio`

The supported catalog should expand without requiring every application manifest to become more complicated.

### Service references

Environment configuration should prefer references to Lumic-managed services instead of hard-coded connection strings.

```yaml
env:
  DATABASE_URL:
    from: service.database.url

  REDIS_URL:
    from: service.cache.url
```

A service may expose values such as:

```text
service.database.host
service.database.port
service.database.database
service.database.user
service.database.password
service.database.url
```

The exact value is resolved by Lumic for the target machine.

This allows the same application description to work when a service is local, moved to another Lumic node, or replaced by a managed service later.

### `env`

Non-secret environment values and references.

```yaml
env:
  NODE_ENV: production
  LOG_LEVEL: info

  DATABASE_URL:
    from: service.database.url
```

Do not commit passwords, API tokens or private keys here.

### `secrets`

Names of secrets required by the application.

```yaml
secrets:
  OPENAI_API_KEY:
    required: true

  STRIPE_SECRET:
    required: true
```

Lumic should obtain missing secrets interactively, from the UI, CLI, MCP workflow or another configured secure source.

Generated secrets can be described without storing the value:

```yaml
secrets:
  SESSION_SECRET:
    generate: random
```

Framework-aware helpers may be supported when useful:

```yaml
secrets:
  APP_KEY:
    generate: laravel-key
```

### `build`

Commands used to prepare an application release.

```yaml
build:
  - pnpm install --frozen-lockfile
  - pnpm build
```

Laravel example:

```yaml
build:
  - composer install --no-dev --prefer-dist --optimize-autoloader
  - npm ci
  - npm run build
```

Build commands run inside the application release directory as the unprivileged application user.

### `web`

The main HTTP workload.

Node example:

```yaml
web:
  command: node dist/server.js
  port: 3000
  instances: 2
```

PHP example:

```yaml
web:
  type: php-fpm
  root: public
  index: index.php
```

Lumic can use this information to create the appropriate process, reverse proxy or PHP-FPM configuration.

### `processes`

Long-running non-web application processes such as queue consumers.

```yaml
processes:
  worker:
    command: node dist/worker.js
    instances: 2

  emails:
    command: node dist/email-worker.js
    instances: 1
```

Laravel example:

```yaml
processes:
  queue:
    command: php artisan queue:work --sleep=1 --tries=3
    instances: 2
```

Lumic normally maps these to supervised host-native services such as systemd units.

### `jobs`

Scheduled application commands.

```yaml
jobs:
  cleanup:
    command: node dist/jobs/cleanup.js
    schedule: "0 2 * * *"
```

Laravel scheduler:

```yaml
jobs:
  scheduler:
    command: php artisan schedule:run
    schedule: "* * * * *"
```

Lumic may implement these using systemd timers or another trusted host-native scheduler.

### `domains`

Domains that should route to the application.

```yaml
domains:
  - example.com
  - www.example.com
```

Expanded form:

```yaml
domains:
  - domain: example.com
    tls: auto

  - domain: www.example.com
    redirect: https://example.com
```

Lumic should configure the web server, certificate management and safe HTTP-to-HTTPS behavior from this intent.

### `deploy`

Application-specific deployment actions.

```yaml
deploy:
  before:
    - pnpm prisma migrate deploy

  after:
    - pnpm cache:warm
```

Laravel example:

```yaml
deploy:
  before:
    - php artisan down

  migrate:
    - php artisan migrate --force

  after:
    - php artisan config:cache
    - php artisan route:cache
    - php artisan view:cache
    - php artisan up
```

These commands describe application lifecycle hooks. They are not privileged server provisioning commands.

### `health`

Health-check hints used after deployment and during normal operation.

```yaml
health:
  path: /health
  expect: 200
```

Expanded form:

```yaml
health:
  path: /health
  interval: 30s
  timeout: 5s
  expect: 200
```

### `storage`

Application-owned writable or persistent paths.

```yaml
storage:
  writable:
    - storage
    - bootstrap/cache
```

Future forms may also describe persistent/shared directories that survive release switching.

### `container`

Use a container only when the application explicitly needs one.

```yaml
container:
  image: ghcr.io/example/app:latest
```

Container support is a workload feature. Lumic remains host-native by default.

## TypeScript application example

```yaml
name: example-saas

source:
  branch: main

runtime:
  node: 24
  package_manager: pnpm

packages:
  - imagemagick

services:
  database:
    type: postgres
    database: app
    user: app
    storage: 20GB
    backups:
      schedule: "0 3 * * *"
      retain: 7

  cache:
    type: redis

  search:
    type: typesense
    storage: 10GB

  storage:
    type: minio
    storage: 50GB

env:
  NODE_ENV: production

  DATABASE_URL:
    from: service.database.url

  REDIS_URL:
    from: service.cache.url

  TYPESENSE_HOST:
    from: service.search.host

  TYPESENSE_API_KEY:
    from: service.search.api_key

secrets:
  SESSION_SECRET:
    generate: random

  OPENAI_API_KEY:
    required: true

build:
  - pnpm install --frozen-lockfile
  - pnpm build

web:
  command: node dist/server.js
  port: 3000
  instances: 2

processes:
  worker:
    command: node dist/worker.js
    instances: 1

jobs:
  cleanup:
    command: node dist/jobs/cleanup.js
    schedule: "0 2 * * *"

domains:
  - domain: example.com
    tls: auto

  - domain: www.example.com
    redirect: https://example.com

deploy:
  before:
    - pnpm prisma migrate deploy

health:
  path: /health
  expect: 200
```

## Laravel application example

```yaml
name: example-laravel

source:
  branch: main

runtime:
  php: "8.4"
  extensions:
    - bcmath
    - curl
    - intl
    - mbstring
    - pdo_pgsql
    - redis
    - zip

tools:
  composer: true
  node: 24

packages:
  - imagemagick

services:
  database:
    type: postgres
    database: app
    user: app
    storage: 20GB
    backups:
      schedule: "0 3 * * *"
      retain: 14

  cache:
    type: redis
    persistence: true

env:
  APP_ENV: production
  APP_DEBUG: "false"

  DATABASE_URL:
    from: service.database.url

  REDIS_URL:
    from: service.cache.url

secrets:
  APP_KEY:
    generate: laravel-key

  STRIPE_SECRET:
    required: true

build:
  - composer install --no-dev --prefer-dist --optimize-autoloader
  - npm ci
  - npm run build

web:
  type: php-fpm
  root: public
  index: index.php

processes:
  queue:
    command: php artisan queue:work --sleep=1 --tries=3
    instances: 2

jobs:
  scheduler:
    command: php artisan schedule:run
    schedule: "* * * * *"

domains:
  - domain: example.com
    tls: auto

deploy:
  migrate:
    - php artisan migrate --force

  after:
    - php artisan config:cache
    - php artisan route:cache
    - php artisan view:cache

health:
  path: /up
  expect: 200

storage:
  writable:
    - storage
    - bootstrap/cache
```

## How coding agents should use the file

A coding agent should treat `lumic.yaml` as strong repository context, not blindly execute it.

Recommended workflow:

1. Read `lumic.yaml`.
2. Inspect normal application manifests such as `package.json`, `composer.json`, lock files and framework configuration.
3. Inspect the target Lumic node and currently installed services.
4. Resolve omissions or obvious mismatches from repository evidence.
5. Ask for missing secrets only when required.
6. Produce a Lumic plan before material host changes.
7. Apply changes through typed Lumic capabilities instead of unrestricted root shell commands.
8. Deploy the application.
9. Run health checks and report the resulting runtime, services, domains and process state.

For most repositories, the useful instruction should become:

> Read `lumic.yaml`, inspect this repository and prepare this Lumic node for production. Fill in safe obvious details from the repository, show the infrastructure plan before material changes, deploy the application and verify its health.

The point of `lumic.yaml` is that the user should not have to explain "Node 24 + PostgreSQL + Redis + worker + scheduler + nginx + TLS" again every time an agent touches the server.

## Status

`lumic.yaml` is currently a planned application contract. The documentation is intentionally ahead of the implementation so real deployments have a clear starting point.

The format should stay pragmatic. When implementation details change, preserve the simple principle: **the repository describes what the application needs; Lumic decides how the server provides it.**
