+++
title = "MCP"
description = "Structured infrastructure capabilities for Codex, Claude and other agent clients."
weight = 70
[extra]
kicker = "AGENTS"
status = "foundation"
+++

MCP is a first-class Lumic interface and the highest-leverage way to operate multiple nodes.

The target workflow is:

1. install Lumic independently on each VPS;
2. configure each Lumic MCP server in the coding agent;
3. let the agent inspect node status and capabilities;
4. describe the infrastructure outcome instead of manually debugging SSH sessions.

Example instruction:

> Create production on node-01 and staging on node-02. Inspect this repository's requirements, provision both environments, configure Git deployment and HTTPS, and verify health.

## Typed tools, not shell

MCP tools should look like:

```text
inspect_server
install_package
install_runtime
install_component
install_service
create_application
plan_deployment
deploy_application
rollback_deployment
search_logs
diagnose_server
```

Lumic must not expose unrestricted root shell execution as its normal MCP model.

## Policy

MCP access is capability-based. A production identity might be allowed to inspect, deploy and restart a service while being denied database deletion, firewall changes, OS upgrades or raw execution.

Mutating operations should carry actor/interface/correlation metadata into Lumic's audit trail.

## Current implementation

The v2 repository already contains an MCP crate boundary. Phase 0 introduces the first read-only server status capability before broader mutations are exposed.
