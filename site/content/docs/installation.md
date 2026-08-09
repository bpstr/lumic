+++
title = "Installation"
description = "From a fresh VPS to a Lumic Control Center node."
weight = 20
[extra]
kicker = "START"
status = "foundation"
+++

Lumic Control Center (Lumic) targets fresh Linux VPS instances first. Debian and Ubuntu are the initial supported OS family.

## One-line remote install

The canonical public installer is served directly from [lumic.cc](https://lumic.cc):

```bash
ssh root@server 'curl -fsSL https://lumic.cc/install.sh | sh'
```

When already connected to the server:

```bash
curl -fsSL https://lumic.cc/install.sh | sudo sh
```

`https://lumic.cc/install.sh` is published from the repository's root `install.sh` during the documentation deployment, so the website and repository always use the same installer source. `https://lumic.cc/install` is kept as a compatibility alias.

The installer detects the operating system and architecture, selects the requested release channel, downloads the matching Lumic binary, verifies that the binary can report its version, and installs it to `/usr/local/bin/lumic` by default. The daemon/system-service bootstrap continues to evolve with the MVP.

## Channels

Lumic has two release channels:

- **stable** — conservative production releases;
- **nightly** — the latest gated build from main.

The installer defaults to stable. During pre-release testing, use the nightly channel explicitly:

```bash
curl -fsSL https://lumic.cc/install.sh | sudo env LUMIC_CHANNEL=nightly sh
```

A node never silently changes channels.

## Verify the installer before running it

For a first live VPS test, it is reasonable to inspect the script first:

```bash
curl -fsSL https://lumic.cc/install.sh
```

Then install and verify:

```bash
lumic version
lumic status
```

## Supported systems

The installer currently accepts Debian and Ubuntu on x86_64 and ARM64/aarch64. A release asset for the selected architecture must exist in the chosen stable or nightly GitHub release channel.

Additional distributions and architectures require automated detection and installation coverage before being documented as supported.
