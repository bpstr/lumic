# Task 0015: Improve schema and model integrity

Status: Completed

## Type
Improvement / bug prevention

## Evidence
- `databases.server_id` and `cronjobs.server_id` are strings rather than foreign ids.
- `servers.ssl` is stored as a string while the model casts it as a date.
- `servers.branch` exists in the migration but is not fillable on the model.
- `jobs` migration drops `failed_jobs` in `down()` even though it does not create it.

## Scope
- Use proper foreign keys or documented migration strategy for existing installations.
- Store SSL timestamps in an appropriate datetime column.
- Align model fillable fields with schema fields.
- Review queue table migrations for correctness.

## Acceptance Criteria
- New installs have consistent column types and relationships.
- Existing install migration path is documented or handled.
- Model fields, casts, and migrations agree.
