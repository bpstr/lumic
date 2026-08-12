+++
title = "lumic.toml"
description = "The versioned repository-to-server application and deployment contract."
weight = 45
[extra]
kicker = "APPLICATIONS"
status = "schema version 2 implemented"
+++

`lumic.toml` is Lumic Control Center's repository-owned application definition. Put it at the repository root. Lumic validates it before changing server state and resolves it into the same typed requirements, runtime components, processes, schedules, deployment, health, and managed-service resources used by built-in applications.

The file describes application intent, not privileged implementation. Unknown fields are rejected. Commands are direct argument arrays. A repository cannot declare host file paths, configuration transports, arbitrary packages, shell interpreters, or root commands; those remain in reviewed built-in definitions and Rust drivers.

## Schema version 2 example

```toml
schema_version = 2
output = "dist"

[application]
name = "billing-api"

[source]
branch = "main"
subdirectory = "apps/api"

[runtime]
type = "node"
version = 24
package_manager = "pnpm"

[[requirements]]
role = "database"
capability = "database.postgresql"

[requirements.resources.database]
name = "billing"

[requirements.resources.user]
name = "billing"

[[requirements]]
role = "cache"
capability = "cache.redis"

[processes.web]
type = "web"
command = ["node", "dist/server.js"]
port = 3100

[processes.queue]
type = "worker"
command = ["node", "dist/worker.js"]
instances = 2
restart = "always"

[[schedules]]
name = "cleanup"
schedule = "0 2 * * *"
command = ["node", "dist/cleanup.js"]

[deployment]
build = ["pnpm", "run", "build"]
migrate = ["pnpm", "prisma", "migrate", "deploy"]
after = [["node", "dist/warm-cache.js"]]
retain_releases = 7

[shared]
directories = ["storage", "uploads"]
files = [".env"]

[health]
path = "/health"
port = 3100
expect = 200
```

PHP runtime components are first-class desired state:

```toml
public = "public"

[runtime]
type = "php"
version = "8.4"
extensions = ["curl", "intl", "mbstring", "mysqli", "xml", "zip"]
```

`runtime.type` is `static`, `node`, or `php`. Node supports majors `20`, `22`, and `24`; PHP supports `8.1` through `8.4`. Apply installs only trusted component packages and verifies the actual runtime. Deployment verifies the same intent without mutating it.

Requirements resolve capabilities through the trusted built-in catalog. A unique provider may be inferred, or `provider` may select a reviewed provider that actually exposes the capability. `configuration` and `resources` are typed requests; relational `database` and `user` resources must match the application's managed-service binding before deployment. Optional requirements may remain unbound. Web requirements are fulfilled by Lumic's owned web-host resource rather than treated as managed-service bindings.

Processes with `type = "worker"` become owned systemd services. One Node `web` process may declare a command and primary port for blue/green handoff. Schedules become owned timers. Build, migration, and lifecycle hooks remain bounded argv arrays and are never shell strings.

## Inspect, plan, apply, deploy

```bash
lumic app manifest inspect --repository-root .
lumic app manifest plan billing-api --repository-root .
lumic app manifest apply billing-api --repository-root .
lumic app deploy billing-api
```

Every deployment reads the manifest from the exact checked-out commit and requires it to equal the reviewed, applied contract. A changed or removed contract blocks deployment until its plan is applied. MCP exposes the same separation through `application_manifest_inspect`, `application_manifest_plan`, and approved `application_manifest_apply`.

## Migration from schema version 1

Lumic continues to read `lumic.yaml` schema version 1 during the migration window. New definitions must use `lumic.toml` schema version 2. The two files cannot coexist because that would make repository intent ambiguous. Migrate the YAML fields into the TOML application/runtime/requirements/processes/schedules/deployment sections, apply the new manifest plan, and then remove `lumic.yaml`.

Both files must be non-symlink regular files no larger than 256 KiB. Repository paths must be normalized relative paths without parent traversal. Service bindings and runtime identity are validated before deployment work begins. A failed deployment follows the normal recovery path: Lumic restores the previous release and Node upstream when available.
