+++
title = "Lumic Control Center documentation"
description = "Install Lumic, connect an agent, and operate the node."
sort_by = "weight"
template = "section.html"
page_template = "page.html"
+++

**Lumic Control Center** (Lumic) is a host-native operating layer for Linux servers. Install it once, then use the same typed capabilities through CLI, UI, and MCP.

## 1. Install Lumic

```bash
ssh root@server 'curl -fsSL https://lumic.cc/install.sh | sh'
```

A fresh VPS becomes an autonomous Lumic node. The canonical installer is `https://lumic.cc/install.sh`.

## 2. Ask the node

```bash
lumic how-are-you
```

CLI, UI, and MCP share one factual view of health, incidents, recent changes, and recommended attention. See [MCP](@/docs/mcp.md) for stdio, restricted SSH, and authenticated HTTP setup.

## 3. Open the UI

The installer starts the UI on the VPS and prints its one-time sign-in token. Run the following tunnel command on your local computer and leave it running:

```bash
ssh -N -L 8080:127.0.0.1:8080 root@server
```

Open `http://127.0.0.1:8080` on your local computer and sign in with the token. If the token was lost, run `sudo lumic ui token rotate` on the VPS (for example, through a normal SSH session) to create a replacement. The authenticated Rust UI shows applications, services, repositories, deployments, events, host status, and confirmed actions. [Read the detailed operator UI instructions](@/docs/operator-ui.md).

Browse the dedicated [Features](@/features/_index.md) catalog for supported applications, managed services, and host capabilities.

## Describe the application once

Repositories can include an optional [`lumic.yaml`](./lumic-yaml/) with runtime, service, process, domain, secret-reference, health, and deployment intent. Lumic combines that intent with repository evidence and live node state before proposing a plan.

A typical request can therefore be short:

> Read `lumic.yaml`, inspect this repository and prepare this Lumic node for production. Fill in safe obvious details from the repository, show the infrastructure plan before material changes, deploy the application and verify its health.

## Give Codex a complete VPS task

After MCP is connected, ask for the outcome and require a plan before mutation:

> Inspect this Lumic node and repository. Plan the runtime, services, web routing, TLS, workers, schedules, and Git deployment. Apply the approved plan, verify health, and report what changed.

The agent supplies the reasoning. Lumic supplies host evidence, plans, policy, safe operations, and an audit trail.

## Explore supported features

The [Features](@/features/_index.md) section lists application support, managed services, and host capabilities in compact catalogs. The [Feature matrix](@/docs/feature-matrix.md) records exact platform coverage and planned expansion.

> **Documentation status:** These pages are the public product contract. Each page distinguishes implemented, nightly, foundation-only, and planned behavior.

Lumic manages Linux directly. Docker is supported as a workload feature, not used as the product's core abstraction.
