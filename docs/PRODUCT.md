# Lumic v2 product brief

## Definition

Lumic is a self-hosted, host-native server, application and infrastructure management system for Linux. It combines the useful outcomes of a control panel, OS manager, infrastructure tool, deployment service and Installatron-style application installer in a clean Rust product designed for both humans and coding agents.

The core promise is: **from empty VPS to managed infrastructure in one command.**

## Primary user journey

1. Buy a blank Linux VPS.
2. Run `ssh root@server 'curl -fsSL https://lumic.cc/install | sh'`.
3. Lumic detects OS/architecture/resources and installs itself as a long-running node service.
4. Open the minimal UI, use the CLI, or register the node as an MCP server in Codex/Claude.
5. Describe desired applications/environments/services.
6. Lumic exposes structured status and safe operations; the AI decides what the target needs and executes plans through Lumic.
7. Routine SSH administration becomes exceptional, not normal.

With two VPS nodes, an operator should be able to connect both to Codex and request production + staging environments, application requirements, Git deployment and HTTPS without manually debugging install scripts over SSH.

## Product surfaces

### Server
OS detection, resources, updates, package inventory, users, SSH policy, firewall, networking, storage, processes, systemd services, logs, events, security and diagnostics.

### Applications
Runtime, Git repository/source, local hosted repositories, environment variables, domains, nginx/web routing, TLS, persistent storage, workers, scheduled jobs, databases, deployments, health checks, logs and rollback.

### Services
Managed PostgreSQL/MariaDB, Redis/Valkey, search services, object storage, queues, Agnative and future infrastructure services. Lumic owns install/config/lifecycle/health/logs/upgrade/backup hooks/events for managed services.

### Infrastructure
Multiple autonomous Lumic nodes, roles and environments. Focus first on conventional 1–10 node infrastructure: app, worker, database, cache, Git, media/storage, backup and edge roles. Do not become Kubernetes-lite.

### Automation
Events, webhook delivery, notifications, deterministic remediation, audit history and future policy-driven workflows.

### Application catalog
Modern Installatron-style recipes for frameworks/apps/services. Recipes should compose runtimes, components, packages, services and setup tasks without hard-coding each product into core.

## Deployment

Zero-downtime deployment is a core feature. Use release directories, shared resources, atomic activation, runtime-specific graceful reload/switch/drain strategies, health validation and rollback. PHP and long-running runtimes may require different activation adapters.

Git is first-class: remote source deployment, local bare repository hosting and optional mirrors/caches.

## Native system philosophy

Lumic does not reimplement apt, systemd, Git, nginx or other reliable Linux tools. It understands them and wraps them in typed, validated, policy-controlled operations.

A whitelisted package can often be added as metadata rather than custom Rust logic. Example: FFmpeg may simply map to a trusted native package plus detection/version capabilities. Components and managed services add richer contracts only when needed.

## Containers

Docker/container support exists where it solves real workload problems, including container applications and optional Lumic-in-container scenarios. It must not drive product navigation, architecture or terminology. Host Lumic remains the canonical deployment because full server diagnosis requires host context.

## Observability and diagnosis

Capture host resources, processes, services, OOM/kernel events, deployment events, web/service errors and domain-specific health. `lumic diagnose` should produce structured evidence and correlations that an AI can reason over. Avoid pretending deterministic heuristics are AI conclusions.

## Events and notifications

Examples: service failed/recovered, deployment started/succeeded/failed/rolled back, certificate renewal, disk/memory thresholds, backup result, repository update and security update availability. Generic webhooks are the base notification integration; other destinations can layer on later.

## UI

Rust UI. Black/white/gray, exceptional typography, restrained state indicators and progressive disclosure. Avoid cPanel-style icon grids and dashboard clutter. The UI is an operational console and visibility surface, not the architecture.

## Release channels

- **nightly**: early-access product channel built every night from main; rapid fixes/capability expansion.
- **stable**: conservative production releases.

Nightly is a Lumic brand feature: “Lumic gets better every night.” It must still be tested, versioned and recoverable.
