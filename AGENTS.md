# Agent instructions

These instructions apply to Codex, Claude and other coding agents working on Lumic Control Center (Lumic).

## Product identity

- The full product name is **Lumic Control Center**.
- **Lumic** is the normal short name used in prose, commands, binaries, crate names and technical identifiers.
- The canonical public site and documentation origin is **https://lumic.cc**.
- The canonical public installer URL is **https://lumic.cc/install.sh**. The root repository `install.sh` is the single source of truth and the Pages build publishes it; do not maintain a second installer implementation under `site/`.
- `https://lumic.cc/install` may remain as a compatibility alias, but new documentation should use `/install.sh`.
- Lumic is open source and each node is autonomous. Do not introduce a mandatory Lumic relay, account, hosted control plane or Lumic-owned DNS dependency into the normal node/MCP architecture.

## Mission

Build Lumic Control Center into a host-native Linux server operating layer for developers and operators. The defining workflow is: install Lumic on a fresh VPS once; afterwards humans and coding agents manage the machine through structured Lumic capabilities instead of routine SSH administration.

## Non-negotiable product boundaries

- Rust is the product stack. Do not introduce Laravel, Node, TypeScript or another application runtime for Lumic itself without an explicit architectural decision.
- The public website/documentation uses Zola under `site/`; it is isolated from the Cargo workspace and is not a Lumic runtime dependency.
- Docker/container support is a feature, not a core abstraction and not a reason to shape unrelated APIs around containers.
- Do not duplicate Linux subsystems. Wrap trusted native mechanisms such as apt, systemd, Git and nginx with validation, policy, plans, audit and stable domain contracts.
- Do not expose unrestricted shell execution as the default CLI/API/MCP model.
- Keep UI, CLI, HTTP and MCP as adapters over the same application/core behavior.
- Prefer additive architecture. New services, runtimes, recipes and integrations should implement contracts rather than require rewrites of core orchestration.
- **Do not create a separate Lumic client binary.** The node's own CLI/UI/API/MCP surfaces are sufficient unless a concrete future requirement proves otherwise.
- **Do not create a Lumic-specific AI skills package/repository.** Agent knowledge belongs in MCP schemas/descriptions, public docs, catalogs/recipes, inspection and suggestion capabilities.

Avoid architectural work whose main effect is adding another installation step, release lifecycle or source of duplicated operational knowledge.

## Core interaction model

Prefer this conceptual flow for human and agent-facing features:

```text
STATUS  -> what exists now?
SUGGEST -> what would make sense?
PLAN    -> what exactly will change?
APPLY   -> perform the approved change
```

Keep those responsibilities separate.

- Status is evidence from the actual host/resources.
- Suggest is read-only recommendation/detection with evidence.
- Plan resolves a concrete intended mutation, risks and preconditions.
- Apply is the mutation boundary.

Suggestions inform; plans execute. Never hide mutation inside a suggestion operation.

A first-class suggestion capability should support known stacks and repository-aware analysis, e.g. `lumic suggest laravel`, `lumic suggest nextjs`, `lumic suggest --path ...`, and an MCP `suggest_application_setup` tool. Significant recommendations must include evidence. The coding agent combines suggestions with live host status and performs the higher-level reasoning; do not overbuild a bespoke recommendation engine that tries to replace the LLM.

## Safety model

Infrastructure code is destructive by nature. Every mutating capability must consider:

1. input validation and shell-injection avoidance;
2. privilege requirements;
3. allow/deny policy;
4. idempotency;
5. dry-run/plan support where meaningful;
6. preconditions;
7. before/after audit data;
8. failure behavior;
9. rollback or recovery guidance;
10. tests on supported operating systems.

Never interpolate untrusted strings into `sh -c`. Prefer direct process execution with separated arguments. Package operations must validate package identifiers and policy before invoking the OS package manager.

## Architecture rules

Dependency direction:

```text
interfaces (CLI/UI/HTTP/MCP)
            ↓
application orchestration
            ↓
       lumic-core
            ↑
platform/service adapters
```

`lumic-core` must not know clap, HTTP transports, UI frameworks, MCP transports or Linux command syntax.

`lumic-platform` owns host detection and OS mechanisms. Begin with Debian/Ubuntu but model package/service/firewall/filesystem/process operations through traits and typed results.

