# Task 0018: Review install script and document production requirements

Status: Completed

## Type
Documentation / operations improvement

## Evidence
- README installs via `bash <(curl -s ...)`.
- `lumic.sh` manages MySQL root credentials, packages, Nginx, permissions, and generated passwords.
- Application commands assume Ubuntu paths, Nginx, MySQL, Certbot, `/var/www`, `/var/git`, and writable config directories.
- Docs mention generated passwords and production setup, but runtime assumptions are spread across code and script.

## Scope
- Audit `lumic.sh` for idempotency, error handling, supported Ubuntu versions, and secret handling.
- Document required environment variables and filesystem permissions.
- Document which commands are safe to run locally versus only on a production VPS.
- Add troubleshooting steps for Nginx, Certbot, MySQL, and permissions.

## Acceptance Criteria
- README or docs clearly state production prerequisites and runtime assumptions.
- Install script risks and manual recovery steps are documented.
- Required environment variables are listed in one place.
