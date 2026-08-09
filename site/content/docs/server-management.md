+++
title = "Server management"
description = "The host is Lumic's primary abstraction: OS, packages, services, users, network, storage and diagnostics."
weight = 30
[extra]
kicker = "SERVER"
status = "host operator implemented"
+++

A Lumic node models the real Linux machine rather than hiding it behind containers.

## Host facts

Lumic exposes live distro/version, architecture, hostname, kernel, logical CPU count, memory/swap and root-filesystem capacity. The host operator adds users/groups, firewall state, listening ports, mounts/capacity, high-RSS processes, systemd timers, pending apt updates and backup schedules. `lumic diagnose` correlates load, memory pressure, failed units, nearly-full mounts and pending security updates with evidence and bounded recovery suggestions.

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

`lumic diagnose` returns the evidence-backed host snapshot. Deployment history, application health, events and audit records remain separate typed reads.

## Host operator

`lumic server snapshot` is the broad read-only view. Focused commands inspect firewall rules, listeners, mounts, processes, timers, updates and journal entries. Typed mutations cover user/group lifecycle and membership, non-symlink path ownership/mode, UFW allow/deny rules, fixed process signals, security/all package updates and systemd-backed managed-service backup schedules.

Deterministic remediation is deliberately narrow: validated systemd service restart with verification, process termination, and bounded journal vacuum. There is no arbitrary command or arbitrary remediation script. Mutations use direct argument vectors, validate identifiers/paths/CIDRs, emit events and audits, and retain data where deletion would make recovery unsafe.

The initial firewall adapter requires UFW; backup schedules target existing Lumic managed-service backup commands. All host mutations require root privileges on Debian/Ubuntu. Inspect/status operations are safe to call first and MCP applies additionally require node policy and explicit approval.
