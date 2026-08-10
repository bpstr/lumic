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

The stdio and HTTP MCP transports deny all mutation tools unless the process has `LUMIC_MCP_ALLOW_MUTATIONS=1`, the required comma-separated `LUMIC_MCP_SCOPES`, and the call supplies `approved=true`. Existing tools use the compatibility `mutations` scope; operations use separate `operations.signal`, `operations.configure`, `operations.automate` and `operations.run` scopes (with `operations.*` wildcard support). This is process-level least privilege, not per-identity authorization. The HTTP transport is opt-in, bearer-authenticated, and loopback-only; public access requires an HTTPS reverse proxy. Only a SHA-256 digest of its separate token is stored. The restricted-SSH installer path uses an OpenSSH forced command so its dedicated key can start `lumic mcp serve` but cannot request a general shell. Repository and webhook tools exchange credential references only; private keys/secrets use mode-`0600` storage and audit arguments redact their values.

Application intelligence reads only regular UTF-8 discovery/configuration files up to 1 MiB beneath the managed application root. Fingerprint/configuration/plan output contains dotenv key names and configured/unset state, never values. Mutation rejects duplicate target keys, writes atomically with mode `0600`, creates a content-hashed private snapshot, verifies snapshot ownership/integrity before rollback, and never interpolates configuration into a shell. `application_integration_apply` and rollback require the `application.integrate` MCP scope plus approval.

Incident analysis is an explicit external disclosure boundary. `incident_analyze` requires the `incident.analyze` scope and approval, redacts recursively by sensitive field name, limits the evidence package to 256 KiB and signs it with the configured destination secret. Responses must match the closed diagnosis/evidence/typed-remediation schema, may cite only supplied evidence, and are always forced to advisory mode. A proposal cannot execute itself; operators or agents must separately use the normal scoped Lumic service or snapshot operation. No command or shell field is accepted.

Multi-node trust uses target-local Ed25519 signing keys and public enrollment documents. Registration and revocation are explicit. A remote operation is accepted only from a trusted public key, for the local target, before a maximum five-minute expiry and once per nonce. The allowlist is currently application deploy and rollback; signatures cannot authorize arbitrary argv or shell. Agents can transport envelopes through stdio/restricted SSH or through Streamable HTTP behind TLS. HTTP bearer authentication is node-level rather than per-identity OAuth. Compromise of one node therefore does not expose another node's private signing key or secret values; revoke the peer and rotate/reinitialize compromised node credentials during recovery.

Portable environment exports contain references, not secret material. Import fails before application creation if any final environment, service or repository credential reference is absent on the target. Generated environment secrets are random mode-`0600` files, and configuration diff reports only configured/missing state for sensitive fields. Endpoint records reject embedded credentials and non-loopback plaintext management URLs.

Self-update follows the node's recorded stable/nightly channel, requires a release SHA-256 asset, verifies the candidate before replacement, retains the previous executable, verifies again after installation, and restores the backup on postflight failure. Current release artifacts are x86_64 only.

Stable and nightly release workflows use commit-pinned third-party actions and publish GitHub artifact attestations for their binaries. Checksums remain mandatory for installation; operators may additionally verify provenance with `gh attestation verify <artifact> --repo bpstr/lumic`.

## Managed services and operator UI

Managed-service settings are provider-allowlisted and limited to loopback. Database identifiers and resource IDs are validated before direct-argv native execution. Generated database and bootstrap passwords come from `/dev/urandom`, are stored in mode-`0600` files, and are represented in state/UI/CLI/MCP/audit only by secret reference. Database credentials are passed to native SQL clients on stdin. Sensitive resource outputs must use the `secret://` reference form; application database bindings never embed the credential value. Material configuration writes are atomic and record both backups and newly created paths for rollback after restart/health failure. OpenSearch's single-node configuration disables its security plugin only behind the enforced loopback boundary; direct network exposure is not a supported configuration.

Certificate operations accept only the registered Certbot/Let's Encrypt provider and validated DNS names, then invoke Certbot with separated arguments. Lumic persists certificate and private-key paths, never key contents; the contact email is not stored in certificate resource state. Nginx attachment is an explicit, locked consumer operation: Lumic writes only owned configuration atomically, validates it before reload and restores the previous configuration if validation or reload fails.

Operations webhooks require HTTPS except explicit loopback test destinations, forbid URL credentials/control characters, sign bounded JSON with HMAC-SHA256, keep the secret off argv, enforce timeouts and bounded retries, and preserve delivery outcome without response bodies. Provider inputs are bounded typed data. Deterministic remediation resolves only to an allowlisted typed systemd restart, with cooldown, attempt limit and post-action state verification. Configuration snapshots are private and recoverable. Backup verification checks recorded SHA-256, size and native format header; verification is evidence, not a substitute for tested off-node recovery.

`lumicd` refuses a non-loopback UI bind. Admin-token rotation prints the token once and stores only SHA-256; authentication establishes an eight-hour in-memory session. Sessions are bounded, and token rotation or daemon restart invalidates them. Login failures are throttled in-process after five attempts in 60 seconds. Cookies are HttpOnly and SameSite=Strict, actions use session-bound CSRF tokens, and browser responses carry CSP, no-sniff, no-referrer and no-store headers. Because the default transport is local HTTP, remote access must use an SSH tunnel or authenticated TLS reverse proxy; persistent/fine-grained UI identities remain future hardening work.

## Recipes and host operations

Recipes are reviewed compiled-in data, not executable YAML. Schema and all required repository/environment inputs validate before application mutation. Setup is limited to existing typed health/process operations; generated values live in mode-`0600` secret files and public state carries references only. Uninstall uses recoverable application deletion and does not silently purge native service data.

Host operations use fixed executables with separated arguments. Account names, systemd units, firewall IP/CIDR and protocol/port, journal filters, calendar values and managed paths are validated. Permission changes reject `/`, relative paths and symlink targets. PID 0, PID 1 and Lumic itself cannot be signalled. Remediation is an explicit enum (verified service restart, process terminate, bounded journal vacuum), not a command string. MCP host/recipe mutations retain the coarse node-policy plus per-call approval gate; the UI's security-update action retains session/CSRF confirmation.

## Attention and personality

Personality is presentation, never policy or diagnosis. The renderer receives a completed factual summary and must print every fact, incident, warning, evidence reference and recommendation regardless of tone; critical severity remains explicit. Historical failures are changes only unless live state independently proves an active incident.

Personality state rejects symlinks, oversized files and unknown enum values, is written atomically with mode `0600`, and is audited. The MCP attention resource/tool exposes the structured factual summary alongside rendered text so agents never need to parse jokes as operational state. Changing personality is intentionally CLI-only in the initial implementation; MCP attention remains read-only.
