# Task 0011: Add file browser

Status: Completed

## Type
Incomplete feature

## Evidence
- README marks "List files and folders" incomplete.
- There is no route or view for browsing server docroots.
- Server model already computes docroot/directory paths.

## Scope
- Add a read-only file browser scoped to a server's docroot.
- Prevent path traversal and symlink escapes.
- Show file/folder names, size, modification time, and type.
- Decide whether downloading/viewing file contents is in scope.

## Acceptance Criteria
- Users can browse within a server docroot.
- Requests using `..`, absolute paths, or symlink escapes are rejected.
- Tests cover allowed and denied paths.
