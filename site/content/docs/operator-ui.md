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

The installer starts `lumicd`, which already serves the UI on the VPS loopback interface. There is no `lumic ui` command to start it.

The installer also prints the initial admin token once. If you need a replacement, run this on the VPS, either in its console or through a normal SSH session:

```bash
sudo lumic ui token rotate
```

The replacement token is shown once. Lumic persists only its SHA-256 digest in private state. Next, run this on your local computer and keep the terminal open while using the UI:

```bash
ssh -N -L 8080:127.0.0.1:8080 root@server
```

Finally, open `http://127.0.0.1:8080` in a browser on that same local computer and sign in with the token. The first `8080` is the local port; `127.0.0.1:8080` after it is the UI listener on the VPS.

Sessions are in-memory, HttpOnly, SameSite=Strict and expire after eight hours. The session set is bounded, and daemon restart or admin-token rotation invalidates existing sessions. Mutating POSTs require a session-bound CSRF token. Responses carry a restrictive content-security policy, no-sniff, no-referrer and no-store headers.

`lumicd` refuses a non-loopback `LUMIC_UI_BIND`. For remote/shared access, keep Lumic on loopback and place an authenticated TLS reverse proxy in front of it; direct unauthenticated exposure is unsupported.

## Current views and actions

The black/white UI uses a responsive, grouped sidenav modeled on Rust/UI's
`Sidenav with Grouped Sections` block. Monitor, workload and system capabilities
remain visually separate; the active destination is identified in both markup and
presentation. On narrow screens the same navigation is available from a
keyboard-operable disclosure panel. The shell is still rendered by Rust/Axum, so
this improvement does not add a browser application runtime or duplicate Lumic's
application behavior.

The UI provides:

- live server identity followed directly by accessible radial charts for normalized 1-minute load,
  memory use, and root-disk use, plus rolling CPU and memory line charts; the
  dashboard samples the node every two seconds, refreshes in place, and retains
  the five-minute chart history while navigating within the current browser tab;
- application list/detail, typed service references, and durable lifecycle operation progress/failure details;
- evidence-backed application fingerprint and dependency graph panels;
- deployment history, phases and commit detail;
- catalog-driven service cards, shared configuration-schema/install forms, managed-instance detail,
  provider health and local data records;
- events and bounded journal logs;
- expert systemd unit, configuration/data paths, version, bind address and port;
- recipe catalog and installed-version state;
- a default installer catalog for WordPress prerequisites, PHP, all built-in managed-service
  packages, nginx, Apache, Node.js, and per-user NVM, with
  installed/candidate versions
  and a CSRF-protected plan/confirm/setup flow;
- host accounts, listeners, mounts, timers and pending updates;
- infrastructure identity, trusted/revoked peers, Git repositories/mirrors, portable environments, endpoints, memberships and coordinated deployment state; on a fresh node, the page remains available and shows the CLI command required to initialize its identity;
- confirmed restart, deploy, rollback and security-update actions.

Safe actions call the existing shared services and therefore retain their validation, health gates, rollback behavior, events and audits. Installation registers `lumicd.service`; inspect it with `systemctl status lumicd.service` and logs with `journalctl -u lumicd.service`.

The attention card uses the same `AttentionService` as `lumic how-are-you` and MCP. A selected personality changes its phrasing, but the card always includes the complete factual summary and never suppresses a warning.

The authenticated `/api/infrastructure` endpoint exposes the same read model as JSON. Before node initialization its `local_node` field is `null` and the remaining infrastructure collections are still available. The Repositories section lists managed and external repositories, shows live bare/HEAD/object status, and submits CSRF-protected create/import operations through the shared repository service. Infrastructure and application-intelligence mutations otherwise remain in CLI/MCP. Service pages read the built-in resource catalog directly and submit approved installs through the same manager used by CLI/MCP. Installer setup remains a separate fixed native-package surface. Fine-grained identities, persistent sessions and fleet-wide mutation forms remain follow-up work.
