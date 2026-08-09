+++
title = "Applications"
description = "Applications own runtime, Git, environment, web routing, workers, jobs, TLS and deployments."
weight = 40
[extra]
kicker = "APPLICATIONS"
status = "planned contract"
+++

In Lumic an application is more than an nginx site. It is the lifecycle boundary for deployable software.

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

Initial runtime targets are PHP, Node.js, Python, static applications and custom processes. Containerized applications are supported as another runtime/workload type, not the default.

## Peripheral dependencies

Runtime components are first-class. For PHP this includes extensions such as `intl`, `redis`, `imagick`, `pgsql`, `bcmath` and `zip`. Lumic resolves the appropriate native packages for the detected OS instead of making callers know package names.

## Application recipes

Installatron-style recipes provide modern stack installation without hard-coding applications into core. Examples include Laravel, WordPress, Drupal, Ghost, Forgejo and other self-hosted software.

Recipes compose existing runtimes, components, services and setup actions and remain declarative wherever possible.
