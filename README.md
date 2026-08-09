# Lumic

<p align="center"><img src="assets/lumic.svg" alt="Lumic" width="260"></p>

**From empty VPS to managed infrastructure in one command.**

Lumic is a host-native server, application, and infrastructure management system for Linux, built in Rust. Install it on a VPS once, connect your coding agent through MCP, and use the UI whenever you want direct visibility and control.

Lumic manages Linux directly. Containers are a supported workload and deployment feature, not the foundation of the product.

```text
        Lumic
          │
   ┌──────┼──────┐
   │      │      │
  CLI     UI     MCP
   │      │      │
   └──────┼──────┘
          │
       Linux VPS
```

## The Lumic workflow

These three steps define the product. Everything else is capability.

### 1. Install Lumic

```bash
ssh root@server 'curl -fsSL https://lumic.cc/install | sh'
```

A fresh VPS becomes a Lumic node. Lumic detects the host and provides the daemon, CLI, management UI and MCP endpoint.

### 2. Connect MCP

The intended v2 flow is deliberately simple:

```bash
lumic mcp setup
```

Lumic prints the endpoint and client configuration required by Codex, Claude or another MCP client. Once connected, the coding agent can inspect the node and call structured Lumic operations instead of being given unrestricted SSH access.

> **Pre-alpha note:** the MCP setup command and remote transport are part of the v2 public contract and are being implemented during the initial roadmap.

### 3. Open the UI

```bash
lumic ui
```

Lumic prints/opens the local management URL. The UI exposes the same server, application, service, deployment, event and diagnostic model as CLI and MCP.

> **Pre-alpha note:** the Rust management UI is part of the v2 contract and follows the core/MCP foundation.

That is Lumic:

**Install when the VPS is created. Connect MCP for agent operation. Open the UI when you want to see or manage it yourself.**

## The best part: give Codex the VPS

After connecting the Lumic MCP server, a complete setup should be describable as a task rather than a shell session:

> Inspect this Lumic node and the current repository. Prepare the VPS as a production environment for this application. Detect the required runtime, system packages, extensions and backing services. Install only what is needed, configure the web server, database, cache, TLS, firewall, workers and scheduled jobs where applicable, and create a zero-downtime Git deployment from the repository. Use Lumic plans before material changes, keep the host secure, verify the deployment with health checks, and report exactly what was configured. Do not use unrestricted shell access when a Lumic capability exists.

For two servers:

> Use `node-01` for production and `node-02` for staging. Inspect this repository, create both environments from its actual requirements, configure deployments and health checks, keep their databases and secrets separate, and verify that both are ready to receive deployments.

The agent supplies reasoning. Lumic supplies trustworthy host state and safe infrastructure operations.

## Straightforward application outcomes

Lumic is intended to make common hosting workflows boring without requiring an infrastructure framework.

### Laravel / PHP

A normal Laravel setup can resolve to:

```text
PHP + required extensions
Composer
Nginx
PostgreSQL or MariaDB
Redis when required
TLS
.env / secrets
queue workers / Horizon
scheduler
Git repository
zero-downtime releases
health checks
```

The developer should be able to connect Codex to the node and say:

> Set up this Laravel repository on this Lumic node and make it production ready.

### React / frontend / Node

A frontend application can be treated similarly to a Render/Vercel-style deployment target while remaining on your VPS:

```text
Git repository
Node runtime
package/build tooling
build command
static output or application process
Nginx routing
TLS
environment variables
health checks
zero-downtime releases when applicable
```

For example:

> Deploy this React application to `app.example.com`. Inspect the repository to determine how it builds and whether it is static or server-rendered, configure the appropriate Lumic application runtime, HTTPS and automatic Git deployments, then verify the live application.

The same model extends to Python apps, static sites, workers, APIs and custom services as integrations land.

## Capabilities should feel like bonuses

Lumic is not defined by a long feature checklist. Server capabilities should increasingly produce the reaction: **“oh, Lumic supports that too.”**

Examples include packages and PHP extensions, PostgreSQL extensions, Git hosting and mirrors, databases, Redis, certificates, backups, workers, cron/jobs, zero-downtime releases, logs, load tracing, diagnostics, notifications, webhooks, server events, managed services, dedicated node roles and multi-server environments.

## Core design laws

1. **Host-native first.** Use Linux directly; do not make Docker the architecture.
2. **Use Linux rather than replacing Linux.** Prefer trusted native tools such as apt, systemd, Git and nginx behind typed Lumic capabilities.
3. **No raw shell as the normal API.** MCP/UI/CLI call structured operations such as `package.install`, `service.restart`, `application.deploy` and `server.diagnose`.
4. **Policy before privilege.** Native commands and packages are whitelisted/validated; dangerous operations are explicit and auditable.
5. **Plan before apply.** Material changes should support inspection, validation and rollback where practical.
6. **One capability model.** UI, CLI, API and MCP must not grow separate business logic.
7. **Autonomous nodes.** A Lumic VPS stays useful without a mandatory central cloud.
8. **Evidence over magic.** Diagnostics expose measurements and correlations; AI performs reasoning on top.
9. **Nightly is a product channel.** Early Lumic improves every night while stable remains conservative.
10. **Extensibility without core bloat.** Packages, components, managed services, application recipes and roles have progressively richer contracts.

## Workspace

```text
crates/
├── lumic-core       domain types, capabilities, plans, events, policy
├── lumic-platform   host detection + Linux adapters
├── lumic-cli        human command-line interface
├── lumic-daemon     long-running node service
└── lumic-mcp        agent-facing MCP surface
```

The future Rust UI and HTTP API call the same application/core services; they must not become a second implementation.

## Development

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo run -p lumic-cli -- status
```

Read in this order before substantial work:

1. [`docs/PRODUCT.md`](docs/PRODUCT.md)
2. [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
3. [`docs/ROADMAP.md`](docs/ROADMAP.md)
4. [`AGENTS.md`](AGENTS.md)
5. [`docs/CODEX_KICKOFF.md`](docs/CODEX_KICKOFF.md)
6. [`docs/CODEX_NIGHTLY.md`](docs/CODEX_NIGHTLY.md)

## Versions

- `main` — Lumic v2 development.
- `v1` — historical Laravel implementation.
- `nightly` releases — automated prerelease builds from `main` during early development.

Lumic v2 is intentionally early. The architecture, tests and documentation contract are established before broad feature implementation so nightly development can add capabilities without repeatedly reshaping the foundation.
