+++
title = "Deployments"
description = "Git-backed releases, health checks, activation and rollback with zero downtime where the runtime allows it."
weight = 60
[extra]
kicker = "APPLICATIONS"
status = "static/PHP release foundation implemented"
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

The implemented static/PHP lifecycle is:

```text
fetch → checkout → dependencies/build → pre-activate validation
      → atomic activation → local HTTP health → retain or automatic rollback
```

Static and generic PHP applications use the same release mechanism and an atomic `current` symlink switch. Node is represented in the runtime catalog and Nginx configuration, but blue/green process handoff is not part of the Epic A reference implementation.

## Plan before apply

`lumic app plan <app>` resolves the repository branch, active release, health gate, risks, preconditions, validation and recovery steps without changing the host. `lumic app deploy <app>` is the separate mutation boundary.

## Git sources and hosting

Lumic accepts a generic Git URL and branch. SSH keys are imported into Lumic's private state and attached by opaque credential reference; key material is never copied into application metadata, events, audit output or MCP responses. GitHub/GitLab-specific deploy keys, webhooks and multi-node mirrors are deferred ecosystem integrations.

Hosted repositories are first-class infrastructure rather than a hidden implementation detail.

## Health and recovery

`lumic app deploy` maintains a local bare mirror, resolves the configured branch to an exact commit, checks it out into a new release directory, runs only the runtime's bounded dependency/build convention, and validates the required entry point before activation. It never edits the active release.

After activation, an enabled health check sends a local HTTP request with the application's `Host` header. An unacceptable response marks the deployment `failed_rolled_back` and atomically restores the previous release. `lumic app rollback` provides the same recovery explicitly. Five releases are retained by default, and deployment history records every phase and whether rollback was automatic.

Generic PHP installs dependencies with Composer when `composer.json` exists; Node runs `npm ci` only when a supported lockfile exists. Arbitrary project build hooks, database migrations, shared-path declarations and zero-downtime Node handoff remain future work rather than hidden shell execution.

The operator UI shows deployment history, source commit, per-phase results and final status. Deploy and rollback are session-authenticated, CSRF-protected confirmation actions that call this same deployment service; they do not bypass plans, health gates, events or audit behavior.
