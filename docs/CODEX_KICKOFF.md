# Codex kickoff

Use this as the first implementation session after cloning Lumic v2.

## Prompt

You are implementing Lumic v2 in `bpstr/lumic`. Read `README.md`, `AGENTS.md`, `docs/PRODUCT.md`, `docs/ARCHITECTURE.md`, `docs/ROADMAP.md`, `docs/SECURITY.md`, and the current CI before changing code. The `v1` branch is historical only; do not port Laravel architecture.

Goal: complete Phase 0 as a coherent, tested vertical foundation without pre-building later phases.

Implement in this order:

1. Define stable core types for `HostFacts`, OS/distribution/architecture, `Capability`, operation context/result, plan/change/risk, and structured Lumic errors. Keep core transport/platform independent.
2. Implement Debian/Ubuntu host detection in `lumic-platform` by parsing `/etc/os-release`, architecture, hostname, CPU count, memory and disk facts through testable adapters. Tests must use fixtures where possible, not depend only on the developer host.
3. Add a process-runner abstraction designed for future privileged native operations: direct executable + argv, timeout, bounded stdout/stderr, exit metadata; no shell interpolation. It may remain minimally used in Phase 0.
4. Make `lumic status` render real host facts and add `--json`. Make `lumic version` deterministic.
5. Make `lumic-daemon` start, log node identity/status and shut down gracefully. Do not add a database unless state persistence is actually needed yet.
6. Define the first read-only MCP surface around server status using the current Rust MCP ecosystem only after checking current official crate/protocol guidance. Keep MCP adapter thin over the same host-status service used by CLI.
7. Harden `install.sh`: supported OS/arch detection, install path, permissions, version/channel input, idempotent update, local-binary CI mode, useful failure messages, and groundwork for systemd registration. Do not pretend releases exist if they do not.
8. Expand integration tests so installation + `lumic status --json` run in Ubuntu 22.04, Ubuntu 24.04, Debian 12 and Debian 13 images. Add host-runner tests for behavior that containers cannot validate.
9. Keep CI green: fmt, clippy with warnings denied, unit tests, integration image tests, release/static build and basic supply-chain checks.
10. Update docs when actual design differs from this kickoff. Do not leave speculative APIs undocumented.

Constraints:

- Docker is only used by CI as convenient OS test images; it is not the product model.
- No TypeScript/Node/Laravel in Lumic itself.
- No generic MCP shell tool.
- Do not over-engineer plugins/dynamic loading before concrete use cases exist.
- Prefer native Linux functionality behind typed safe adapters.
- Every supported platform claim needs tests.
- Finish with a summary of implemented behavior, tests, known limitations and the next 3 roadmap tasks.
