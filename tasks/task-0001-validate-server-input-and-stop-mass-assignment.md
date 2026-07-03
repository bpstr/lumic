# Task 0001: Validate server input and stop unsafe mass assignment

Status: Completed

## Type
Bug / security improvement

## Evidence
- `routes/web.php` creates servers with `Server::create(request()->all() + [...])`.
- `routes/web.php` updates servers with `$server->update(request()->all())`.
- The only current validation is an empty-domain check and a TODO comment for PHP version validation.
- User-controlled values later influence filesystem paths, Nginx templates, Git URLs, shell commands, and rendered configs.

## Scope
- Add explicit validation for server create/update inputs.
- Accept only supported fields: domain, path, php, git, branch, template, and initial setup flags.
- Validate `template` against `config('server.templates')`.
- Validate `php` against `AVAILABLE_PHP_VERSIONS`.
- Validate `path` as a relative path without `..`, leading slash, control characters, or shell metacharacters.
- Validate `domain` with a strict domain-name rule.
- Replace `request()->all()` mass assignment with whitelisted payloads.

## Acceptance Criteria
- Invalid create/update requests redirect back with an error and do not write a server row.
- Server create/update paths never mass-assign raw request payloads.
- Tests cover valid input, invalid domain, invalid template, invalid PHP version, and unsafe path.