A capability should look semantically like `InstallPackage { package }`, not `RunCommand { command }`.

## Integration levels

When implementing or changing a Lumic integration—including a package, component, runtime, managed service, service resource, artifact, application process or schedule, binding, provider, or application definition—consult `.agents/skills/lumic-integration-authoring/SKILL.md` and its relevant references before editing code.

Keep these separate:

- **Package**: a whitelisted native package with detection/version metadata.
- **Component**: something attached/configured to a runtime or service, e.g. PHP extension or PostgreSQL extension.
- **Managed service**: lifecycle + config + health + logs + upgrades + events, e.g. PostgreSQL, Redis, nginx, Agnative.
- **Application recipe**: declarative application installation/composition, e.g. Laravel, WordPress, Forgejo.
- **Role**: composition for a server/node purpose, e.g. app, cache, Git, media, worker.

Do not turn every apt package into a managed service.

## Development requirements

### Commit messages

Write commit subjects as concise imperative sentences without conventional-commit or category prefixes. Use `Harden state persistence`, not `fix: harden state persistence`, `feat(ui): ...`, or similar prefixed forms.

### Versioning and releases

Lumic follows Semantic Versioning. Treat the version reported by every binary, release tag, installer, updater, release artifact, and public document as one coordinated contract.

- Use `MAJOR.MINOR.PATCH` stable release tags without a `v` prefix.
- Treat an intentional `MAJOR.MINOR.PATCH-PRERELEASE.N` workspace version as the forward development line; the earlier stable release remains `1.0.0` until a newer stable release passes every gate.
- Publish a prerelease tag only through an explicit prerelease channel; the stable release workflow accepts stable versions only.
- Increment `MAJOR` for incompatible public CLI, API, MCP, configuration, state-format, or operational-contract changes.
- Increment `MINOR` for backward-compatible capabilities.
- Increment `PATCH` for backward-compatible fixes and hardening.
- Push release-candidate commits without a tag and wait for every required GitHub Actions workflow on the exact candidate commit to succeed. Create the version tag only after that green commit is final.
- A fix or test commit is not a release by itself. Group related commits under one candidate version and create one tag only when the complete candidate is proven.
- Never use a version tag to test CI. If a tag was pushed before its gates completed and any gate fails, delete the local and remote tag, do not publish a GitHub release for it, retire that version, and advance the workspace version before the next candidate.
- Never reuse, move, or publish a successful release tag, or a release tag with a version that disagrees with the Cargo workspace version.
- Never decrease a released version or publish unrelated builds under the same version.
- After the untagged candidate is green, verify the release workflow, installer, and self-updater resolve the intended exact tag and artifacts before pushing the tag. A release is incomplete until the tag workflow succeeds and existing Lumic servers can discover and install it through their configured channel.
- Update public release and installation documentation in the same commit as any release-contract change.

Before finishing a change:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

If a change affects host behavior, add/update integration coverage under `tests/` and CI image matrices. Do not claim an OS is supported without automated install/detection coverage.

## Documentation is part of done

This is a standing requirement, not a later cleanup task.

Public docs live in `site/content/docs/`; internal architecture/specification material lives in `docs/`. Read `docs/DOCUMENTATION.md`.

Every change that affects public behavior MUST update the relevant documentation in the same PR/commit. This includes commands, capabilities, MCP tools, configuration, OS support, events, installation, deployment behavior and security/policy.

For every operator-visible stable or nightly change, update `site/content/changelog.md` in the same PR/commit. Follow `docs/DOCUMENTATION.md`: write compact dated outcomes, group related commits, keep newest entries first and link to the detailed capability docs.

For a new capability document, where relevant:

- user intent/use case;
- CLI shape;
- MCP shape if agent-relevant;
- UI behavior;
- plan/apply behavior;
- permissions/policy;
- OS support;
- events emitted;
- failure and recovery behavior;
- tests;
- current implementation status.

Never leave docs claiming a feature is implemented when it is only planned. If code intentionally changes the contract, update the contract immediately.

## UX direction

The UI is black/white/gray, restrained, fast and stable. It should feel like a serious appliance console, not a crowded hosting dashboard. Progressive disclosure is preferred: simple status/actions first, underlying service/config/process details available to experts.
