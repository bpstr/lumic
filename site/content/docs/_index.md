+++
title = "Lumic documentation"
description = "The operating contract for Lumic v2: server management, applications, services, deployments, MCP and infrastructure."
sort_by = "weight"
template = "section.html"
page_template = "page.html"
+++

Lumic is a host-native Linux server operating layer. Install it once on a VPS, then manage the machine through structured Lumic capabilities instead of routine SSH administration.

> **Documentation status:** Lumic v2 is currently pre-alpha. These pages describe both the implemented foundation and the intended public contract. Each capability page must be updated in the same change that makes the capability real.

The product has three equal interfaces over the same behavior: **CLI**, **UI**, and **MCP**. Docker is supported as a workload feature, not used as Lumic's core abstraction.
