+++
title = "Configuration"
description = "Machine, application and infrastructure intent should be exportable and reproducible."
weight = 110
[extra]
kicker = "REFERENCE"
status = "planned contract"
+++

Anything configured through UI or MCP should ideally have a structured representation suitable for backup, inspection and reproduction.

Configuration layers are expected to include node-level Lumic configuration, application definitions and eventually multi-node environment/role definitions.

Example direction:

```yaml
runtimes:
  php:
    version: "8.4"
    extensions: [redis, intl]

services:
  redis: {}
  postgres: {}

applications:
  api:
    runtime: php
    domain: api.example.com
```

The schema is not yet stable. Do not treat examples on this page as a compatibility promise until the configuration subsystem is implemented and versioned.
