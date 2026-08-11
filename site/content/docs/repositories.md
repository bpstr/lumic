+++
title = "Git repositories"
description = "Create, import, discover, adopt and synchronize host-native Git repositories."
weight = 54
[extra]
kicker = "REPOSITORIES"
status = "Implemented"
+++

Lumic manages Git repositories as first-class host resources. Managed repositories are bare, namespaced, locked during mutation and stored under `/var/lib/lumic/repositories/{namespace}/{name}.git` by default. External repositories can be discovered beneath explicit allowlisted roots and registered without changing their contents.

The older `lumic git host`, `mirror` and push-trigger commands remain available for compatibility. New work should use the provider-neutral `lumic repo` contract.

## Status, plan and apply

```bash
lumic repo list
lumic repo status default/api
lumic repo plan-create api
sudo lumic repo create api
sudo lumic repo import api https://github.com/example/api.git
```

`plan-create` is read-only and reports the exact destination, risk, validation and recovery. Create and import initialize a group-shared bare repository with hooks disabled. Repeating create for the same registered identity is a no-op; an unregistered directory at the target path is never overwritten.

Branch and tag inspection is also read-only:

```bash
lumic repo branches default/api
lumic repo tags default/api
```

## External discovery and adoption

Discovery never crawls the unrestricted host. Each root must appear in `git.discovery_roots`, traversal is bounded, symbolic links are ignored, and only `.git` or `*.git` directories are reported.

```bash
lumic repo discover /srv/git
sudo lumic repo register api /srv/git/api.git
sudo lumic repo adopt default/api
```

Registration changes only Lumic metadata. Adoption creates a managed bare clone and leaves the registered external repository in place, including working-tree `.git` directories. Deleting an external registration leaves its files unchanged. Deleting managed storage moves it into recoverable Lumic trash.

## Remotes and credentials

Remote URLs are validated and passed to Git as separate arguments. GitHub, GitLab and Bitbucket are detected from the host name; every other accepted host is provider `generic`. Credentials are stored separately and referenced by name—Lumic never persists a token inside a remote URL.

```bash
lumic repo remote-add default/api origin https://github.com/example/api.git \
  --credential-reference github-api
lumic repo fetch default/api origin
lumic repo push default/api origin
```

Mirror push additionally requires the remote to have been registered with `--mirror`. Fetch and push are explicit operations; Lumic does not perform hidden network synchronization while reading status.

## Deployment configuration

A repository can carry one validated deployment configuration that associates a branch with a Lumic application and destination. Planning is read-only; applying the configuration writes repository state, audit data, and a `repository.deployment.configured` event.

```bash
lumic repo plan-deployment default/api api /var/lib/lumic/apps/api \
  --branch main --strategy atomic --health-url http://127.0.0.1/health
sudo lumic repo configure-deployment default/api api /var/lib/lumic/apps/api \
  --branch main --strategy atomic --deploy-on-push --keep-releases 5
```

Strategies are `atomic` and `in_place`. Destinations must be absolute and normalized; `/`, core system directories, and Lumic's state root cannot be targeted directly. Commands are stored as argv vectors rather than shell strings, shared paths must be relative and traversal-free, retention is bounded to 1–100 releases, and HTTP health checks are restricted to the local server.

This repository contract records deployment intent. Release execution, history, rollback, pruning, and health-driven automatic rollback remain provided by the application deployment surface (`lumic app deploy`, `deployments`, and `rollback`). The compatibility `lumic git trigger` surface remains the active push-receive trigger until the provider-neutral repository receiver consumes `deploy_on_push`; configuring the flag does not install a hook or mutate an application by itself.

## Smart HTTP

The Rust UI server exposes authenticated Git Smart HTTP at `/git/{namespace}/{name}.git/...` through the native `git-http-backend`. Use the UI administrator token as an HTTP Bearer token. Anonymous reads and writes are rejected. Managed repositories only are exposed; external registrations are not served automatically.

The administrator token currently grants both repository read and write access. Separate per-repository identities and `repository:read`/`repository:write` grants are follow-up policy work, so do not expose the loopback UI listener publicly as a general-purpose forge.

## Configuration

Lumic reads `/etc/lumic/config.toml`, or the path named by `LUMIC_CONFIG_FILE`:

```toml
[git]
enabled = true
repository_root = "/var/lib/lumic/repositories"
http_enabled = true
http_path = "/git"
default_namespace = "default"
default_branch = "main"
discovery_roots = ["/srv/git", "/var/lib/gitea/repositories"]
```

Repository state is private, atomic and opened without following symbolic links. Mutations acquire both the repository resource lock and a shared registry lock so concurrent updates cannot overwrite one another. Persisted identities, paths, remote URLs and deployment settings are validated again when decoded. Branch and tag reads are bounded at 1000 entries and return an explicit error instead of silently presenting incomplete state. Mutations emit structured audit records plus `repository.created`, `repository.imported`, `repository.adopted`, `repository.deleted`, `repository.remote.added`, `repository.remote.removed`, `repository.fetched`, `repository.pushed`, and `repository.deployment.configured` events.

## MCP and UI

MCP exposes list/get/status, create and deployment planning, create/import/delete, deployment configuration, discovery/register/adopt, branch/tag listing, remote add/remove, fetch and push tools. Mutation tools require `approved=true`. The operator UI provides repository list/detail, create, import, and deployment configuration forms over the same service and CSRF/session protections as other admin actions.

## Gitea and Gogs

Lumic can install one Gitea or Gogs managed service against the configured `git.repository_root`:

```bash
lumic managed-service plan-install git-forge gitea
sudo lumic managed-service install git-forge gitea
# Alternatively:
sudo lumic managed-service install git-forge gogs
```

The installer uses pinned, SHA-256-verified upstream artifacts, a dedicated service account, and the shared `lumic-git` group. It reconciles existing repository group permissions, and repositories subsequently created or imported by Lumic remain group-writable. Both forges listen only on loopback and use their own SQLite database for application metadata.

Filesystem compatibility is intentionally separate from metadata ownership: Lumic does not silently create forge database records for repositories that already exist on disk. Register or import those repositories through the selected forge when they must appear in its UI. Only one forge can own a Lumic repository root. Forgejo remains usable through external discovery/registration but does not yet have a built-in installer.
