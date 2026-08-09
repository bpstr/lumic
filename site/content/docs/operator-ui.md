+++
title = "Operator UI"
description = "Operate applications and managed services through Lumic's local Rust UI."
weight = 65
[extra]
kicker = "OPERATE"
status = "initial authenticated UI implemented"
+++

`lumicd` serves the initial operator UI from the same Rust application/platform services used by CLI and MCP. It has no frontend runtime and no separate business-logic stack.

## Sign in

Generate or rotate the admin token on the node:

```bash
sudo lumic ui token rotate
```

The token is shown once. Lumic persists only its SHA-256 digest in private state. Open `http://127.0.0.1:8080` through an SSH tunnel:

```bash
ssh -L 8080:127.0.0.1:8080 root@server
```

Sessions are in-memory, HttpOnly, SameSite=Strict and expire after eight hours; daemon restart invalidates them. Mutating POSTs require a session-bound CSRF token. Responses carry a restrictive content-security policy, no-sniff, no-referrer and no-store headers.

`lumicd` refuses a non-loopback `LUMIC_UI_BIND`. For remote/shared access, keep Lumic on loopback and place an authenticated TLS reverse proxy in front of it; direct unauthenticated exposure is unsupported.

## Current views and actions

The black/white UI provides:

- live server identity and resource overview;
- application list/detail and typed service references;
- deployment history, phases and commit detail;
- managed-service list/detail, provider health and local data records;
- events and bounded journal logs;
- expert systemd unit, configuration/data paths, version, bind address and port;
- recipe catalog and installed-version state;
- host accounts, listeners, mounts, timers and pending updates;
- infrastructure identity, trusted/revoked peers, Git repositories/mirrors, portable environments, endpoints, memberships and coordinated deployment state;
- confirmed restart, deploy, rollback and security-update actions.

Safe actions call the existing shared services and therefore retain their validation, health gates, rollback behavior, events and audits. Installation registers `lumicd.service`; inspect it with `systemctl status lumicd.service` and logs with `journalctl -u lumicd.service`.

The authenticated `/api/infrastructure` endpoint exposes the same read model as JSON. Infrastructure mutations remain in CLI/MCP for now. The initial UI intentionally omits service installation/configuration forms, fine-grained identities, persistent sessions, mobile polish and fleet-wide mutation forms. Those are follow-up work rather than a reason to introduce a large frontend framework.
