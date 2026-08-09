# Secure remote bootstrap and MCP enrollment

Lumic should remove routine VPS credential setup without weakening the trust boundary.

The target fresh-server experience is:

```bash
curl -fsSL https://lumic.cc/bootstrap | sh -s -- root@203.0.113.42
```

The operator starts with only the VPS provider's IP address, root account and password. Lumic is not installed locally. The local bootstrap is an ephemeral shell helper which delegates password handling to the system OpenSSH client, installs Lumic remotely, establishes the node's trusted HTTPS management endpoint and enrolls the local operator/client through a one-time grant.

This document is a security contract. Do not trade these invariants away for a shorter demo.

## Security goals

1. The root password is never read, stored, forwarded, logged or parsed by Lumic.
2. The first SSH login is the only place the VPS password is required.
3. A bootstrap credential is single-use, high entropy, short lived and enrollment-only.
4. Bootstrap credentials never become general API or MCP bearer tokens.
5. Remote MCP is exposed only over authenticated, publicly trusted TLS.
6. Long-lived MCP authentication follows the current MCP authorization standard; Lumic must not invent a proprietary password flow.
7. MCP clients receive scoped credentials, not root-equivalent authority.
8. The daemon is not exposed directly to the Internet; public HTTP terminates through the managed HTTPS edge.
9. SSH remains an independent break-glass path. Lumic must not lock the operator out while hardening access.
10. Every security-sensitive transition is auditable without logging secrets.

## Trust boundaries

A fresh VPS has three separate trust decisions:

```text
VPS provider credentials
        |
        v
first SSH connection
        |
        v
Lumic installation + release verification
        |
        v
trusted HTTPS node identity
        |
        v
one-time enrollment
        |
        v
OAuth-scoped UI / MCP clients
```

Do not collapse these into one reusable token.

### First SSH connection

The bootstrap helper MUST use the user's system `ssh` executable and allow OpenSSH itself to read the password from the controlling TTY.

Never implement or recommend:

- `sshpass`;
- `expect` password automation;
- a `--password` argument;
- `LUMIC_ROOT_PASSWORD` or similar environment variables;
- piping a password into stdin;
- storing the password in a temporary file;
- disabling host-key verification with `StrictHostKeyChecking=no`.

The default OpenSSH host-key policy remains in force. On a first connection this is normally trust-on-first-use unless the VPS provider supplies a host fingerprint. A future bootstrap option may accept a provider-supplied fingerprint and pin it before authentication.

This first-host-key decision is an irreducible trust boundary. Lumic must document it rather than silently weakening SSH to eliminate one prompt.

## Canonical bootstrap protocol

The intended flow is:

```text
local bootstrap shell
        |
        | ssh root@IP
        | password handled only by OpenSSH
        v
remote install
        |
        | verify release
        | start Lumic
        | provision HTTPS
        v
issue one-time enrollment grant
        |
        | returned only through encrypted SSH channel
        v
local bootstrap waits for trusted HTTPS
        |
        | no --insecure / no certificate bypass
        v
POST bootstrap exchange
        |
        v
enroll first owner device/session
        |
        v
configure MCP URL
        |
        v
client performs standard OAuth authorization
```

The bootstrap helper is not a Lumic client runtime. It exits after enrollment.

## One-time enrollment grant

The server generates the grant after Lumic has been installed successfully.

Requirements:

- at least 256 bits from the operating system CSPRNG;
- opaque random value; do not derive it from the root password, IP, hostname, time or machine ID;
- maximum lifetime: 120 seconds for the normal bootstrap flow;
- single use;
- purpose-bound to `initial-owner-enrollment`;
- bound to the node identity that issued it;
- only one active initial-owner grant per fresh node;
- server stores only a cryptographic digest of the secret;
- secret is removed from memory/state as soon as practical after exchange;
- consumption is atomic: a concurrent replay cannot win a second time;
- invalid/expired/replayed grants return indistinguishable public errors;
- exchange endpoint is rate limited even though the secret has high entropy.

