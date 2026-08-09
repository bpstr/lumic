+++
title = "Development & documentation"
description = "How contributors and coding agents keep Lumic extensible and the docs permanently trustworthy."
weight = 130
[extra]
kicker = "DEVELOPMENT"
status = "active"
+++

Lumic is intentionally structured so the first core can grow without repeated rewrites. New runtimes, services, packages, recipes and interfaces should extend typed contracts rather than introduce parallel execution systems.

## Documentation is part of done

Public documentation lives under `site/content/docs/`. Internal architecture, specifications and coding-agent prompts live under `docs/`.

A change is not complete when it changes public behavior without updating the relevant public page in the same commit/PR.

For a new capability document, where relevant:

- user intent and use case;
- CLI contract;
- MCP contract;
- UI behavior;
- plan/apply behavior;
- permissions/policy;
- supported operating systems;
- events;
- failure and recovery behavior;
- test coverage;
- implementation status.

When implementation differs from planned documentation, update the documentation rather than leaving an aspirational contract that no longer matches the product.

## Local docs build

Install Zola and run:

```bash
cd site
zola serve
```

The website is an isolated static project. It is not a Cargo workspace member and adds no runtime dependency to the Lumic binary.
