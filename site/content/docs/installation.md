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

To install and provision a dedicated passwordless MCP key in the same SSH login, generate a key used only for Lumic and pass its public half to the installer:

```bash
MCP_KEY="$HOME/.ssh/lumic-server"
test -f "$MCP_KEY" || ssh-keygen -q -t ed25519 -N '' -C lumic-mcp -f "$MCP_KEY"
MCP_PUBLIC_KEY="$(cat "$MCP_KEY.pub")"
ssh root@server "export LUMIC_MCP_AUTHORIZED_KEY='$MCP_PUBLIC_KEY'; curl -fsSL https://lumic.cc/install.sh | sh"
codex mcp add lumic-server -- ssh -T -o BatchMode=yes -i "$MCP_KEY" root@server
```

The installed `authorized_keys` entry uses OpenSSH `restrict` plus a forced `lumic mcp serve` command. That dedicated key cannot request a root shell, port forwarding or an arbitrary command. Keep ordinary unrestricted administration keys out of the agent configuration.

When already connected to the server:

```bash
curl -fsSL https://lumic.cc/install.sh | sudo sh
```

`https://lumic.cc/install.sh` is published from the repository's root `install.sh` during the documentation deployment, so the website and repository always use the same installer source. `https://lumic.cc/install` is kept as a compatibility alias.

The installer detects the operating system and architecture, selects the requested release channel, downloads the matching Lumic binaries, verifies their versions and the built-in MCP adapter, installs them under `/usr/local/bin`, creates the first UI credential, and enables `lumicd`. It prints the one-time UI token, tunnel URL and MCP command at the end.

## Channels

Lumic has two release channels:

- **stable** — conservative production releases;
- **nightly** — the latest gated build from main.

The installer defaults to stable. Select nightly explicitly during nightly testing:

```bash
curl -fsSL https://lumic.cc/install.sh | sudo env LUMIC_CHANNEL=nightly sh
```

A node never silently changes channels.

Stable installation resolves GitHub's latest stable release. Pin an exact immutable stable or prerelease when required:

```bash
curl -fsSL https://lumic.cc/install.sh | sudo env LUMIC_VERSION=2.0.0-alpha.4 sh
```

Exact prerelease versions are opt-in and do not change the node's recorded stable/nightly channel. GitHub marks prerelease tags separately, so they never replace the latest stable release.

Release downloads include SHA-256 files and GitHub artifact attestations. The installer enforces the checksum; a downloaded binary can also be checked independently with `gh attestation verify <artifact> --repo bpstr/lumic`.

Installer inputs are environment variables: `LUMIC_CHANNEL` (`stable` or `nightly`), `LUMIC_VERSION` for an explicit stable or prerelease version, `LUMIC_INSTALL_DIR`, `LUMIC_CONFIG_DIR`, `LUMIC_STATE_DIR`, and optional `LUMIC_MCP_AUTHORIZED_KEY` for a dedicated restricted OpenSSH key. `LUMIC_INSTALL_BINARY` and optional `LUMIC_INSTALL_DAEMON_BINARY` are reserved for local/CI installation and may use the host's native architecture. Published release artifacts remain x86_64-only until other architectures have automated release coverage. Installing identical artifacts twice is a no-op; different verified artifacts replace them atomically.

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

The installer creates the initial UI credential if none exists. Rotate it later if needed, then connect over an SSH tunnel:

On the VPS, or from a normal SSH session to it, rotate a lost or expired token:

```bash
sudo lumic ui token rotate
```

On your local computer, start the tunnel and leave that terminal running while you use the UI:

```bash
ssh -N -L 8080:127.0.0.1:8080 root@server
```

Then open `http://127.0.0.1:8080` in your local browser. The UI listens on the VPS at `127.0.0.1:8080` by default; the SSH tunnel makes it available only on your local computer. `LUMIC_UI_BIND` may select another loopback address/port; non-loopback binds are rejected. See the [operator UI guide](@/docs/operator-ui.md) for sign-in, session, service, and remote-access details.

## Supported systems

Installer/detection and attention-summary smoke coverage runs on Ubuntu 22.04, Ubuntu 24.04, Debian 12 and Debian 13 on x86_64. A live MySQL/PostgreSQL/Redis lifecycle/database/backup scenario runs on Ubuntu 24.04. Additional distributions and architectures require automated detection and installation coverage before being documented as supported.

Continue with the [first-VPS guide](@/docs/first-vps.md), then use the [feature matrix](@/docs/feature-matrix.md) to distinguish implemented behavior from nightly expansion.
