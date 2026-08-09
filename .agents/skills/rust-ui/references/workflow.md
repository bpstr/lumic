# Registry workflow

Rust/UI is a source registry: components and blocks are copied into the project
and become application-owned code.

## Discover

1. Inspect the live documentation at `https://rust-ui.com/docs/components`,
   `https://rust-ui.com/docs/hooks`, `https://rust-ui.com/blocks`, or
   `https://rust-ui.com/charts`.
2. Read the component page and its source tab. Registry markdown is commonly
   available at `https://rust-ui.com/registry/components/<slug>.md` and blocks
   at `https://rust-ui.com/registry/blocks/<slug>.md`.
3. Confirm the current API from source. Documentation, CLI output, and cached
   catalog counts may lag one another.

## Install

The documented CLI flow is:

```bash
cargo install rust-ui-cli
cargo rust-ui add button
```

Run help before using any additional flags:

```bash
cargo rust-ui --help
cargo rust-ui add --help
```

Do not invent flags or overwrite locally modified component files without first
reviewing the diff. If the CLI is unavailable, copy the complete documented
source and its declared dependencies; do not scrape rendered demo markup.

## Update

Registry files are project-owned after installation. Compare upstream source to
the local version, preserve intentional application changes, merge behavior and
accessibility fixes, then rerun the repository's Rust and browser checks.

## Architecture boundary

A Rust/UI block may be translated rather than installed when the host is not a
Leptos application. Preserve its composition, semantics, responsive behavior,
and accessibility without importing an incompatible runtime.
