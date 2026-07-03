# Task 0002: Harden shell command execution

Status: Completed

## Type
Security improvement

## Evidence
- `app/Console/CommandBase.php` uses `Process::fromShellCommandline($cmd)`.
- Many commands concatenate model or environment values into shell strings.
- Risky commands include MySQL provisioning, Certbot, Nginx restart, directory ownership, Git clone/pull/reset, hook execution, and rsync.

## Scope
- Replace shell-string execution with argument-array `Process` calls wherever possible.
- Add a small command-runner API that captures output, exit code, timeout, and command metadata without leaking secrets.
- Escape or reject unsafe values before they can reach shell commands.
- Redact tokens and passwords from thrown exceptions and logs.

## Acceptance Criteria
- No new code passes user-controlled values into `fromShellCommandline`.
- Commands using passwords or tokens do not expose them in exception messages.
- Unit tests cover successful execution, non-zero exit behavior, and secret redaction.
