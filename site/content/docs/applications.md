+++
title = "Applications"
description = "Applications own runtime, Git, environment, web routing, workers, jobs, TLS and deployments."
weight = 40
[extra]
kicker = "APPLICATIONS"
status = "application lifecycle and intelligence implemented"
+++

In Lumic an application is more than an nginx site. It is the lifecycle boundary for deployable software.

The Lumic CLI persists application identity, domain, static/PHP/Node runtime, Git repository and branch, health configuration/state, worker/schedule definitions, typed managed-service references, nginx/TLS state, release retention, timestamps, and deployment history. It creates `releases/`, `shared/`, `repository/`, and an atomic `current` symlink under `/var/lib/lumic/apps/<name>`.

Managed MySQL/PostgreSQL/Redis resources can be attached with a semantic role plus optional database/user. Relational database attachments publish separate database and sensitive credential-reference bindings, require a recorded user grant, and support multiple roles per application. Application metadata keeps only the secret reference; it does not copy credential values. Portable environment bundles clone this typed application definition between nodes with an explicit target tier, domain and secret/service-reference transforms. Application intelligence now discovers deployed Laravel/dotenv evidence and safely wires the single reference Redis integration; broader framework/service combinations remain nightly catalog work.

See [Server intelligence](@/docs/server-intelligence.md) for fingerprint, plan/apply, rollback, dependency graph and incident workflows.

Exports never include secret values. Before import creates or updates an application, Lumic verifies that every final environment, service and repository credential reference exists in private state on the target node. `lumic environment diff` redacts sensitive references while still showing whether the source and target are configured differently.

Application environment values are deliberately not a general-purpose vault. `secret-set` accepts a single application key over stdin, encrypts it with authenticated encryption under a node-local mode-`0600` key, and stores only the typed reference in application state. `secret-rotate` replaces only application-owned values with fresh random material; `secret-delete` detaches the key and removes the value only when the application owns it. Ordinary UI and MCP application inspection masks references, environment comparison reports keys/configured state only, and no read capability returns plaintext.

Deployment resolves references only at the apply boundary. Pre-deploy, build, migration and post-deploy commands receive values through a scoped process environment; output is redacted against the resolved values. Persistent application units load a root-readable environment file materialized under `/run/lumic/application-environments`, so values are not embedded in unit files or persistent application state. A missing runtime file fails closed rather than starting a process without its required environment; redeploy rematerializes it.

Repository URLs must use HTTPS, SSH/Git scp syntax, or `file://`. HTTPS credentials embedded in URLs are rejected; metadata stores only an optional credential reference. `app credential import` copies a validated private key into the private state directory with mode `0600`; Git receives the resolved key path through a scoped process environment and status/audit output remains redacted.

An application can own:

- domain and web routing;
- runtime and runtime components;
- source repository;
- environment values and secrets;
- persistent/shared storage;
- database/service relationships;
- workers and scheduled jobs;
- TLS certificates;
- deployments, health checks and rollback history;
- application logs and events.

## Repository application intent with `lumic.toml`

Applications may include a versioned [`lumic.toml`](@/docs/lumic-toml.md) contract at the repository root. Schema version 2 uses the same capability requirements, typed service configuration, service resources, runtime extensions, processes, schedules, deployment, and health vocabulary as built-in application definitions. Lumic resolves capabilities through the trusted catalog; repository files cannot introduce privileged paths or arbitrary root commands. Legacy `lumic.yaml` schema version 1 remains readable during migration, but it cannot coexist with `lumic.toml`.

`lumic app manifest inspect` is read-only, `lumic app manifest plan` resolves changes and risks against the target application, and `lumic app manifest apply` is the approved state mutation. Every deployment also validates the manifest from the exact checked-out commit and uses its working/public directory and deployment behavior. Required services must already have matching typed application bindings; the manifest cannot silently install a package or introduce a secret.

## Runtimes

The reference deployment types are static repositories with a root `index.html`, and generic PHP repositories with a root `index.php`. PHP runs production Composer install flags when `composer.json` exists, with project plugins and lifecycle scripts disabled. PHP provisioning requires an explicit `8.1`, `8.2`, `8.3`, or `8.4` runtime version; the selected package version must be available from the node's configured Debian/Ubuntu apt repositories. Lumic installs version-qualified PHP-FPM, PHP CLI and extension packages plus Composer through its apt policy. The selected runtime publishes its deterministic FPM socket and CLI outputs; the web host binds to that exact runtime instead of scanning the host for an arbitrary socket.

