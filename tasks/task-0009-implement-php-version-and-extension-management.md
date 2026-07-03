# Task 0009: Implement PHP version and extension management

Status: Completed

## Type
Incomplete feature

## Evidence
- README marks "Manage PHP versions and extensions" incomplete.
- The server form reads `AVAILABLE_PHP_VERSIONS` and lets users choose a PHP version.
- Nginx templates likely consume the selected PHP version, but there is no installer or extension-management workflow.

## Scope
- Validate selected PHP version against installed/available versions.
- Add a way to inspect installed PHP versions and extensions.
- Decide whether Lumic should install packages or only configure already-installed versions.
- Update Nginx templates and server setup to fail clearly when the selected PHP-FPM socket/version is missing.

## Acceptance Criteria
- Unsupported PHP versions cannot be selected.
- Users can see installed PHP versions/extensions or receive clear unsupported-state feedback.
- Tests cover PHP version validation and template rendering for selected versions.
