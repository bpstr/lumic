+++
title = "Introduction"
description = "What Lumic Control Center is, what it replaces, and the rules that keep it simple."
weight = 10
[extra]
kicker = "START"
status = "foundation"
+++

**Lumic Control Center** (short name: **Lumic**) is a self-hosted server, application and infrastructure manager for Linux written in Rust. Its public home is [lumic.cc](https://lumic.cc).

Its defining workflow is simple:

```bash
ssh root@server 'curl -fsSL https://lumic.cc/install.sh | sh'
```

After installation, the server becomes an autonomous Lumic node. Humans can operate it through the CLI and UI; coding agents can operate it through MCP. No Lumic relay service or central account is required for the normal architecture.

## The model

```text
 Lumic Control Center
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

The repository contains the Rust workspace, CLI/daemon/MCP boundaries, installer, public documentation deployment and multi-OS CI. Lumic 1.x is the supported Rust implementation for real VPS installations.
