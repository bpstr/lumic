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

The installer defaults to stable. Select nightly explicitly during nightly testing:

```bash
curl -fsSL https://lumic.cc/install.sh | sudo env LUMIC_CHANNEL=nightly sh
```

A node never silently changes channels.

Stable installation resolves GitHub's latest stable release. Pin an exact immutable release when required:

```bash
curl -fsSL https://lumic.cc/install.sh | sudo env LUMIC_VERSION=1.0.0 sh
```

Release downloads include SHA-256 files and GitHub artifact attestations. The installer enforces the checksum; a downloaded binary can also be checked independently with `gh attestation verify <artifact> --repo bpstr/lumic`.

Installer inputs are environment variables: `LUMIC_CHANNEL` (`stable` or `nightly`), `LUMIC_VERSION` for an explicit stable version, `LUMIC_INSTALL_DIR`, `LUMIC_CONFIG_DIR`, and `LUMIC_STATE_DIR`. `LUMIC_INSTALL_BINARY` and optional `LUMIC_INSTALL_DAEMON_BINARY` are reserved for local/CI installation and may use the host's native architecture. Published release artifacts remain x86_64-only until other architectures have automated release coverage. Installing identical artifacts twice is a no-op; different verified artifacts replace them atomically.

## Self-update

An installed node can apply the verified artifact flow without rerunning the bootstrap script. The installer records the selected channel under `/etc/lumic/channel`; stable is the default. Nightly nodes can also install a daily systemd timer:

```bash
sudo lumic self-update apply
sudo lumic self-update enable-nightly
```

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

which reports OS, architecture, node identity, resources, Lumic version/channel and managed capabilities.

For the operational summary, including recent changes and anything needing attention:

```bash
lumic how-are-you
```

Create the initial UI credential and connect over an SSH tunnel:

```bash
sudo lumic ui token rotate
ssh -L 8080:127.0.0.1:8080 root@server
```

The UI listens on `127.0.0.1:8080` by default. `LUMIC_UI_BIND` may select another loopback address/port; non-loopback binds are rejected.

## Supported systems

Installer/detection and attention-summary smoke coverage runs on Ubuntu 22.04, Ubuntu 24.04, Debian 12 and Debian 13 on x86_64. A live PostgreSQL/Redis lifecycle/database/backup scenario runs on Ubuntu 24.04. Additional distributions and architectures require automated detection and installation coverage before being documented as supported.

Continue with the [first-VPS guide](@/docs/first-vps.md), then use the [feature matrix](@/docs/feature-matrix.md) to distinguish implemented behavior from nightly expansion.
