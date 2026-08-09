+++
title = "Installation"
description = "From a fresh VPS to a Lumic node."
weight = 20
[extra]
kicker = "START"
status = "foundation"
+++

Lumic targets fresh Linux VPS instances first. Debian and Ubuntu are the initial supported OS family.

## One-line remote install

The intended onboarding command is:

```bash
ssh root@server 'curl -fsSL https://lumic.cc/install | sh'
```

The installer is responsible for detecting the operating system and architecture, installing the correct Lumic binary, establishing Lumic directories and eventually configuring the `lumicd` system service.

## Local install

When already connected to the server:

```bash
curl -fsSL https://lumic.cc/install | sudo sh
```

## Channels

Lumic has two release channels:

- **stable** — conservative production releases;
- **nightly** — the latest gated build from main.

Nightly is a deliberate early-Lumic product feature. A node never silently changes channels.

## After installation

The target experience is:

```bash
lumic status
```

which reports OS, architecture, node identity, resources, Lumic version/channel and managed capabilities.

## Supported systems

Initial support is intentionally narrow: Debian and Ubuntu on x86_64. Additional distributions and architectures require automated detection and installation coverage before being documented as supported.
