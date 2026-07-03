# Task 0007: Implement cron management

Status: Completed

## Type
Incomplete feature

## Evidence
- README marks "Manage cron jobs" incomplete.
- `resources/views/servers/cron.blade.php` posts to `/servers/{id}/cron`, but only a GET route exists.
- `database/migrations/04_create_cronjobs_table.php` defines a `cronjobs` table, but no model or route writes to it.
- `CronjobCreateCommand` is a copy of `NginxConfigCommand` with the duplicate signature `nginx:config {server}`.
- `GenerateCronJobTable` reads system crontab and rewrites a Blade partial.

## Scope
- Add a `Cronjob` model and relationship to `Server`.
- Implement POST `/servers/{id}/cron` with validation for cron expressions and commands.
- Replace or repair `CronjobCreateCommand` with a correctly named command.
- Decide whether Lumic should manage a database-backed desired state, the system crontab, or both.
- Render cron jobs from application data instead of rewriting a Blade partial as runtime state.

## Acceptance Criteria
- Users can create and list cron jobs for a server.
- Command names are unique and accurate.
- Tests cover validation and storage without modifying the real system crontab.
