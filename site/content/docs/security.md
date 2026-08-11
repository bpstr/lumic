+++
title = "Security model"
description = "Policy, safe native execution, and auditable host mutations."
weight = 90
[extra]
kicker = "SECURITY"
status = "foundation"
+++

Lumic operates close to root privileges. Security is therefore an architecture boundary, not a feature to add after provisioning works.

## Safe native execution

Lumic prefers direct executable invocation with separated argument arrays. Untrusted values must not be interpolated into `sh -c`.

The Phase 0 internal process runner enforces a per-process timeout, consumes stdout/stderr without deadlocking, bounds retained output, reports truncation and captures exit code/signal metadata. It is not exposed through CLI or MCP.

## Capability policy

Interfaces receive Lumic capabilities rather than a generic shell. Policy can allow or deny operations and constrain arguments such as package identifiers.

## Package allowlisting

Using apt through Lumic does not mean exposing arbitrary apt execution. A package installation request is validated against policy, resolved through the OS adapter and recorded as an operation. Application package requirements include a bounded reason and acquire an explicit trust source only after policy review; syntactic validity never grants installation authority.

## Plan and audit

Mutating operations should consider preconditions, idempotency, plan/dry-run support, failure behavior, before/after state, rollback/recovery and audit metadata.

Managed database and bootstrap credentials are generated from the operating system random source, stored below private Lumic state with mode `0600`, and represented externally only by opaque `secret://` references. Database credentials are passed to native tools over stdin. Every built-in managed-service configuration is loopback-only and accepts only bounded provider settings. Failed configuration health checks restore the previous native file or remove a newly created file. OpenSearch disables its security plugin only for the enforced loopback-only single-node configuration; Lumic rejects direct non-loopback exposure.

Certificate issuance accepts only Lumic's registered Certbot/Let's Encrypt provider and validated DNS names. Commands use separated arguments. Certificate resource state contains certificate and private-key paths, but never private-key contents or the contact email. Attaching a certificate is a locked, atomic nginx configuration change; Lumic validates nginx before reload and restores the previous configuration on failure.

The UI binds only to loopback. Its one-time admin token is stored as a digest, sessions are short-lived and in-memory, cookies are HttpOnly/SameSite=Strict, mutation forms are CSRF protected, and responses include restrictive browser headers. Remote UI access requires an operator-provided authenticated TLS reverse proxy or an SSH tunnel.

## Container boundaries

Docker socket mounts and privileged host mounts are treated as host-level power. Containerized Lumic must never imply host control unless those privileges have been deliberately granted and surfaced to the operator.