The grant MUST NOT be:

- accepted by `/mcp`;
- accepted as an API bearer token;
- written to normal logs or audit payloads;
- embedded in a URL query string;
- persisted by the local bootstrap helper;
- passed as a command-line argument or environment variable to another process.

Transfer it from the remote installer to the local bootstrap over the already-authenticated SSH stream. Keep it in process memory only until the HTTPS exchange completes.

## HTTPS and IP-only servers

Lumic must support a node whose only public identity is an IP address.

Target endpoints:

```text
https://203.0.113.42/
https://203.0.113.42/mcp
```

Requirements:

- acquire and automatically renew a publicly trusted certificate valid for the IP address;
- verify the certificate chain and IP SAN normally;
- never ask users to install a Lumic CA for the default public-VPS path;
- never fall back to a self-signed certificate for remote MCP;
- never use `curl -k`, `--insecure` or equivalent in bootstrap;
- if trusted TLS cannot be provisioned, keep remote MCP closed and provide the SSH/stdio fallback instead;
- only public port 443 is required for UI/API/MCP; internal Lumic services bind to loopback or a Unix socket;
- certificate renewal is automatic and monitored because short-lived IP certificates require reliable renewal.

Domains later pointed at the VPS are application identities, not the Lumic node identity. The IP management virtual host must coexist with application virtual hosts.

## Bootstrap exchange endpoint

The enrollment endpoint exists only to convert the SSH-established grant into the first owner enrollment.

Suggested shape:

```text
POST /bootstrap/v1/exchange
```

Requirements:

- HTTPS only;
- no redirects;
- bootstrap secret carried in a request body or dedicated authorization header, never the URL;
- request body/header redaction at every logging layer;
- strict request-size limit;
- strict content type;
- no wildcard CORS;
- rate limiting by node and source;
- atomic grant consumption before credential issuance;
- endpoint disabled when no valid bootstrap grant exists;
- initial-owner enrollment permanently closes after the first owner is established;
- reopening enrollment later requires an already-authenticated privileged action or local root recovery.

A bootstrap grant proves control of the initial root SSH session. It does not itself become a normal user session.

## Owner and browser enrollment

The first exchange establishes the node's first owner identity/session. Avoid asking the operator to create another bootstrap password when control was already proven through root SSH.

Browser handoff must use a separate one-time web ticket if a browser is required. The ticket must be short-lived, single-use and safe if it remains in browser history after consumption. Prefer POST-based handoff where practical; if a URL ticket is necessary, set strict `Referrer-Policy`, consume immediately and make replay useless.

Session requirements:

- secure, HttpOnly cookies;
- SameSite protection appropriate to the OAuth flow;
- CSRF protection for state-changing browser operations;
- session rotation at privilege/authentication transitions;
- explicit device/session listing and revocation;
- no secrets in URLs, HTML logs or analytics.

## MCP authentication

Remote MCP uses the current MCP authorization model over Streamable HTTP. The default must be OAuth-style client authorization rather than a static administrator token copied into configuration files.

The bootstrap helper may:

1. detect installed MCP clients;
2. register only the Lumic MCP URL;
3. launch the client's normal OAuth login flow when the client provides a stable supported command.

It must not reverse-engineer private credential stores or write long-lived OAuth tokens itself.

Recommended token policy:

- short-lived access tokens;
- refresh-token rotation;
- refresh-token reuse detection where supported;
- per-client/device credentials;
- explicit revocation;
- audience/resource binding to the Lumic node;
- least-privilege scopes;
- authorization code + PKCE for interactive clients;
- no implicit flow;
- no password grant;
- no root password reuse.

Representative scopes:

```text
server.read
package.read
package.install.allowed
service.restart
application.deploy
database.backup
system.exec
```

`system.exec` remains disabled by default. Authentication is not authorization: every MCP capability still passes through Lumic policy.

