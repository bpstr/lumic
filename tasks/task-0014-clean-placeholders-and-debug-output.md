# Task 0014: Clean placeholders and debug output

Status: Completed

## Type
Maintenance

## Evidence
- `ServerSetupJob` calls `var_dump($item)`.
- `Authenticate` calls `var_dump(request()->headers->all())`.
- `AppServiceProvider` shares `$data = 'asdf'` with every view.
- `resources/views/servers/ftp.blade.php` contains `aaa`.
- `/settings` passes `deploy_token => 'asd'`.
- Several commands output misleading messages such as `create db.` in non-database commands.

## Scope
- Remove debug dumps and placeholder strings.
- Replace misleading command descriptions/messages.
- Keep any useful diagnostic output behind structured logging.

## Acceptance Criteria
- Searching for `var_dump`, `asdf`, `aaa`, and placeholder token `asd` no longer finds application code occurrences.
- Command descriptions and success messages match the command behavior.
