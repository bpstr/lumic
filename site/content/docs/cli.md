+++
title = "CLI reference"
description = "The human console over the same typed capabilities used by UI and MCP."
weight = 100
[extra]
kicker = "REFERENCE"
status = "Epics A-E implemented"
+++

The CLI should stay predictable and map to Lumic's domain model.

Implemented nightly commands include:

```text
lumic app list
lumic app create <name> --domain <domain> --runtime static|php|node
lumic app credential import <name> <private-key-path>
lumic app repository set <app> <url> --branch main
lumic app provision <app> [--component intl] [--component zip]
lumic app health <app> --path /health --port 80
lumic app process worker <app> <name> -- php artisan queue:work
lumic app process schedule <app> <name> --on-calendar daily -- php task.php
lumic app plan <app>
lumic app deploy <app>
lumic app deployments <app>
lumic app rollback <app>
lumic app tls <app> --email admin@example.com
lumic app inspect <app>
lumic app delete <app>

lumic managed-service list
lumic managed-service detect postgresql|redis
lumic managed-service inspect <service>
lumic managed-service plan-install <service> postgresql|redis
lumic managed-service install <service> postgresql|redis [--dry-run]
lumic managed-service configure <service> [--bind 127.0.0.1] [--port <port>] [--setting key=value]
lumic managed-service start|stop|restart <service>
lumic managed-service update|remove <service> [--dry-run]
lumic managed-service logs <service> [--lines 100]
lumic managed-service declare-dependency <service> <dependency> --purpose <text>
lumic managed-service database-create <service> <database> [--owner <user>]
lumic managed-service user-create <service> <user>
lumic managed-service grant <service> <database> <user>
lumic managed-service backup <service> [--database <database>]
lumic managed-service backup-verify <backup-id>
lumic managed-service restore <service> <backup-id>
lumic managed-service attach <service> <app> --role <role> [--database <database>] [--user <user>]

lumic recipe catalog|list
lumic recipe inspect <app>
lumic recipe plan <recipe> <app> <domain> --repository <url> [--tls-email <email>] [--env NAME=value]
lumic recipe install <recipe> <app> <domain> --repository <url> [--tls-email <email>] [--env NAME=value]
lumic recipe update <app>
lumic recipe uninstall <app>

lumic server snapshot
lumic server user-create|user-delete <name>
lumic server group-create|group-delete <name>
lumic server group-add-member <group> <user>
lumic server permissions <path> <owner> <group> <octal-mode>
lumic server firewall-list
lumic server firewall-rule allow|deny <port> [tcp|udp] [--source any|IP|CIDR] [--remove]
lumic server listeners|mounts|timers|updates
lumic server processes [--limit 25]
lumic server process-signal <pid> terminate|kill|hangup
lumic server update-apply security|all
lumic server logs [--unit <unit>] [--priority err] [--since today] [--query <text>]
lumic server backup-schedule <id> <service> --on-calendar daily [--database <name>]
lumic server remediate-restart <unit>
lumic server remediate-terminate <pid>
lumic server remediate-journal --older-than-days <days>

lumic git host <repository> [--branch main]
lumic git mirror <mirror> <url> [--credential-reference <reference>]
lumic git trigger <repository> <application> [--branch main]

lumic environment secret-generate <reference>
lumic environment reference-set <application> <NAME> <reference>
lumic environment export <application> <environment> --tier production|staging|development --output <file>
lumic environment import <bundle> --target <application> --tier <tier> --domain <domain> [--env NAME=reference] [--service source=target]
lumic environment diff <source-bundle> <target-bundle>

lumic infrastructure init <node> --name <name> --role app [--role git]
lumic infrastructure enrollment --endpoint <https-url> --output <file>
lumic infrastructure register <peer-enrollment-file>
lumic infrastructure status
lumic infrastructure endpoint <id> --provider-node <node> --provider-kind <kind> --provider <id> --consumer-node <node> --consumer-kind <kind> --consumer <id> --protocol tcp --host <host> --port <port>
lumic infrastructure membership --kind worker|reverse_proxy --environment <environment> --application <app> --node <node>
lumic infrastructure coordinate <environment> --member node=application [--member node=application]
lumic infrastructure sign --target <trusted-node> --operation application.deploy|application.rollback --application <app> --output <file>
lumic infrastructure apply <signed-request-file>

lumic ui token rotate

lumic package search <name>
lumic package inspect <name>
lumic package install <name> [--dry-run]
lumic package remove <name> [--dry-run]
lumic package update-index
lumic package allowed

lumic events
lumic audit
lumic diagnose
lumic operations capture
lumic operations observe
lumic operations timeline [--entity <kind>] [--entity-id <id>] [--event-type <type>] [--since-ms <unix-ms>]
lumic operations incident [--entity-id <id>] [--since-ms <unix-ms>] [--until-ms <unix-ms>]
lumic operations provider-signal <event-type> <entity> <entity-id> --severity <level> --summary <text> [--payload <json>]
lumic operations webhook-plan|webhook-apply <id> <https-url> <secret-reference>
lumic operations subscribe <id> <destination> --event <event-type> [--event <event-type>]
lumic operations rule-plan|rule-apply <id> <event-type> <unit> [--entity-id <id>] [--cooldown-seconds 60] [--max-attempts 2]
lumic operations run-once
lumic operations deliveries
lumic operations rollback-configuration
lumic intelligence catalog
lumic intelligence fingerprint <application>
lumic intelligence config <application>
lumic intelligence graph <application>
lumic intelligence plan <application> [--integration laravel-redis@1] [--service <redis-id>]
lumic intelligence apply <application> [--integration laravel-redis@1] [--service <redis-id>]
lumic intelligence rollback <application> <snapshot-id>
lumic intelligence incident [--app <application>] [--since-ms <unix-ms>] [--until-ms <unix-ms>]
lumic intelligence analyze <destination> [--app <application>]
lumic service inspect nginx.service
lumic service restart nginx.service
lumic self-update apply
lumic self-update enable-nightly
```

