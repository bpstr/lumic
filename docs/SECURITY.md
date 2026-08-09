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
