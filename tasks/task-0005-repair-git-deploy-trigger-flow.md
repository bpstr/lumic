# Task 0005: Repair Git deploy trigger flow

Status: Completed

## Type
Bug

## Evidence
- `/servers/{id}/deploy/trigger` dispatches `new GitDeployJob($server)`.
- `GitDeployJob` has a `Server $server` property but ignores it in `handle()`.
- `GitDeployJob` scans all servers with Git repos and deploys only when a deploy log exists with exactly one line starting with `User triggered deploy`.
- `DoJobCommand` instantiates `new GitDeployJob()` without the required constructor argument.

## Scope
- Define one clear deploy trigger model: direct per-server deploy, queued deploy request records, or deploy-log marker.
- Update `GitDeployJob` and `DoJobCommand` to follow that model.
- Initialize deploy logs safely if logs are still part of the trigger flow.
- Return user-visible deploy status or errors.

## Acceptance Criteria
- Triggering deploy for server A cannot accidentally deploy server B.
- `DoJobCommand` no longer throws constructor errors.
- Tests cover job behavior for one configured server and for missing/invalid Git config.
