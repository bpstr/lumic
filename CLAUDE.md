# CLAUDE.md

Guidance for Claude Code and other coding agents working in this repository.

## Project Overview

Lumic is a Lumen 10 application for managing websites on a single Ubuntu VPS. The UI lets an authenticated root-style user create server entries, generate Nginx server blocks, prepare web directories, request Let's Encrypt certificates, create MySQL databases/users, view basic system/database status, and trigger Git-based deployments.

The README describes the product as a lightweight server-management panel. Treat this as infrastructure-adjacent software: changes can affect Nginx, Certbot, MySQL, `/var/www`, `/var/git`, cron, and live service restarts.

## Tech Stack

- PHP 8.1+
- Laravel Lumen 10
- Eloquent models and migrations
- Blade views in `resources/views`
- Artisan commands and queued jobs for operational work
- PHPUnit 10 for tests
- VitePress for documentation in `docs`

## Important Paths

- `routes/web.php`: Main web routes, login flow, authenticated UI, and internal API route group.
- `app/Models/Server.php`: Website/server model. Computed paths include docroot, git root, Nginx config path, and deploy log path.
- `app/Models/Database.php`: Database credential model. The `password` attribute uses Laravel encrypted casting.
- `app/Console/Commands`: Operational commands that create directories, write Nginx configs, create databases, install templates, run Certbot, restart Nginx, and deploy Git repositories.
- `app/Jobs`: Job wrappers around setup, SSL renewal, and Git deployment workflows.
- `resources/views/sample`: Nginx, crontab, and HTML templates rendered by commands.
- `database/migrations`: Small schema for servers, databases, jobs, and cron jobs.
- `config/server.php`: Available server/Nginx template choices.

## Runtime Concepts

### Authentication

The web UI compares posted `name` and `pass` values against `ROOT_USER_NAME` and `ROOT_USER_PASS`, then stores an `auth` cookie containing a salted SHA-256 hash. The `/api` group uses `BasicAuth`, where the expected username and password are also hashed values derived from the same environment variables.

Do not change auth behavior casually. If touching it, add tests and inspect both `Authenticate` and `BasicAuth`.

### Servers

`Server` records hold user-facing site configuration:

- `domain`
- `name`
- `path`
- `ssl`
- `php`
- `git`
- `commit`
- `template`

Computed accessors derive paths from environment variables:

- `DOCROOT_PATH`
- `GITROOT_PATH`
- `NGINX_ROOT_PATH`

Keep path construction centralized on the model when possible.

### Databases

Each server can have many `Database` records. A new server currently gets an initial database in `POST /servers/add`. Database creation is implemented by `db:create`, which shells out to `mysql` with `MYSQL_ROOT_USER` and `MYSQL_ROOT_PASS`.

Be careful when changing names or credentials: model fields are `name`, `username`, and `password`.

### Operational Commands

Most system work goes through Artisan commands:

- `nginx:config {server}` writes a generated Nginx config into `storage/blocks`.
- `dir:prepare {server}` creates docroot/git directories and runs `chown`.
- `ssl:certificate {server} {--force=}` runs Certbot and updates `servers.ssl`.
- `nginx:restart` runs `service nginx restart`.
- `db:create {database}` creates a MySQL database, user, grants, and flushes privileges.
- `template:install {server}` writes a sample `index.html`.
- `git:deploy {server}` clones/pulls a repository into `/var/git/{server}` and rsyncs it into `/var/www/{server}`.
- `cron:table` reads `crontab -l` and rewrites `resources/views/blocks/cronjob.blade.php`.

These commands use shell execution through `CommandBase::exec()`. Validate and escape any new shell inputs; never concatenate untrusted request values into shell commands without explicit sanitization.

### Jobs

- `ServerSetupJob` scans `storage/blocks/*.conf`, resolves matching `Server` records, prepares directories, moves configs to Nginx, creates databases, requests SSL, installs an HTML template, then restarts Nginx.
- `ForceSslCertJob` calls `ssl:certificate` with `force`.
- `GitDeployJob` currently scans all servers with a Git repository and deploys only when a deploy log exists with exactly one line starting with `User triggered deploy`.

`GitDeployJob` does not simply deploy the server passed to its constructor. Verify the desired behavior before modifying deploy triggers.

## Local Development

Install PHP dependencies:

```bash
composer install
```

Install docs dependencies:

```bash
npm install
```

Run tests:

```bash
vendor/bin/phpunit
```

Run VitePress docs:

```bash
npm run docs:dev
```

This app is designed for an Ubuntu server with Nginx, MySQL, Certbot, and writable system paths. Many Artisan commands are not safe or useful on a normal development machine.

## Testing Guidance

- Prefer unit or feature tests for model accessors, route behavior, auth behavior, and command construction.
- Mock or isolate shell execution for commands that would touch Nginx, MySQL, Certbot, `/var/www`, `/var/git`, or system services.
- Do not run destructive operational commands against a real host unless the user explicitly asks and the environment is known to be disposable.

## High-Risk Areas

- `routes/web.php` contains many route closures and direct request handling. Validate inputs before using values in models, paths, SQL, or shell commands.
- `CommandBase::exec()` runs shell commands and throws on non-zero exit codes.
- `GitDeployCommand` runs `git reset --hard`, `git pull`, `git checkout main`, hook scripts, and `rsync`.
- `CreateDatabaseCommand` constructs SQL through shell `echo | mysql`.
- `SslCertificateCommand` runs Certbot against real domains.
- `RestartNginxCommand` restarts the Nginx service.
- `ServerSetupJob` moves files from `storage/blocks` into Nginx config paths.

When reviewing or changing these areas, prioritize security, escaping, idempotency, rollback behavior, and test coverage.

## Known Implementation Notes

- The README lists FTP, deploys, cron jobs, aliases, and file browsing as incomplete or partial features.
- `/explorer` uses direct `mysqli` queries for MySQL users, databases, sizes, and table counts.
- Some controller classes exist, but most current route behavior is implemented directly in `routes/web.php`.
- There are apparent naming mismatches in setup/database paths; for example, database model fields are `username/password`, while some setup code references `user/pass`. Verify before relying on these flows.
- Generated Nginx config files are first written to `storage/blocks` before setup moves them into the configured Nginx path.

## Contribution Style

- Keep changes small and operationally conservative.
- Follow existing Lumen/Eloquent conventions.
- Prefer adding focused tests around risky behavior before refactoring.
- Do not introduce broad architectural rewrites unless the user explicitly asks.
- Update README or docs when behavior visible to operators changes.
