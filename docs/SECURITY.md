# Security model

Lumic is privileged infrastructure software. Security is a product feature, not cleanup work.

## Default stance

- MCP does not receive a generic root shell.
- Commands use direct argv execution, not interpolated shell strings.
- Native package operations validate package names against configured policy/trust sources.
- Risky capabilities are denied by default or approval-gated.
- Secrets are redacted from logs/audits and never returned by status endpoints unless explicitly requested through a secret-management capability.
- Bind interfaces conservatively; remote access requires authentication and encryption.

## Package allowlisting

Do not reimplement apt. Wrap it.

The policy layer decides whether a requested package/repository is trusted. Initial catalog entries may be exact names. Future policy may support reviewed prefixes/version constraints/trusted repository identities. Inputs must remain typed and validated; “`apt-get install *`” must never become equivalent to arbitrary command execution.

## Raw execution

A future `system.exec` capability, if implemented, is separate from normal operation, disabled by default, auditable and optionally approval-required. Core functionality must not depend on it.

## Operations

Mutations should include actor/interface/correlation metadata. Capture before/after state when feasible. Record failures too. Plans should identify destructive actions and irreversible changes.

## Agent safety

MCP descriptions must make risk explicit. Prefer small typed tools to one omnipotent tool. AI reasoning is not a permission boundary; Lumic validates every operation independently.

The local stdio MCP server denies all mutation tools unless its operator starts it with `LUMIC_MCP_ALLOW_MUTATIONS=1`; each mutation also requires `approved=true`. This is an initial coarse node policy, not remote authentication. Do not bridge the stdio server to a network. Repository tools exchange credential references only; private keys are imported locally with mode `0600` and audit arguments redact their values.

Nightly self-update requires a release SHA-256 asset, verifies the candidate before replacement, retains the previous executable, verifies again after installation, and restores the backup on postflight failure. Current nightly artifacts are x86_64 only.

## Managed services and operator UI

PostgreSQL/Redis reference settings are provider-allowlisted and limited to loopback. Database identifiers and resource IDs are validated before direct-argv native execution. Generated database passwords come from `/dev/urandom`, are stored in mode-`0600` files, are passed to `psql` on stdin and are represented in state/UI/CLI/MCP/audit only by secret reference. Material configuration writes are atomic and record both backups and newly created paths for rollback after restart/health failure.

`lumicd` refuses a non-loopback UI bind. Admin-token rotation prints the token once and stores only SHA-256; authentication establishes an eight-hour in-memory session. Cookies are HttpOnly and SameSite=Strict, actions use session-bound CSRF tokens, and browser responses carry CSP, no-sniff, no-referrer and no-store headers. Because the default transport is local HTTP, remote access must use an SSH tunnel or authenticated TLS reverse proxy; persistent/fine-grained UI identities and throttling remain hardening work.