## MCP exposure

The public MCP endpoint should be:

```text
https://IP/mcp
```

Conceptually:

```text
Internet :443
    |
    v
managed TLS / HTTP edge
    |
    +--> UI/API
    |
    +--> /mcp
            |
            v
        MCP adapter
            |
            v
      application/core
            |
            v
          policy
            |
            v
      privileged adapters
```

Do not bind the privileged daemon or internal MCP listener directly to a public interface.

## Optional SSH hardening

After HTTPS enrollment succeeds, Lumic may offer to harden SSH, but this is optional and must be transactional.

Safe sequence:

1. detect an existing local SSH key or create one only with explicit user consent;
2. install the public key;
3. verify a second independent SSH login succeeds;
4. verify Lumic HTTPS/MCP access remains healthy;
5. only then offer to disable root password authentication;
6. retain rollback/recovery instructions.

Never disable password authentication before the replacement path has been verified. Never modify SSH access merely because Lumic was installed.

## Release/install supply-chain requirements

The remote installer is privileged code. Treat installation integrity as part of the security boundary.

Before the public bootstrap flow is declared stable:

- release artifacts must have cryptographic checksums;
- release metadata/artifacts should be signed using a documented release identity;
- installer must fail closed on verification errors;
- stable and nightly channels must remain distinguishable;
- redirects must remain HTTPS;
- downloaded binaries are written to a temporary path, verified, then atomically installed;
- temporary artifacts use restrictive permissions and are always cleaned up;
- installer must not source remote shell fragments beyond the explicitly fetched installer itself.

A checksum fetched from the same compromised origin is not a complete supply-chain signature. Add signed release provenance before treating unattended bootstrap as hardened production installation.

## Logging and audit

Security events should be auditable:

```text
bootstrap.started
bootstrap.install_verified
bootstrap.grant_issued
bootstrap.grant_consumed
bootstrap.grant_expired
owner.enrolled
mcp.client_authorized
mcp.client_revoked
ssh.hardening_planned
ssh.hardening_applied
```

Audit records may contain node ID, actor/client ID, source interface, timestamps, result and correlation ID. They must never contain:

- root passwords;
- bootstrap secrets;
- access tokens;
- refresh tokens;
- session cookies;
- private keys.

## Failure behavior

Fail closed.

- SSH failure: make no remote change.
- release verification failure: do not install.
- daemon start failure: do not issue a grant.
- TLS provisioning failure: do not expose unauthenticated/plaintext MCP.
- grant expiry: require a new grant over authenticated SSH/local root.
- OAuth failure: leave the node installed but unpaired; provide a safe retry path.
- SSH hardening verification failure: leave the existing SSH configuration untouched.

The installer must be idempotent enough that a failed bootstrap can be retried safely.

## Threats that tests must cover

At minimum:

- malicious destination beginning with `-` or containing shell metacharacters;
- bootstrap token replay;
- concurrent exchange race;
- expired grant;
- wrong-purpose grant;
- token appearing in logs/error output;
- HTTP downgrade/redirect attempts;
- untrusted/self-signed certificate;
- malformed Host/SNI/request sizes;
- CSRF/state mismatch in browser authorization;
- OAuth redirect URI abuse;
- refresh-token reuse/revocation;
- unauthorized scope escalation;
- MCP mutation denied by policy despite valid authentication;
- failed SSH-key verification during optional hardening;
- interrupted bootstrap and safe retry.

## Product rule

The simple UX is a security outcome, not a security exception:

```text
IP + root password
        |
        | one local bootstrap command
        v
Lumic installed
trusted HTTPS ready
owner enrolled
MCP endpoint registered
        |
        v
normal work no longer needs root/password setup
```

If an implementation requires the user to copy long-lived tokens, disable TLS verification, install Lumic locally, hand-edit SSH keys, or grant an agent a generic root shell, it has missed the intended design.
