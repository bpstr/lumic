+++
title = "Application recipes"
description = "Install and reconcile versioned application compositions over Lumic's existing capabilities."
weight = 48
[extra]
kicker = "APPLICATIONS"
status = "initial recipe engine implemented"
+++

Recipes are small, validated application definitions. They compose the same runtime, component, managed-service, domain, TLS, secret-reference, process and health-check capabilities available separately; they are not scripts and cannot run arbitrary shell.

## Lifecycle

Inspect and plan before applying:

```bash
lumic recipe catalog
lumic recipe plan static-git demo demo.example.com \
  --repository https://github.com/example/demo.git \
  --tls-email ops@example.com
lumic recipe install static-git demo demo.example.com \
  --repository https://github.com/example/demo.git \
  --tls-email ops@example.com
lumic recipe list
lumic recipe update demo
lumic recipe uninstall demo
```

`plan` validates the versioned schema, application/domain, required repository and declared environment inputs without changing state. `install` is idempotent: the same installed catalog version returns unchanged. `update` reconciles an installation against the current built-in version while retaining its resolved repository, branch, TLS contact and existing private input references. `uninstall` removes recipe metadata and generated secret material and uses the application service's recoverable delete path; app files move to Lumic trash. Recipe-created native service data is retained rather than silently purged.

Every mutation emits recipe events and audit records. Secret values are stored in private files and application state contains references only. TLS and native packages still use their existing Lumic validation, policy, health and recovery boundaries.

## Initial catalog

The one reference recipe is `static-git@1.0.0`. It requires a Git URL, creates a static application and nginx site, configures a local HTTP health check, optionally enables TLS, and creates a generated recipe secret reference. Its purpose is to prove the reusable composition mechanism.

Framework/CMS breadth—including Laravel, Drupal, WordPress, Symfony, Ghost and Forgejo—is explicitly nightly follow-up work. Recipes remain compiled-in reviewed data for now; remote signed catalog distribution is not implemented.

## MCP and UI

MCP exposes `recipe_catalog`, `recipe_installations`, `recipe_plan`, `recipe_install`, `recipe_update`, and `recipe_uninstall`. Apply calls require node mutation policy plus `approved: true`. The UI Recipes view shows catalog and installed-version state; installation remains a CLI/MCP planned workflow in this initial surface.
