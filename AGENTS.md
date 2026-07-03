# AGENTS.md

Instructions for AI coding agents working on Lumic.

<!-- codebase-memory-mcp:start -->
## Codebase Knowledge Graph

This project uses codebase-memory-mcp. Prefer graph tools over raw text search for code discovery:

1. `search_graph` or equivalent graph search to find functions, classes, routes, and variables.
2. `trace_path` to inspect callers/callees when changing behavior.
3. `get_code_snippet` to read exact function/class source after finding a qualified name.
4. `query_graph` for complex relationships.
5. `get_architecture` for the high-level project map.

Fall back to `rg`, file reads, or shell tools for README/docs, configs, string literals, generated files, or when graph results are insufficient.
<!-- codebase-memory-mcp:end -->

## Application Summary

Lumic is a Laravel Lumen 10 server-management panel for a single VPS. It manages website/server records, Nginx server-block templates, Let's Encrypt SSL certificates, MySQL databases, basic status views, and Git deployment workflows.

This repository is not just a web CRUD app. Several commands can modify host-level services and filesystem paths such as Nginx, MySQL, Certbot, `/var/www`, and `/var/git`.

## Stack

- PHP 8.1+
- Laravel Lumen 10
- Eloquent ORM
- Blade templates
- Artisan console commands
- Queue jobs
- PHPUnit 10
- VitePress documentation

## Repository Map

- `README.md`: Product overview, installation notes, and feature checklist.
- `routes/web.php`: Primary UI routes and `/api/status`.
- `app/Models/Server.php`: Server/site model and computed filesystem paths.
- `app/Models/Database.php`: Database credential model.
- `app/Console/CommandBase.php`: Shared shell execution helper.
- `app/Console/Commands`: Host-level operational commands.
- `app/Jobs`: Setup, SSL, and deploy orchestration.
- `resources/views`: UI, layout, sample Nginx configs, and generated cron block.
- `config/server.php`: Available Nginx template metadata.
- `database/migrations`: Schema for servers, databases, jobs, and cron jobs.
- `tests`: PHPUnit tests.

## Development Commands

Install dependencies:

```bash
composer install
npm install
```

Run tests:

```bash
vendor/bin/phpunit
```

Run docs locally:

```bash
npm run docs:dev
```

Build docs:

```bash
npm run docs:build
```

## Architecture Notes

Most user-facing behavior is currently defined as route closures in `routes/web.php`, although controller classes exist. Persistent state is concentrated in `Server` and `Database` models.

Operational workflows generally follow this shape:

1. A route creates or updates a model.
2. The route calls an Artisan command or dispatches a job.
3. The command/job renders Blade templates or shells out to system tools.
4. System files, services, databases, or deploy directories are updated.

Key commands:

- `nginx:config {server}`: Render an Nginx config into `storage/blocks/{server}.conf`.
- `dir:prepare {server}`: Create project directories and run `chown`.
- `db:create {database}`: Create MySQL database/user/grants.
- `ssl:certificate {server} {--force=}`: Run Certbot and update SSL timestamp.
- `template:install {server}`: Write a starter `index.html`.
- `git:deploy {server}`: Clone/pull/reset Git repo and rsync into web root.
- `nginx:restart`: Restart Nginx.
- `cron:table`: Render the current crontab as a Blade table block.

Key jobs:

- `ServerSetupJob`: Processes pending Nginx config blocks and runs server provisioning.
- `ForceSslCertJob`: Forces certificate generation/renewal for a server.
- `GitDeployJob`: Scans deploy logs and invokes `git:deploy` for eligible servers.

## Safety Rules

- Do not run commands that touch Nginx, MySQL, Certbot, `/var/www`, `/var/git`, service restarts, `git reset --hard`, or `rsync` unless the user explicitly asks and the environment is safe.
- Treat request input as unsafe. Validate domains, paths, Git URLs, template names, database names, usernames, and passwords before they reach shell commands or SQL.
- Be extremely careful with `CommandBase::exec()`: it runs shell commands through Symfony Process from a shell command line.
- Prefer tests or mocks over exercising real host-level side effects.
- Preserve user changes in the worktree. Do not reset, delete, or overwrite unrelated files.

## Testing Expectations

When changing behavior, run the most focused safe tests available:

```bash
vendor/bin/phpunit
```

For command changes, add coverage around argument handling, generated command strings, rendered file contents, or model updates. Avoid invoking real system tools in tests.

For route changes, cover auth, validation, redirects, and model writes. The current app uses route closures, so small feature tests are often more useful than heavy refactors.

## Security and Correctness Hotspots

- `routes/web.php` mass-assigns request data into models in multiple places.
- `/explorer` uses direct `mysqli` queries against the local MySQL server.
- `CreateDatabaseCommand` builds SQL passed through shell commands.
- `GitDeployCommand` injects the GitHub token into clone URLs and runs repository hook scripts from `.lumic/hooks`.
- `SslCertificateCommand` runs Certbot for real domains.
- `RestartNginxCommand` restarts the host service.
- Auth cookies and Basic Auth credentials are custom hashed values based on environment variables.

Changes in these areas should be reviewed as security-sensitive.

## Known Caveats

- The README marks PHP extension management, FTP users, deploys, cron management, aliases, and file browsing as incomplete or partial.
- `GitDeployJob` does not directly deploy only the server passed to its constructor; it scans servers and deploy logs.
- Some naming in setup/database orchestration appears inconsistent with the `Database` model fields. Verify behavior before depending on it.
- Many commands assume an Ubuntu-style production host with Nginx, MySQL, Certbot, and appropriate permissions.

## Agent Workflow

1. Read `README.md` and this file before broad changes.
2. Use the code graph to locate affected models, commands, jobs, routes, and views.
3. Inspect call paths before changing host-level behavior.
4. Make the smallest change that satisfies the task.
5. Add or update focused tests when practical.
6. Run safe verification commands and report anything that could not be tested locally.
