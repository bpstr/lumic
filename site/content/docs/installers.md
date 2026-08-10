+++
title = "Software installers"
description = "Inspect, plan, and install Lumic's default supported software catalog."
weight = 44
status = "native package setup implemented"
+++

Lumic includes a fixed software catalog for WordPress prerequisites, PHP, MySQL, PostgreSQL,
Redis, Typesense, Meilisearch, Valkey, RabbitMQ, MinIO, OpenSearch, Memcached, MongoDB,
ClickHouse, Prometheus, Grafana, Loki, nginx, Apache, Node.js, and NVM. The authenticated operator UI
shows these entries under **Installers**, including the installed and apt
candidate version of every required package.

Each entry follows STATUS → PLAN → APPLY:

1. Status calls `apt-cache policy` and `dpkg-query` through Lumic's typed package
   adapter. It reports installed and candidate versions without changing the host.
2. The confirmation screen resolves the exact fixed package set, risks,
   preconditions, validation, and recovery guidance.
3. For distribution-backed installers, setup refreshes apt metadata when required,
   re-checks every candidate, and installs only packages authorized by Lumic's
   built-in policy. Package names are passed to apt as arguments, never
   interpolated into a shell command. Apt runs noninteractively, retains existing
   configuration files when package defaults conflict, and bounds package-lock waits.

The UI inspects catalog entries concurrently and bounds each entry's status probe.
A slow or temporarily blocked native package query is shown on its own installer card
instead of preventing the complete Installers page from loading.

The catalog currently resolves to these native packages:

| Installer | Required packages |
| --- | --- |
| WordPress prerequisites | `php-fpm`, `php-mysql`, `default-mysql-server`, `nginx` |
| PHP | `php-fpm`, `php-cli`, `composer` |
| Default MySQL-compatible server | `default-mysql-server` |
| PostgreSQL | `postgresql` |
| Redis | `redis-server` |
| Typesense | `typesense-server` |
| Meilisearch | `meilisearch` |
| Valkey | `valkey-server` |
| RabbitMQ | `rabbitmq-server` |
| MinIO | `minio` |
| OpenSearch | `opensearch` |
| Memcached | `memcached` |
| MongoDB | `mongodb-org` |
| ClickHouse | `clickhouse-server` |
| Prometheus | `prometheus` |
| Grafana | `grafana` |
| Loki | `loki` |
| nginx | `nginx` |
| Apache | `apache2` |
| Node.js | `nodejs`, `npm` |
| NVM | `git`, `curl` prerequisites, then pinned `nvm-sh/nvm` Git checkout |

Typesense, Meilisearch, Valkey, MinIO, OpenSearch, MongoDB, ClickHouse, Grafana, and Loki require their package to have a candidate in an apt
source already configured and trusted by the operator. Lumic does not silently
add third-party repositories or keys. Status marks these installers as
**Repository required**, plan names the missing packages as a precondition, and
setup refuses to start any apt mutation until every missing package has a
candidate.

Once that prerequisite is satisfied, every service listed above can also be installed through the managed-service commands. That path adds loopback-only provider configuration, systemd lifecycle, and provider health validation. Providers that require bootstrap credentials receive generated private secrets. The Installers page itself remains package setup only.

PHP, WordPress prerequisites, the default MySQL-compatible server, PostgreSQL,
Redis, RabbitMQ, Memcached, Prometheus, nginx, Apache, Node.js, and the NVM prerequisites are distribution-backed.
If their candidates are absent on a fresh server, status reports **Package index
refresh needed** instead of incorrectly asking for another repository. The UI
keeps the installer actionable as **Refresh index and set up**. Apply runs an
audited `apt-get update`, checks candidates again, and then installs them. If a
candidate is still absent, Lumic reports the supported Debian or Ubuntu source
problem without passing an impossible package name to `apt-get install`.

The `default-mysql-server` metapackage is available on both supported distribution
families. It installs Oracle MySQL on Ubuntu and MariaDB on Debian. Lumic labels
this distinction explicitly instead of claiming that Debian's default is Oracle
MySQL.

The WordPress entry does not request a native package named `wordpress`, because
that package is not consistently available across Lumic's supported Debian and
Ubuntu repositories. It installs only the native hosting prerequisites. Full
site deployment is available separately through the reviewed `wordpress@1.0.0`
application recipe, which makes its database, credential, domain, optional TLS,
artifact, and removal behavior explicit at the recipe plan/apply boundary. This
separation prevents a package button from hiding application configuration or
credential mutations.

Node.js is system-scoped and reports the versions supplied by the operator's
configured apt sources. NVM is intentionally user-scoped because upstream NVM
is loaded by a user's shell. Its UI and MCP requests require an existing Linux
account. Lumic installs the pinned upstream `v0.40.6` Git tag into that account's
`~/.nvm`, adds an identifiable activation block to `~/.profile`, and reports the
checked-out tag. It invokes `git`, `runuser`, and `tee` with separated arguments;
it does not pipe a downloaded installation script into a shell.

Agents use `software_catalog`, `software_status`, `software_plan_setup`, and
`software_setup`. The first three are read-only. `software_setup` requires the
MCP mutation process policy, a matching scope, and `approved=true` on the call.
Pass `user` to the status, plan, and setup tools when `software` is `nvm`.
