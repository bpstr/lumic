# Task 0004: Support branch-aware Git deploys

Status: Completed

## Type
Bug / incomplete feature

## Evidence
- `database/migrations/02_create_servers_table.php` defines a `branch` column with default `main`.
- `resources/views/servers/deploy.blade.php` lets users edit branch.
- `app/Models/Server.php` does not include `branch` in `$fillable`, so updates may not persist.
- `GitDeployCommand` hard-codes `git checkout main`.

## Scope
- Add `branch` to the server fillable list.
- Validate branch names on deploy settings update.
- Update `GitDeployCommand` to checkout/pull the configured branch.
- Record deployed commit on the server when deployment succeeds.

## Acceptance Criteria
- Saving a deploy branch persists it to the database.
- Deploys checkout the configured branch instead of always using `main`.
- Tests cover branch persistence and generated Git command behavior.
