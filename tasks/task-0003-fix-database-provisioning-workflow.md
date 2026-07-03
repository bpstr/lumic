# Task 0003: Fix database provisioning workflow

Status: Completed

## Type
Bug

## Evidence
- `CreateDatabaseCommand` signature is `db:create {database}` and expects a `Database` model or id.
- `ServerSetupJob` calls `Artisan::call('db:create', ['name' => ..., 'user' => ..., 'pass' => ...])`, which does not match the command signature.
- `Database` model fields are `name`, `username`, and `password`; setup code references `user` and `pass`.
- `POST /servers/{id}/db` creates a row but does not appear to call `db:create`.

## Scope
- Make database creation use one consistent contract.
- Decide whether `db:create` accepts a database id or explicit arguments, then update all callers.
- Use idempotent SQL where practical, such as `CREATE DATABASE IF NOT EXISTS`.
- Ensure newly created database records are actually provisioned in MySQL.
- Add validation for database name, username, and password.

## Acceptance Criteria
- Creating a server provisions its initial database through a working command path.
- Creating a database from `/servers/{id}/db` provisions it or clearly queues/provides status for provisioning.
- Tests cover command argument resolution and the route-to-command flow without touching a real MySQL server.
