+++
title = "Introduction"
description = "What Lumic is, what it replaces, and the rules that keep it simple."
weight = 10
[extra]
kicker = "START"
status = "foundation"
+++

Lumic is a self-hosted server, application and infrastructure manager for Linux written in Rust.

Its defining workflow is simple:

```bash
ssh root@server 'curl -fsSL https://lumic.cc/install | sh'
```

After installation, the server becomes a Lumic node. Humans can operate it through the CLI and UI; coding agents can operate it through MCP.

## The model

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

Lumic combines useful parts of traditional hosting panels, OS managers, deployment platforms, infrastructure tools and application installers, but keeps Linux visible underneath.

## Product rules

1. **Host-native first.** Lumic manages the Linux host directly.
2. **Docker is a feature.** Container workloads are supported without driving unrelated architecture.
3. **Use Linux instead of replacing Linux.** Lumic wraps apt, systemd, Git, nginx and other trusted mechanisms with typed operations, policy and audit.
4. **No shell-shaped API.** The public model is `install package`, `restart service`, `deploy application`, not arbitrary root shell strings.
5. **One core, three interfaces.** CLI, UI and MCP use the same application behavior.
6. **Autonomous nodes.** A central Lumic cloud must never be required for a useful installation.

## Current status

The v2 repository currently contains the Rust workspace, CLI/daemon/MCP boundaries, installer foundation and multi-OS CI. Host facts, systemd installation, real MCP operations and managed capabilities are being implemented in Phase 0.
