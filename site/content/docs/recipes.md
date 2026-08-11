+++
title = "Application recipes"
description = "Install and reconcile versioned application compositions over Lumic's existing capabilities."
weight = 48
[extra]
kicker = "APPLICATIONS"
status = "catalog lifecycle implemented"
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
lumic recipe plan wordpress blog blog.example.com \
  --env WORDPRESS_SITE_TITLE="Example Blog" \
  --env WORDPRESS_ADMIN_USER=admin \
  --env WORDPRESS_ADMIN_EMAIL=admin@example.com
lumic recipe install wordpress blog blog.example.com \
  --env WORDPRESS_SITE_TITLE="Example Blog" \
  --env WORDPRESS_ADMIN_USER=admin \
  --env WORDPRESS_ADMIN_EMAIL=admin@example.com
lumic recipe list
lumic recipe update demo
lumic recipe uninstall demo
```

`plan` validates the versioned schema, application/domain, required repository and declared environment inputs without changing state. `install` is idempotent: a healthy installation of the same catalog version returns unchanged. `update` reconciles an installation against the current built-in version while retaining its resolved repository, branch, TLS contact and existing private input references. `uninstall` removes recipe metadata, generated secret material, the owned nginx configuration and application-owned resources; app files move to Lumic trash. Shared runtimes, packages, managed services and recipe-created database data are retained rather than silently purged.

Every mutation emits recipe events and audit records. Secret values are stored in private files and application state contains references only. TLS and native packages still use their existing Lumic validation, policy, health and recovery boundaries.

## Built-in catalog

`static-git@1.0.0` requires a Git URL, creates a static application and nginx site, configures a local HTTP health check, optionally enables TLS, and creates a generated recipe secret reference.

`wordpress@1.0.0` provisions WordPress 6.8.2 through WP-CLI 2.12.0 with PHP 8.3, an isolated MySQL database and credential, an owned nginx site, optional TLS, and a generated administrator password. Both upstream artifacts are pinned by release URL, version, and SHA-256 digest. Lumic's shared artifact manager serializes acquisition, rejects unsafe cache entries, streams checksum verification and atomically commits only verified downloads; it reverifies cached bytes before reuse. Credentials are passed to native tools through stdin. The recipe records each durable lifecycle step so a failed apply can be inspected and retried; release activation restores the prior `current` target when setup fails.

The compiled-in application catalog also covers Laravel, Laravel with Typesense, Drupal, Symfony with PostgreSQL, Ghost, Matomo, and Forgejo. Executable recipes are available for Laravel, Laravel with Typesense, Drupal, Symfony, Ghost, and Matomo; they reuse the generic repository/runtime/service deployment lifecycle. Forgejo is currently a catalog definition for the native service/application composition and is not advertised as an executable recipe until its application driver can provide complete lifecycle recovery.

Uninstall deliberately retains artifact caches, shared runtimes, native packages, managed services, databases, users, and grants. It removes recipe-owned application state, nginx configuration, generated secrets and application bindings. Recipes remain compiled-in reviewed data for now; remote signed catalog distribution is not implemented.

## MCP and UI

MCP exposes `recipe_catalog`, `recipe_installations`, `recipe_plan`, `recipe_install`, `recipe_update`, and `recipe_uninstall`. Apply calls require node mutation policy plus `approved: true`. The UI Recipes view supports the same install review/apply, update, and uninstall lifecycle. Sensitive environment inputs are held in the authenticated session between review and confirmation and are not emitted into confirmation HTML.
