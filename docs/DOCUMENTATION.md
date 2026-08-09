# Documentation policy

Lumic documentation is a product contract and must evolve with implementation.

## Locations

- `site/content/docs/` — public user/operator/developer documentation published at lumic.cc.
- `docs/` — internal architecture, specification, roadmap, CI and coding-agent prompts.

## Definition of done

Any change that affects a public capability, command, configuration contract, supported platform, MCP tool, event, security behavior, installation path or operational workflow MUST update the relevant public docs in the same PR/commit.

Do not postpone documentation to a later cleanup task. If implementation intentionally diverges from an existing planned contract, change the contract and explain the new behavior.

Each capability page should clearly label whether behavior is implemented, foundation-only, experimental/nightly, or planned.

## Docs CI

The public site is built by Zola from `site/` and deployed independently to GitHub Pages. The website is intentionally outside the Cargo workspace and has no effect on the Lumic runtime/binary.
