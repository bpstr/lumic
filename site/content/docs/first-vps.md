+++
title = "First VPS"
description = "Install a stable node, verify it, open the local UI and run the first attention check."
weight = 11
[extra]
kicker = "GUIDE"
status = "stable 1.0 x86_64 Debian/Ubuntu"
+++

Start with a fresh x86_64 Debian 12/13 or Ubuntu 22.04/24.04 VPS. Lumic needs root for installation and host mutations; status commands can run without mutation approval.

## Install and verify

The installer selects the stable channel by default:

```bash
ssh root@server 'curl -fsSL https://lumic.cc/install | sh'
ssh root@server 'lumic version && lumic status && lumic how-are-you'
```

The installer verifies published SHA-256 files, installs `lumic` and `lumicd` atomically, creates private state, and starts `lumicd` when systemd is available. Re-running the same artifact is a no-op. See [Installation](@/docs/installation.md) for channel and local-CI inputs.

## Open the operator UI

```bash
ssh root@server 'lumic ui token rotate'
ssh -L 8080:127.0.0.1:8080 root@server
```

Open `http://127.0.0.1:8080` and sign in with the one-time displayed token. The daemon binds the UI to loopback; do not expose that HTTP listener directly.

## Establish the operating loop

```bash
ssh root@server 'lumic diagnose'
ssh root@server 'lumic personality set dry && lumic how-are-you'
ssh root@server 'lumic events --limit 20'
ssh root@server 'lumic audit --limit 20'
```

Use STATUS → SUGGEST → PLAN → APPLY. `how-are-you` is status only. Package, service, application and recipe changes retain their own validation, approval, events and recovery behavior.

## Continue

- [MCP](@/docs/mcp.md) documents the implemented local stdio development connection and its remote-transport limitation.
- [Deployments](@/docs/deployments.md) walks through Git releases, health gates and rollback.
- [Infrastructure](@/docs/infrastructure.md) contains the two-node environment workflow and acceptance script.
- [Server intelligence](@/docs/server-intelligence.md) covers the Laravel + Redis and controlled-incident demonstrations.
- [Feature matrix](@/docs/feature-matrix.md) distinguishes shipped mechanism from nightly breadth.