Generic PHP applications also have a validated desired-state lifecycle contract. It covers the Lumic-managed application root, primary domain and optional `www` alias, repository, exact PHP runtime and components, policy-reviewed native package requirements, role-scoped database and credential references, TLS intent, workers, schedules, and HTTP health. Install, reconcile, update, and removal each produce an explicit human plan plus a typed, journalable pipeline. Each package requirement records why it is needed and the policy source from which trust was derived; a valid package name is not trusted by syntax alone. Update includes deployment of the configured repository; reconcile repairs desired state without implicitly selecting a new release.

Application creation persists a first-class application resource. Workers and schedules become owned resources that publish their systemd units and bind to the application; web hosts, PHP runtimes, databases, and certificates keep their existing explicit bindings. Removal plans detach application-owned certificate, process/schedule, database-binding, web-host, and root relationships in dependency order. They do not uninstall shared packages, runtimes, databases, or managed services. Pipeline execution takes the application resource lock and records progress in the authoritative resource state before native mutation. Catalog-driven CLI, UI, and MCP controls for this combined lifecycle are still planned; the current commands continue to use the same underlying application/platform operations.

nginx is installed and persisted independently as the owned `nginx.main` managed service. Each successfully activated site is an owned web-host resource bound to nginx and its application. Lumic writes and validates nginx configuration, enables nginx, and either starts an inactive service or reloads an active one. Static and Node runtime setup no longer installs nginx implicitly. The Node foundation installs Node, runs `npm ci --omit=dev --ignore-scripts` when a lockfile is present, requires `package.json`, and proxies the independently managed nginx service to port 3000; richer Node process configuration is deliberately deferred.

## TLS certificates

`lumic app tls <app> --email <contact>` installs the trusted Certbot packages, validates the application domain and contact email, verifies Certbot and nginx, and requests a named Let's Encrypt certificate only after the owned web host exists. The application domain and optional explicit `www` alias are the only requested names; wildcard issuance is not supported by this HTTP/nginx flow.

The certificate is persisted as `certificate.<application>` and bound to `nginx.web-host.<application>`. Lumic records certificate paths and lifecycle metadata, not private-key contents or the contact email. Certbot obtains the certificate with `certonly`; Lumic remains the nginx configuration owner. It writes the HTTPS listener and HTTP redirect atomically, runs `nginx -t`, reloads nginx, and commits state only after success. A failed attach restores the prior known-good configuration and removes a newly issued certificate where safe. The resource MCP interface can inspect the certificate and its bindings; provider-neutral renewal and detach mutations remain available through the internal lifecycle contract until dedicated adapter controls are added.

## Peripheral dependencies

Runtime components are first-class. The PHP catalog includes `curl`, `intl`, `mbstring`, `mysql`, `xml`, and `zip`; application definitions may use the native aliases `dom`, `exif`, `fileinfo`, `mysqli`, and `openssl`, which resolve to the reviewed XML, MySQL, or PHP core packages as appropriate. Lumic resolves each to the selected version-qualified native package (for example, `php8.3-intl`); unknown component names and unsupported versions are denied before apt runs.

## Safe activation and recovery

Each deployment records source, checkout, build, pre-activation, activation and health phases. Activation is an atomic symlink replacement. An enabled local HTTP health check retries within its configured readiness timeout. If it never returns the configured successful status range, Lumic restores the previous release, records `failed_rolled_back`, emits deployment events and retains the evidence in the audit trail. Manual rollback uses the same activation primitive.

nginx files use atomic sibling writes and retain a `.lumic-backup`. Lumic restores the previous file/link if validation, service activation, certificate attachment, or framework-state persistence fails, and reloads the restored known-good configuration where applicable. A web-host or certificate binding is committed only after native validation and activation succeed. Workers run direct argument vectors as systemd services with restart-on-failure. Scheduled-job intent supports calendar or interval timing, missed-run behavior and optional jitter without exposing a systemd-specific domain contract; the Linux adapter renders it as a timer. Disabled process definitions are stopped and disabled.

## Application recipes

Installatron-style recipes provide modern stack installation without hard-coding applications into core. The built-in WordPress definition declares PHP 8.3 and its required extensions, web capability, MySQL capability, database, and database-user resources. Its reduced Rust driver handles the pinned WordPress/WP-CLI artifacts and application-specific configuration and installation. The shared framework owns runtime/service/web composition, TLS, generated credentials, health, durable steps, convergence, rollback, and safe removal. Shared runtimes, packages, services and database data survive uninstall.

Recipes compose existing runtimes, components, services and setup actions and remain declarative wherever possible. Laravel, Drupal, Ghost, Forgejo and other self-hosted software remain catalog-expansion work.
