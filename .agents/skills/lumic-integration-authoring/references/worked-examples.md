# Worked Lumic Integration Examples

These examples show the shape of a correct change. Inspect current symbols and nearby tests before editing: paths and trait methods can evolve while the architectural boundaries remain stable.

Current implementation landmarks include `lumic_core::software::SOFTWARE_CATALOG`, `lumic_core::package::PackagePolicy`, `lumic_platform::runtime::component_package`, `lumic_platform::service_driver::{ServiceDriver, ServiceDriverRegistry}`, `lumic_platform::artifact::ArtifactManager`, `lumic_platform::app_process::ApplicationProcessManager`, `lumic_core::binding::BindingGraph`, and `lumic_core::pipeline::PipelineAction`. Use the knowledge graph to find their current callers and tests.

## 1. Add a simple apt-backed package

Suppose Lumic should install `jq` as a reviewed native package.

1. Classify it as a **Package**, not a managed service. It has no independent Lumic lifecycle beyond native package detection/install/update/remove.
2. Add trusted catalog/policy metadata using the current software/package catalog. Use a stable Lumic ID and the exact Debian/Ubuntu package mapping.
3. Ensure the package name passes `PackageName` validation and the allow/deny policy. Do not accept a raw package string from an adapter and do not concatenate an apt command.
4. Reuse the apt package manager's typed detect/plan/install path. The plan should report the candidate version and be a no-op if the required version/state is already present.
5. Persist a Package resource with its detected version and ownership only after verification.
6. Expose it through the existing package/software application service. Any CLI/UI/MCP changes should delegate there.

Tests should cover policy acceptance/rejection, package-name validation, absent/present/upgrade planning, dry-run purity, repeated install, audit/state ordering, and an Ubuntu host test if the package is claimed as supported.

Do not add a `jq` driver, a new command runner, or a managed-service definition.

## 2. Add a PHP extension component

Suppose a supported application needs PHP `imagick`.

1. Classify it as a **Component** owned by a versioned PHP Runtime.
2. Add `imagick` to the reviewed PHP component catalog/mapping so a PHP version such as `8.3` resolves deterministically to the native package such as `php8.3-imagick` on a supported platform.
3. Validate both the PHP version and component ID before resolving a package. Keep this mapping in the runtime/platform integration, not in the application recipe.
4. Let the runtime manager install the version-qualified package through the apt primitive and record the Component resource with a parent/owner edge to the PHP runtime.
5. Detect the extension through the appropriate typed runtime probe, not just apt state, when activation can differ from installation.
6. Reconcile additions/removals using the runtime's desired component set. Refuse removal if an application declares a dependency that would be broken.

Tests should cover supported and unknown components, version-qualified package resolution, repeated reconciliation, activation detection, parent ownership, reverse-dependency removal refusal, and supported-host behavior.

Do not teach WordPress, Laravel, or another application how to construct PHP package names.

## 3. Add a managed service such as Redis or Meilisearch

Use the existing Redis and Meilisearch definitions and drivers as nearby references.

1. Add a service TOML definition with stable ID/version, category, driver ID, instance policy, capabilities, configuration schema, outputs, and supported platform package/unit mappings.
2. Mark secrets with the Secret configuration type and `sensitive = true`. Give fields precise constraints and apply behaviors: for example, a port may require restart while a tunable may allow reload if the provider supports it.
3. Implement a narrow `ServiceDriver` for provider behavior: defaults, cross-field validation, paths, deterministic managed config files, typed health probe, logs/unit metadata, output discovery, backup/restore support, service-resource behavior, and upgrade rules as applicable.
4. Register the driver in the service driver registry. Catalog validation should reject the definition if registration is missing.
5. Reuse generic service orchestration for detection, plan/dry-run, apt operations, file backup/write, systemd reload/start/restart, health, rollback, audit, and state commit.
6. Publish typed outputs after health succeeds. Redis might publish a `connection` output with capability `cache.redis`; Meilisearch might publish an HTTP endpoint plus a sensitive credential reference.

A healthy second apply should plan no mutation. A changed config should calculate its declared reload/restart/recreate behavior. A failed health check should restore the previous config and service state when safe.

Tests should include catalog/driver registration, schema and cross-field validation, rendered config golden cases, health arguments, secret redaction, install/configure/update/remove pipelines, rollback, logs, outputs, bindings, reverse dependencies, and real host installation on every claimed OS.

