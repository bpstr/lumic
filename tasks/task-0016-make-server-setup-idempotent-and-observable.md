# Task 0016: Make server setup idempotent and observable

Status: Completed

## Type
Reliability improvement

## Evidence
- `ServerSetupJob` scans every `storage/blocks/*.conf` and moves matching files into the Nginx path.
- It then prepares directories, creates log directories, creates databases, requests SSL, installs templates, and restarts Nginx.
- Partial failures can leave moved configs or half-provisioned resources without status tracking.
- `app/Models/Job.php` exists but there is no visible user-facing setup status.

## Scope
- Define a setup state machine or provisioning status per server.
- Make each setup step idempotent.
- Record setup logs and step outcomes.
- Only restart Nginx after config validation succeeds.
- Provide user-visible error/status feedback.

## Acceptance Criteria
- Re-running setup for a server is safe.
- Failed steps are recorded and visible.
- Nginx restart is skipped when config validation fails.
- Tests cover repeated setup and simulated step failure.
