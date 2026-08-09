# Security model

Lumic is privileged infrastructure software. Security is a product feature, not cleanup work.

The detailed fresh-VPS trust and enrollment protocol is defined in [`BOOTSTRAP_SECURITY.md`](BOOTSTRAP_SECURITY.md). That document is normative for installer, HTTPS, first-owner enrollment and remote MCP authentication work.

## Default stance

- MCP does not receive a generic root shell.
- Commands use direct argv execution, not interpolated shell strings.
- Native package operations validate package names against configured policy/trust sources.
- Risky capabilities are denied by default or approval-gated.
- Secrets are redacted from logs/audits and never returned by status endpoints unless explicitly requested through a secret-management capability.
- Bind interfaces conservatively; remote access requires authentication and encryption.
- The root/VPS password is never a Lumic credential and is never read or stored by Lumic.
- Remote MCP is never exposed over plaintext HTTP or certificate-verification bypasses.
- Authentication proves identity; it never bypasses capability policy.

## Secure first bootstrap

The intended convenience path starts with the credentials a VPS provider already gives the operator: IP address, root account and password.

Lumic may automate installation and enrollment around that SSH session, but the password must remain exclusively inside the user's OpenSSH process. Do not add password flags, password environment variables, `sshpass`, `expect`, stdin password parsing or temporary password files.

Never disable SSH host-key verification to remove the first-connect prompt. Where the provider exposes a host fingerprint, Lumic may support explicit fingerprint pinning. Otherwise normal OpenSSH trust-on-first-use remains the documented initial trust boundary.

The SSH-authenticated installer may issue a one-time initial-owner enrollment grant. That grant must be high-entropy, short-lived, single-use, purpose-bound, stored server-side only as a digest and exchanged only over verified HTTPS. It must never be accepted as an MCP/API bearer token.

## Remote MCP

The public target is `https://IP/mcp` over trusted TLS. The privileged daemon/internal MCP listener binds to loopback or a Unix socket and is reached through the managed HTTP edge.

Remote MCP authentication follows the current MCP authorization standard. Prefer per-client OAuth credentials with short-lived access tokens, refresh-token rotation/revocation and least-privilege scopes. Do not make a copied static administrator token the default developer workflow.

If trusted TLS cannot be provisioned for the node identity, keep remote MCP closed and offer a secure SSH/stdio fallback. Never fall back to self-signed public MCP plus `--insecure`.

## Package allowlisting

Do not reimplement apt. Wrap it.

The policy layer decides whether a requested package/repository is trusted. Initial catalog entries may be exact names. Future policy may support reviewed prefixes/version constraints/trusted repository identities. Inputs must remain typed and validated; “`apt-get install *`” must never become equivalent to arbitrary command execution.

## Raw execution

A future `system.exec` capability, if implemented, is separate from normal operation, disabled by default, auditable and optionally approval-required. Core functionality must not depend on it.

## Operations

Mutations should include actor/interface/correlation metadata. Capture before/after state when feasible. Record failures too. Plans should identify destructive actions and irreversible changes.

Security-sensitive events include bootstrap issuance/consumption, owner enrollment, client authorization/revocation and SSH-hardening changes. Audit metadata must never contain passwords, bootstrap secrets, access/refresh tokens, cookies or private keys.

## Agent safety

MCP descriptions must make risk explicit. Prefer small typed tools to one omnipotent tool. AI reasoning is not a permission boundary; Lumic validates every operation independently.

A valid OAuth session does not imply permission to install arbitrary packages, modify the firewall, read secrets or execute shell commands. Every tool invocation is still evaluated against its Lumic capability scope/policy and approval rules.

## Lockout prevention

SSH is an independent break-glass path. Installing Lumic does not automatically disable root password authentication or rewrite SSH access.

If Lumic later offers SSH hardening, it must first verify the replacement authentication path with an independent connection and verify HTTPS/MCP health before offering to disable password login. Failed verification leaves the existing SSH configuration untouched.

## Installation supply chain

The installer runs as root and must fail closed. Before the public one-command bootstrap is declared production-ready, release artifacts require cryptographic integrity metadata and documented signed release provenance. Download to restrictive temporary files, verify before installation, install atomically and clean temporary artifacts on every exit path.

A checksum fetched from the same compromised origin is useful for corruption detection but is not by itself a complete supply-chain trust mechanism.