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

The generic lifecycle is:

```text
fetch → checkout → dependencies → build → shared links → pre-activate
      → health/preconditions → activate → reload/drain → verify → cleanup
```

Activation is runtime-specific. PHP can use an atomic symlink switch and graceful FPM reload. A Node process may start a new process, pass a health check, switch reverse-proxy traffic, drain the old process and then stop it.

## Plan before apply

Dangerous deployment operations should expose a plan that reports detected migrations, worker changes, disk requirements, health checks, expected interruption class and rollback availability.

## Git sources and hosting

Lumic supports external Git providers such as GitHub/GitLab, generic Git remotes, locally hosted bare repositories, and eventually local mirrors/caches for multi-node deployment.

Hosted repositories are first-class infrastructure rather than a hidden implementation detail.

## Current nightly behavior

`lumic app deploy` maintains a local bare mirror, resolves the configured branch to an exact commit, checks it out into a new release directory, validates the required entry point, and only then atomically replaces `current`. It never edits the active release. A failed pre-activation validation is recorded and the incomplete release is removed. `lumic app rollback` activates the prior retained successful release. Five releases are retained by default.

This is not the complete production deployment promise yet. HTTP post-activation health, automatic rollback after an HTTP failure, shared-path linking, Nginx/FPM reload, structured build hooks, and service-aware activation are still planned.
