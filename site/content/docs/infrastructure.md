+++
title = "Git, environments and multiple nodes"
description = "Host Git, clone portable environments and coordinate autonomous Lumic nodes."
weight = 55
[extra]
kicker = "INFRASTRUCTURE"
status = "Epic D reference workflow implemented"
+++

Lumic treats a small group of VPSs as explicit infrastructure without turning them into a cluster. Every node remains independently operable. A coding agent can inspect each node's MCP endpoint, exchange public enrollments, transform an application definition and ask each node to perform its own health-gated deployment.

## Native Git

The first-class [repository contract](@/docs/repositories.md) creates namespaced managed bare repositories, imports remotes, registers explicitly discovered external storage, exposes authenticated Smart HTTP and keeps fetch/push operations explicit. `lumic git host`, `lumic git mirror` and `lumic git trigger` remain compatibility commands for the original infrastructure workflow; the trigger cannot execute a caller-supplied command.

Smart HTTP is served only for managed repositories and requires the administrator bearer token. SSH repository transport and per-repository identities remain follow-up work. Push-to-deploy invokes the installed `/usr/local/bin/lumic` and then uses the ordinary application deploy/health/rollback contract.

## Portable environments

An environment bundle contains versioned application configuration: runtime, repository reference, domain, health check, processes, service relationships, secret references and release retention. It never contains secret values.

Import requires a target application ID, one of `production`, `staging` or `development`, a target domain and explicit reference transformations. Every resulting secret/credential reference must already exist on that target node. Diff output marks sensitive fields but reports only configured/missing state.

## Trust and remote operations

Node initialization creates a stable private Ed25519 signing key. Enrollment exports only the public identity, roles, endpoint, verification key and fingerprint. Registration is explicit and reversible with `infrastructure revoke`.

On a fresh installation, infrastructure status remains readable before identity initialization. The operator UI shows an initialization prompt and `/api/infrastructure` reports `local_node` as `null`. Initialize the identity with `lumic infrastructure init NODE_ID --name "Node name" --role app`; choose one or more roles appropriate to the node.

The current remote allowlist is application deploy and rollback. Requests are signed, target-bound, expire in at most five minutes and are rejected on replay. The coding agent carries the JSON request between node MCP endpoints or CLI sessions; Lumic does not expose a generic remote command runner and does not require a central hub.

## Coordination and failure boundaries

A coordinated deployment records the target environment and each `node=application` member. Deployments remain node-local. The coordinator records running/succeeded/failed/rolled-back results and health. The boundary is explicit: stop unstarted members after the first failure and use normal rollback only for members changed by this coordination.

Worker and reverse-proxy memberships are declarative topology records. Resource endpoints explicitly name provider node/resource, consumer node/resource, protocol, host, port, health path and optional target-local secret reference. Lumic does not infer an overlay network or distributed scheduler.

## Recovery

- Revoke a compromised or retired peer, then distribute a fresh public enrollment after rotating/reinitializing its node credentials.
- Generate or import missing secrets on the target and retry an environment import; Lumic does not partially create the application first.
- If a signed request expires or was consumed by a failed attempt, sign a new request. Nonces are intentionally single-use.
- If one coordinated member fails, stop remaining members, inspect its node-local deployment evidence, and use the normal application rollback before reporting the final member state.

The complete two-node acceptance sequence is executable as `tests/epic-d-smoke.sh` in the source tree and runs in CI on supported Debian/Ubuntu images.
