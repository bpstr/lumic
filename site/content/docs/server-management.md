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

Lumic exposes live distro/version, architecture, hostname, kernel, logical CPU count, memory/swap and root-filesystem capacity. `lumic diagnose` adds load, uptime, memory pressure, high-RSS processes and failed systemd units with evidence and recovery suggestions. A complete mount and network inventory remains future work.

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

Package identifiers are validated as Debian names and the initial policy uses exact built-in entries. Unknown search results do not become trusted. Integrations may add reviewed exact packages to their policy instance. Mutations capture bounded native-tool output and emit local events and audit records. MCP exposes package inspection and exact-catalog installation only when the local server enables mutations and the caller supplies explicit approval.

## systemd lifecycle

Lumic wraps systemd with validated unit names and direct `systemctl` argument vectors:

```text
service.inspect
service.start / stop / restart / reload
service.enable / disable
```

The same service adapter backs CLI and MCP. Application workers and schedules generate recoverable units under `/etc/systemd/system`, reload the manager, and enable/start the resulting service or timer. Lumic does not expose a generic shell capability.

## Integration levels

Lumic distinguishes:

- **Package** — a whitelisted native package such as ffmpeg or rsync.
- **Component** — runtime/service attachment such as a PHP extension or PostgreSQL extension.
- **Managed service** — configuration, lifecycle, health, logs and events.
- **Application recipe** — reusable application installation/composition.
- **Role** — a node purpose such as app, database, cache, Git, worker or media.

## Diagnostics

`lumic diagnose` returns the currently implemented evidence-backed host snapshot. Deployment history, application health, events and audit records are separate typed reads, so clients can combine them without Lumic pretending to diagnose subsystems it does not yet inspect.
