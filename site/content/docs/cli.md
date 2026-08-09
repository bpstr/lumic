+++
title = "CLI reference"
description = "The human console over the same typed capabilities used by UI and MCP."
weight = 100
[extra]
kicker = "REFERENCE"
status = "nightly foundation"
+++

The CLI should stay predictable and map to Lumic's domain model.

Implemented nightly commands include:

```text
lumic app list
lumic app create <name> --domain <domain> --runtime static|php
lumic app repository set <app> <url> --branch main
lumic app deploy <app>
lumic app deployments <app>
lumic app rollback <app>
lumic app inspect <app>
lumic app delete <app>

lumic package search <name>
lumic package inspect <name>
lumic package install <name> [--dry-run]
lumic package remove <name> [--dry-run]
lumic package update-index
lumic package allowed

lumic events
```

Command implementations remain thin adapters over the shared host-status service rather than reimplementing host logic.

The host commands remain:

```text
lumic version
lumic status
lumic status --json
```

`lumic version` prints only the deterministic Cargo package version (`lumic <version>`). `lumic status` reads live Linux host facts and renders node identity, Debian/Ubuntu release, kernel, architecture, logical CPU count, memory and root-disk capacity. `--json` serializes the full typed fact model, including swap and root filesystem metadata, for automation.

Package search is discovery only and never grants trust. Installation and removal require exact built-in policy entries, use direct argv invocation, are idempotent, and record events. `LUMIC_STATE_DIR` and `LUMIC_APPS_ROOT` can relocate state for testing; production defaults are `/var/lib/lumic` and `/var/lib/lumic/apps`.

Static Git deployments and generic PHP entry-point/Composer deployment are experimental nightly capabilities. Nginx, PHP-FPM lifecycle, TLS, HTTP health checks, webhooks, runtime commands, and self-update commands are not implemented yet; these application commands do not claim the site is externally reachable.