Do not add `match provider { Redis => ..., Meilisearch => ... }` to service or application orchestration.

## 4. Add a verified-binary service such as an artifact-backed Typesense

At the time this skill was authored, the Typesense catalog platform mapping was apt-backed. Re-inspect the current definition first. Treat changing it to an artifact distribution as one coherent provider/platform migration—never maintain two competing installation routes accidentally.

1. Keep Typesense classified as a **Managed Service**. Model the downloaded release as its **Artifact** input.
2. Define an immutable artifact identity containing version, HTTPS source URL, and an audited SHA-256 digest. Select the artifact for the detected architecture and supported OS through trusted metadata or driver logic.
3. Plan an `EnsureArtifact` action and call the existing artifact manager. It should lock by artifact identity, download to a private temporary file, verify the digest, fsync, atomically move into the cache, and reuse a verified cached artifact on retry.
4. Use typed platform primitives to ensure the system user/group, data/config directories, permissions, managed config, binary installation/symlink, and systemd unit. If a missing primitive is needed, add a narrow one.
5. Preserve the previous binary or release pointer until the new version passes provider validation and health. On failure, restore it and restart the prior healthy version when safe.
6. Publish the HTTP endpoint and API credential as typed outputs; the credential output must contain a secret reference, not the key.
7. Encode upgrade compatibility in the driver, including data-format prerequisites and prohibited major-version jumps.

Tests should cover URL/digest/architecture validation, checksum mismatch, cache reuse, interrupted download, atomic activation, permissions, health failure rollback, upgrade compatibility, secret output redaction, second-apply no-op, and host-level systemd/HTTP health behavior.

Never use `curl | sh`, execute a downloaded installer, skip checksum verification, or embed download logic in an application recipe.

## 5. Add an application process such as Laravel Horizon

Horizon is an **Application Process**, not a managed infrastructure service.

1. Add a process to the application's desired composition using the existing `ApplicationProcess` contract with a stable name, Worker kind, enabled state, and an argument vector equivalent to `php artisan horizon`.
2. Resolve the executable/working context from the selected PHP Runtime and application release outputs where the current contract supports it. Do not have the Laravel integration search PATH or construct PHP package names independently.
3. Let the application process manager render the systemd service deterministically, write it atomically, reload systemd, and converge enable/start state.
4. Make the process owned by the application resource. Record its runtime and service bindings, such as the Redis connection consumed by Horizon.
5. Use journal/systemd primitives for status and logs. Removal should disable/remove the unit only after reverse dependencies are clear.

For Laravel's scheduler, create a separate **Application Schedule** with a backend-neutral calendar or interval expression, missed-run policy, and jitter. Let the platform map it to a systemd timer; do not store a crontab line or shell command.

Tests should cover deterministic unit/timer rendering, separated arguments, runtime and Redis bindings, no-op reapply, command/config change reconciliation, unit failure, logs, ownership, and uninstall boundaries.

Do not add Horizon to the managed-service driver registry or add Horizon branches to generic process orchestration.

## 6. Add an application integration bound to Redis or search

Suppose an application can use Redis for caching and either Meilisearch or Typesense for search.

1. In the Application definition, declare capability requirements with stable consumer roles, such as `cache.redis` for `cache` and a search capability appropriate to the application's supported contract. Mark genuinely optional integrations optional.
2. Ensure each provider definition publishes the required typed outputs: for example, Redis `connection`, or search `http` plus a sensitive API credential reference.
3. Use the generic capability/provider selection and binding service to connect producer outputs to application inputs. Persist bindings and validate the graph.
4. Translate bound values into application configuration through a small application integration/config-rendering contract. The translator understands the application's keys; it does not install or configure the provider.
5. When a credential is required, carry a secret reference through the binding and resolve it only while writing the protected application configuration or invoking the application setup command.
6. Include bindings in plans and removal checks. Refuse to remove the provider or output while the application consumes it; provide an explicit detach/rebind plan.

Tests should run the same generic binding path against more than one compatible provider, verify optional/required behavior, reject missing or sensitive/plaintext outputs, detect duplicate inputs and cycles, enforce reverse dependencies, redact plans/events, and reconcile a provider endpoint change.

Do not extend application orchestration with branches such as `if provider == "typesense"`. Provider selection is driven by capabilities and outputs; application-specific translation is isolated behind its integration contract.
