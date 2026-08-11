+++
title = "Deployments"
description = "Health-gated Git releases, activation, retention, and rollback."
weight = 60
[extra]
kicker = "APPLICATIONS"
status = "production workflow implemented"
+++

Zero-downtime deployment is a core Lumic capability, not an optional plugin.

## Release layout

A conventional application may use:

```text
/var/lib/lumic/apps/myapp/
├── releases/
├── shared/
└── current -> releases/<release>
```

The implemented lifecycle is:

```text
lock → fetch → checkout → pre-deploy → build → migrate → validate
     → start inactive Node release (when configured) → atomic activation
     → health → post-deploy → drain old Node release → retain or rollback
```

Static, generic PHP and Node applications use the same immutable release state machine. A configured Node handoff starts a release-scoped systemd unit on the inactive loopback port, checks it directly, switches the owned nginx upstream, validates the public health gate, waits the configured drain interval, and stops the old unit. Retained Node units can be restarted by an explicit rollback.

## Plan before apply

`lumic app plan <app>` resolves the repository branch, active release, health gate, risks, preconditions, validation and recovery steps without changing the host. `lumic app deploy <app>` is the separate mutation boundary.

When the repository contains [`lumic.yaml`](@/docs/lumic-yaml.md), inspect and review its repository-to-server contract first:

```bash
lumic app manifest inspect --repository-root .
lumic app manifest plan myapp --repository-root .
lumic app manifest apply myapp --repository-root .
lumic app deploy myapp
```

The exact checked-out commit is authoritative during deployment. Lumic validates its manifest, resolves the source/public path, replaces the typed deployment workflow for that release, blocks unresolved managed-service requirements, and configures declared workers and schedules only after health succeeds. A pushed commit that removes an already applied manifest is rejected. For Lumic-hosted Git, `lumic git trigger <repository> <application>` installs the fixed post-receive mapping once; the manifest branch and `deploy_on_push` flag then gate automatic deployment through the same locked state machine.

Only one deployment or rollback may hold an application's cross-process deployment lock. A second apply fails without changing a release. Configure explicit phases with JSON argv arrays so no shell parsing is involved:

```bash
lumic app configure-deployment myapp \
  --pre-deploy-command '["php","artisan","down","--retry=30"]' \
  --build-command '["npm","run","build"]' \
  --migrate-command '["php","artisan","migrate","--force"]' \
  --post-deploy-command '["php","artisan","up"]'
```

For Node blue/green handoff, add `--node-command '["node","server.js"]' --primary-port 3100 --secondary-port 3101 --drain-seconds 10`. The process must listen on the `PORT` environment variable and the application must have a Lumic-owned nginx web host.

Node handoff follows a generic release lifecycle: start the candidate release, wait for direct readiness, atomically switch nginx traffic, pass the public health gate, drain the previous release, then terminate it. Application environment references are resolved only during deployment; candidate and worker units read a root-owned `/run` environment file, and phase output is secret-redacted before logs become inspectable.

## Git sources and hosting

Lumic accepts a generic Git URL and branch. SSH keys are imported into Lumic's private state and attached by opaque credential reference; key material is never copied into application metadata, events, audit output or MCP responses. GitHub/GitLab-specific deploy keys, webhooks and multi-node mirrors are deferred ecosystem integrations.

Hosted repositories are first-class infrastructure rather than a hidden implementation detail.

## Health and recovery

`lumic app deploy` maintains a local bare mirror, resolves the configured branch to an exact commit, records author/email/subject/authored time, and checks it out into a new release directory. Explicit pre-deploy, build and migration commands run in the manifest's source directory as validated argument vectors. Migration completes before activation; a migration failure therefore leaves the active release and process set unchanged.

After activation, an enabled health check sends a local HTTP request with the application's `Host` header. Post-deploy commands run only after that gate passes. A health or post-deploy failure marks the deployment `failed_rolled_back`, restores the previous release and Node upstream, and stops the rejected Node process. `lumic app rollback` provides the same recovery explicitly. Five releases are retained by default, and deployment history records every phase and whether rollback was automatic.

Database rollback is intentionally not inferred: release rollback cannot safely reverse an arbitrary schema migration. The plan reports migration as a high risk. Use backward-compatible expand/contract migrations and keep a database recovery procedure.

`lumic app cancel <app> <deployment>` requests cooperative cancellation. Lumic lets the current direct child process finish, stops at the next phase boundary, and restores the prior release if activation already occurred. `lumic app redeploy <app> <deployment>` creates a new deployment pinned to the recorded commit even if the branch has advanced.

Deployment stdout, stderr and system messages are persisted with monotonically increasing cursors. Read once with `lumic app logs <app> <deployment> --after <cursor>`, or stream until completion with `--follow`. MCP exposes the same cursor contract through `application_deployment_logs`.

Without an explicit build command, generic PHP installs dependencies with Composer when `composer.json` exists, using `--no-plugins --no-scripts`; Node runs `npm ci --omit=dev --ignore-scripts` when a supported lockfile exists. Shared-path declarations remain future work.

The operator UI shows deployment history, source commit, per-phase results and final status. Deploy and rollback are session-authenticated, CSRF-protected confirmation actions that call this same deployment service; they do not bypass plans, health gates, events or audit behavior.