Command implementations remain thin adapters over shared application and platform services rather than reimplementing host logic.

The host commands remain:

```text
lumic version
lumic status
lumic status --json
```

`lumic version` prints only the deterministic Cargo package version (`lumic <version>`). `lumic status` reads live Linux host facts and renders node identity, Debian/Ubuntu release, kernel, architecture, logical CPU count, memory and root-disk capacity. `--json` serializes the full typed fact model, including swap and root filesystem metadata, for automation.

`lumic diagnose` adds live load, uptime, high-memory processes, failed systemd units, listeners, mounts, timers and pending updates. Findings flag memory/load pressure, nearly-full filesystems, failed services and security updates with evidence. Service lifecycle commands accept validated unit names and map to direct `systemctl` arguments.

Package search is discovery only and never grants trust. Installation and removal require exact built-in policy entries, use direct argv invocation, are idempotent, and record events and audits. `LUMIC_STATE_DIR` and `LUMIC_APPS_ROOT` can relocate state for testing; production defaults are `/var/lib/lumic` and `/var/lib/lumic/apps`.

## Application deployment

`app provision` installs only packages in Lumic's explicit runtime/component catalog and writes an nginx site atomically. Lumic runs `nginx -t` before reload and restores the prior configuration if validation or reload fails. PHP currently supports `curl`, `intl`, `mbstring`, `xml`, and `zip` components. Node has a minimal package/build/proxy foundation; the two acceptance references are static and generic PHP Git applications.

Deployment is a separate plan/apply flow. `app plan` shows the intended source change, risks, preconditions, validation and recovery. `app deploy` mirrors Git, checks out an isolated release, runs Composer or npm when the corresponding manifest/lock exists, validates the runtime entry point, switches `current` atomically, then runs the configured local HTTP health check. A failed health check automatically restores the previous release. Deployment phase results, source commits and rollback state remain in history.

SSH keys are copied into Lumic's mode-`0600` credential store and application metadata retains only the named reference. Workers and schedules become validated systemd service/timer units; commands remain argv data and do not use `sh -c`. TLS uses Certbot's nginx integration after web provisioning and records certificate events.

## Nightly updates

`lumic self-update apply` downloads the x86_64 nightly artifact and its SHA-256 file, verifies the checksum, runs the candidate's version preflight, preserves the previous executable, atomically replaces Lumic, and performs a post-install check with automatic restoration on failure. `enable-nightly` installs a persistent daily systemd timer. Nightly release publishing includes the checksum asset. AArch64 release artifacts are not published yet.

Generic outbound webhooks and framework-specific recipe breadth are later work and are not part of this capability set. The implemented `static-git` recipe proves versioned validation, plan/install/update/uninstall, generated secret references, health/TLS composition and idempotency.

## Managed services and UI

Managed-service commands return structured JSON and compose approved apt packages, systemd, atomic provider configuration and provider health checks. PostgreSQL adds database/user/grant and local dump/restore primitives; Redis adds local snapshot/restore. Generated database passwords are never printed—the CLI returns only a private secret reference. `ui token rotate` is the exception for an operator credential: it prints the newly generated token once and stores only its digest.
