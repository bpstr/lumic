# Task 0013: Refactor database explorer

Status: Completed

## Type
Improvement

## Evidence
- `/explorer` in `routes/web.php` opens a raw `mysqli` connection.
- On connection failure it calls `die(...)`.
- It builds SQL strings inline and queries MySQL metadata directly in a route closure.
- The explorer view links to `/explorer/{database}`, but no such route exists.

## Scope
- Move database exploration into a service class.
- Replace `die()` with controlled error handling and user-visible messages.
- Add route support or remove links for database-detail pages.
- Avoid leaking sensitive MySQL connection errors.
- Add tests with a mocked explorer service.

## Acceptance Criteria
- `/explorer` cannot terminate the app with raw `die()`.
- Connection failures render a safe response.
- Links in the explorer point to implemented routes or are removed.
