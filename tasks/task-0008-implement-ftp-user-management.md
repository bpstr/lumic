# Task 0008: Implement FTP user management

Status: Completed

## Type
Incomplete feature

## Evidence
- README marks "Create and manage FTP users" incomplete.
- The server creation form shows "Create FTP user" but disables it.
- `CreateFtpUserCommand` hard-codes `useradd -m ftp2 -s /bin/bash`.
- `resources/views/servers/ftp.blade.php` displays database credentials and contains placeholder text `aaa`.

## Scope
- Decide whether the feature should be FTP, SFTP-only system users, or another file access model.
- Add storage for FTP/SFTP users if needed.
- Implement safe user creation with validated usernames, home directories, shells, and permissions.
- Replace placeholder UI with actual FTP/SFTP user management.
- Document required server packages and security model.

## Acceptance Criteria
- No hard-coded user such as `ftp2` remains.
- The FTP page displays relevant user/access data, not database credentials.
- User creation is validated and tested without creating real system users.
