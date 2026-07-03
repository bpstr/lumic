# Task 0006: Secure Git deploy command

Status: Completed

## Type
Security improvement

## Evidence
- `GitDeployCommand` concatenates repository URLs, server names, paths, and log paths into shell commands.
- It injects `GITHUB_TOKEN` into HTTPS clone URLs.
- It runs `git reset --hard`, pull, checkout, repository hook scripts, and `rsync`.
- The custom exclude-list branch sets `$exclude_list = ' --exclude-from={'.'} ';`, which appears broken.
- Paths are hard-coded to `/var/git`, `/var/www`, and `/var/www/html/resources/lists/default-excluded.lst` instead of using model/env paths consistently.

## Scope
- Validate Git URLs and allowed providers/protocols.
- Use safe process arguments and avoid exposing tokens in logs.
- Use `Server` path accessors and environment config instead of hard-coded paths.
- Fix custom `.lumic/excluded.lst` handling.
- Decide whether repository hook scripts are allowed, and if so document and sandbox their execution expectations.

## Acceptance Criteria
- Deploy command uses the configured server branch and paths.
- Secrets are redacted from logs and exceptions.
- Default and custom exclude lists work in tests.
- Unsafe Git URLs or server names are rejected before command execution.
