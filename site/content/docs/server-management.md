+++
title = "Server management"
description = "The host is Lumic's primary abstraction: OS, packages, services, users, network, storage and diagnostics."
weight = 30
[extra]
kicker = "SERVER"
status = "planned contract"
+++

A Lumic node models the real Linux machine rather than hiding it behind containers.

## Host facts

Lumic should expose structured facts including distro/version, architecture, hostname, kernel, CPU, memory, disks, networking and installed/managed capabilities.

These facts are shared by CLI, UI and MCP so an agent can inspect the machine before choosing an operation.

## Native package management

Lumic does not implement its own package manager. Debian/Ubuntu adapters invoke apt using validated executable arguments and policy.

Public operations remain semantic:

```text
package.search
package.install
package.remove
package.upgrade
```

not arbitrary `apt-get` strings.

Package policy can allow exact packages or constrained package families. The default MCP policy should be deny-by-default for dangerous operations.

## Integration levels

Lumic distinguishes:

- **Package** — a whitelisted native package such as ffmpeg or rsync.
- **Component** — runtime/service attachment such as a PHP extension or PostgreSQL extension.
- **Managed service** — configuration, lifecycle, health, logs and events.
- **Application recipe** — reusable application installation/composition.
- **Role** — a node purpose such as app, database, cache, Git, worker or media.

## Diagnostics

`lumic diagnose` is intended to return an evidence-backed snapshot of load, memory, disk, services, errors, deployments, network, OOM events and relevant service metrics. Lumic reports evidence; AI clients can reason over it.
