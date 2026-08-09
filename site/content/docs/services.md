+++
title = "Managed services"
description = "Install and operate server-side infrastructure without turning every package into a bespoke subsystem."
weight = 50
[extra]
kicker = "SERVICES"
status = "planned contract"
+++

Managed services represent long-lived server capabilities that deserve lifecycle awareness.

Candidate official services include PostgreSQL, MariaDB, Redis/Valkey, nginx, Typesense, Meilisearch, MinIO, RabbitMQ, NATS and Agnative.

A managed service adapter should understand:

- supported OS/version combinations;
- installation and detection;
- configuration location/schema;
- process or systemd unit;
- ports and network exposure;
- start, stop, restart and reload semantics;
- health checks;
- logs and relevant metrics;
- upgrades;
- backup/recovery integration;
- emitted events.

Lumic should use trusted native packages and service mechanisms whenever practical. Custom installers are justified only when the upstream distribution model requires them.
