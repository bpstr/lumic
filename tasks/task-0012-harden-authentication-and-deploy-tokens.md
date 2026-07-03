# Task 0012: Harden authentication and deploy tokens

Status: Completed

## Type
Security improvement

## Evidence
- Login stores a custom `auth` cookie with a binary SHA-256 value.
- `Authenticate` prints request headers with `var_dump()`.
- `/settings` returns a placeholder deploy token value `asd`.
- `/settings` displays a webhook URL ending in `/apitrigger`, but no such route exists.
- Basic Auth compares hashed environment-derived values from headers.

## Scope
- Remove debug output from auth middleware.
- Replace placeholder deploy token behavior with a real documented token strategy or remove the UI.
- Use secure cookie attributes where supported: HTTP-only, secure, same-site, expiration.
- Add logout and token rotation if tokens remain part of the product.
- Align documented webhook/API endpoints with implemented routes.

## Acceptance Criteria
- No auth middleware dumps request headers.
- Settings no longer displays fake tokens or nonexistent webhook URLs.
- Tests cover successful login, failed login, protected page redirect, and API unauthorized response.
