+++
title = "Server management"
description = "The host is Lumic's primary abstraction: OS, packages, services, users, network, storage and diagnostics."
weight = 30
[extra]
kicker = "SERVER"
status = "foundation implemented; broader management planned"
+++

A Lumic node models the real Linux machine rather than hiding it behind containers.

## Host facts

Lumic currently exposes live distro/version, architecture, hostname, kernel, logical CPU count, memory/swap and root-filesystem capacity. A complete mount inventory, load, networking, processes, packages and services follow in Phase 1.

These facts are shared by CLI, UI and MCP so an agent can inspect the machine before choosing an operation.

Detection supports Debian 12/13 and Ubuntu 22.04/24.04 on x86_64 in the Phase 0 CI matrix. Distribution parsing and resource conversion also use host-independent fixtures, including an aarch64 parsing fixture; aarch64 is not a supported release target until an artifact is built and tested in CI.

## Native package management

Lumic does not implement its own package manager. Debian/Ubuntu adapters invoke apt using validated executable arguments and policy.

The nightly CLI implements these semantic operations:

```text
package.search
package.install
package.remove
package.update_index
```

not arbitrary `apt-get` strings.

Package identifiers are validated as Debian names and the initial policy uses exact built-in entries. Unknown search results do not become trusted. Integrations may add reviewed exact packages to their policy instance. Mutations capture bounded native-tool output and emit local events; MCP package mutations remain disabled.

## Integration levels

Lumic distinguishes:

- **Package** — a whitelisted native package such as ffmpeg or rsync.
- **Component** — runtime/service attachment such as a PHP extension or PostgreSQL extension.
- **Managed service** — configuration, lifecycle, health, logs and events.
- **Application recipe** — reusable application installation/composition.
- **Role** — a node purpose such as app, database, cache, Git, worker or media.

## Diagnostics

`lumic diagnose` is intended to return an evidence-backed snapshot of load, memory, disk, services, errors, deployments, network, OOM events and relevant service metrics. Lumic reports evidence; AI clients can reason over it.
