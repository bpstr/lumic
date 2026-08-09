# Lumic

<p align="center"><img src="assets/lumic.svg" alt="Lumic" width="260"></p>

**From empty VPS to managed infrastructure in one command.**

Lumic is a host-native server, application, and infrastructure management system for Linux, built in Rust. It provisions machines, manages runtimes and services, hosts repositories, performs zero-downtime deployments, observes the host, emits events and notifications, and exposes the same capabilities through CLI, UI, API, and MCP.

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

## One command

```bash
ssh root@server 'curl -fsSL https://lumic.cc/install | sh'
```

The target experience is simple: install Lumic once, then stop treating SSH, `apt`, `systemctl`, nginx files, certificates, deployment scripts, and service debugging as the normal interface to a VPS.

## Product pillars

- **Server** — OS, resources, packages, users, networking, firewall, storage, processes, updates, logs and security.
- **Applications** — Git source, runtime, environment, domains, SSL, workers, jobs, storage, health and zero-downtime deployments.
- **Services** — PostgreSQL, MariaDB, Redis/Valkey, search, storage, queues, Agnative and other managed services.
- **Infrastructure** — multiple autonomous Lumic nodes, roles, environments and conventional multi-server topologies without becoming Kubernetes.
- **Automation** — events, webhooks, notifications, deterministic remediation and audit history.
- **Interfaces** — clean Rust UI, CLI and MCP over the same capability model.

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

The future Rust UI and HTTP API should call the same application/core services; they must not become a second implementation.

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

Lumic v2 is intentionally early. The architecture and CI are established before broad feature implementation so nightly development can add capabilities without repeatedly reshaping the foundation.
