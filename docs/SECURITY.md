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

The local stdio MCP server denies all mutation tools unless its operator starts it with `LUMIC_MCP_ALLOW_MUTATIONS=1`, grants the required comma-separated `LUMIC_MCP_SCOPES`, and the call supplies `approved=true`. Existing tools use the compatibility `mutations` scope; operations use separate `operations.signal`, `operations.configure`, `operations.automate` and `operations.run` scopes (with `operations.*` wildcard support). This is process-level least privilege, not remote per-identity authentication. Do not bridge the stdio server to a network. Repository and webhook tools exchange credential references only; private keys/secrets use mode-`0600` storage and audit arguments redact their values.

Multi-node trust uses target-local Ed25519 signing keys and public enrollment documents. Registration and revocation are explicit. A remote operation is accepted only from a trusted public key, for the local target, before a maximum five-minute expiry and once per nonce. The allowlist is currently application deploy and rollback; signatures cannot authorize arbitrary argv or shell. Transporting these envelopes between local stdio MCP endpoints is implemented, while authenticated encrypted network MCP transport remains deferred. Compromise of one node therefore does not expose another node's private signing key or secret values; revoke the peer and rotate/reinitialize compromised node credentials during recovery.

Portable environment exports contain references, not secret material. Import fails before application creation if any final environment, service or repository credential reference is absent on the target. Generated environment secrets are random mode-`0600` files, and configuration diff reports only configured/missing state for sensitive fields. Endpoint records reject embedded credentials and non-loopback plaintext management URLs.

Nightly self-update requires a release SHA-256 asset, verifies the candidate before replacement, retains the previous executable, verifies again after installation, and restores the backup on postflight failure. Current nightly artifacts are x86_64 only.

## Managed services and operator UI

PostgreSQL/Redis reference settings are provider-allowlisted and limited to loopback. Database identifiers and resource IDs are validated before direct-argv native execution. Generated database passwords come from `/dev/urandom`, are stored in mode-`0600` files, are passed to `psql` on stdin and are represented in state/UI/CLI/MCP/audit only by secret reference. Material configuration writes are atomic and record both backups and newly created paths for rollback after restart/health failure.

Operations webhooks require HTTPS except explicit loopback test destinations, forbid URL credentials/control characters, sign bounded JSON with HMAC-SHA256, keep the secret off argv, enforce timeouts and bounded retries, and preserve delivery outcome without response bodies. Provider inputs are bounded typed data. Deterministic remediation resolves only to an allowlisted typed systemd restart, with cooldown, attempt limit and post-action state verification. Configuration snapshots are private and recoverable. Backup verification checks recorded SHA-256, size and native format header; verification is evidence, not a substitute for tested off-node recovery.

`lumicd` refuses a non-loopback UI bind. Admin-token rotation prints the token once and stores only SHA-256; authentication establishes an eight-hour in-memory session. Cookies are HttpOnly and SameSite=Strict, actions use session-bound CSRF tokens, and browser responses carry CSP, no-sniff, no-referrer and no-store headers. Because the default transport is local HTTP, remote access must use an SSH tunnel or authenticated TLS reverse proxy; persistent/fine-grained UI identities and throttling remain hardening work.

## Recipes and host operations

Recipes are reviewed compiled-in data, not executable YAML. Schema and all required repository/environment inputs validate before application mutation. Setup is limited to existing typed health/process operations; generated values live in mode-`0600` secret files and public state carries references only. Uninstall uses recoverable application deletion and does not silently purge native service data.

Host operations use fixed executables with separated arguments. Account names, systemd units, firewall IP/CIDR and protocol/port, journal filters, calendar values and managed paths are validated. Permission changes reject `/`, relative paths and symlink targets. PID 0, PID 1 and Lumic itself cannot be signalled. Remediation is an explicit enum (verified service restart, process terminate, bounded journal vacuum), not a command string. MCP host/recipe mutations retain the coarse node-policy plus per-call approval gate; the UI's security-update action retains session/CSRF confirmation.
