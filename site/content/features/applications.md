+++
title = "Applications"
description = "Application types and recipes Lumic can recognize, install, or deploy."
weight = 10
[extra]
kicker = "CATALOG"
status = "current nightly"
+++

Lumic treats an application as a lifecycle boundary: source, runtime, environment, routes, services, processes, health, releases, and rollback stay connected.

| Application | Current support |
|---|---|
| WordPress | Versioned `wordpress@1.0.0` recipe with pinned download, PHP, nginx, MySQL, Redis, TLS attachment, idempotent reconcile, and uninstall planning. |
| Static sites | Git-backed release layout, nginx hosting, health-gated activation, retention, and rollback. |
| PHP applications | PHP 8.1–8.4 where available through the host package manager, approved extensions, nginx, TLS, workers, schedules, and zero-downtime release foundations. |
| Laravel | Repository and deployed-state fingerprinting, configuration evidence, dependency graphs, incident context, and the `laravel-redis@1` integration. A complete Laravel recipe remains planned. |
| Node.js applications | Runtime and application-model foundation. General production build and server handoff workflows remain planned. |

Repositories may add [`lumic.toml`](@/docs/lumic-toml.md) to record production intent with the same capability and resource vocabulary as built-in applications. See [Applications](@/docs/applications.md), [Recipes](@/docs/recipes.md), and [Deployments](@/docs/deployments.md) for the complete contracts and limitations.
