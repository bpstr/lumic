+++
title = "Lumic Control Center documentation"
description = "Install Lumic Control Center, connect MCP, open the UI, then let Lumic manage the server."
sort_by = "weight"
template = "section.html"
page_template = "page.html"
+++

**Lumic Control Center** (Lumic) is a host-native Linux server operating layer available at [lumic.cc](https://lumic.cc). Its core workflow has three steps.

## 1. Install Lumic

```bash
ssh root@server 'curl -fsSL https://lumic.cc/install.sh | sh'
```

A fresh VPS becomes an autonomous Lumic node with the daemon, CLI, management surface and structured host model. The installer is published directly at `https://lumic.cc/install.sh` from the repository's canonical root `install.sh`.

## 2. Connect MCP

```bash
lumic mcp setup
```

Lumic exposes that individual node to Codex, Claude and other MCP clients through structured operations rather than making unrestricted SSH the normal agent interface. Multiple VPS nodes remain independent; no Lumic relay or central cloud account is required.

> **Pre-alpha:** the MCP setup command and remote transport are part of the v2 public contract and are being implemented during the initial roadmap.

## 3. Open the UI

```bash
lumic ui
```

Use the clean Rust management UI when you want visibility or direct control. CLI, UI and MCP must operate over the same capability model.

> **Pre-alpha:** the management UI follows the core/MCP foundation and is not yet complete.

Those three actions define Lumic. The growing collection of server capabilities is deliberately secondary: packages, runtimes, PHP extensions, Git hosting, databases, Redis, TLS, deployments, workers, events, diagnostics, notifications, webhooks and multi-node infrastructure should increasingly feel like things Lumic simply already knows how to do.

## Give Codex a complete VPS task

After MCP is connected, the ideal Lumic workflow looks like this:

> Inspect this Lumic node and the current repository. Prepare the VPS as a production environment for this application. Detect the required runtime, system packages, extensions and backing services. Install only what is needed, configure the web server, database, cache, TLS, firewall, workers and scheduled jobs where applicable, and create a zero-downtime Git deployment from the repository. Use Lumic plans before material changes, keep the host secure, verify the deployment with health checks, and report exactly what was configured. Do not use unrestricted shell access when a Lumic capability exists.

The coding agent performs the reasoning. Lumic provides trustworthy host status, safe operations, plans, policy and auditability.

## Common outcomes without infrastructure frameworks

A Laravel project should be able to resolve into PHP, the required extensions, Composer, Nginx, PostgreSQL or MariaDB, Redis when needed, TLS, environment configuration, workers/Horizon, the scheduler, Git deployment and zero-downtime releases.

A React or Node project should be able to resolve into its actual Node/build requirements, environment variables, static or server-rendered deployment mode, Nginx routing, TLS, Git deployment, health checks and zero-downtime activation where applicable — giving a VPS a Render/Vercel-like application workflow without handing control of the infrastructure to a hosted platform.

The same model will cover Python applications, static sites, APIs, workers and custom services as their integrations become available.

> **Documentation status:** Lumic v2 is pre-alpha. These pages describe both the implemented foundation and the intended public contract. Each capability page must be updated in the same change that makes the capability real. Documentation must never silently present planned behavior as already shipped.

Lumic manages Linux directly. Docker is supported as a workload feature, not used as the product's core abstraction.
