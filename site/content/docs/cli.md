+++
title = "CLI reference"
description = "The human console over the same typed capabilities used by UI and MCP."
weight = 100
[extra]
kicker = "REFERENCE"
status = "foundation"
+++

The CLI should stay predictable and map to Lumic's domain model.

Target command families include:

```text
lumic server status
lumic server diagnose

lumic app list
lumic app create
lumic app deploy
lumic app rollback

lumic service list
lumic service install <service>
lumic service restart <service>

lumic runtime install <runtime>@<version>
lumic runtime extension add <runtime>@<version> <extension>

lumic db create
lumic git create
lumic events
lumic logs
```

Machine-readable output such as `--json` is expected for automation. Command implementations must remain thin adapters over application/core behavior rather than reimplementing host logic.

## Current commands

At the v2 foundation stage the CLI provides the initial status/version skeleton. This page must be updated whenever a command becomes supported or its contract changes.
