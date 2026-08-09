---
name: rust-ui
description: Build, add, compose, debug, style, or review Rust/UI interfaces for Rust and Leptos. Use whenever a task mentions rust-ui.com, Rust/UI components, blocks, hooks, charts, icons, themes, the rust-ui CLI, or asks for shadcn-style UI in a Leptos/Rust application. Also use when a Rust web UI needs accessible component selection, Leptos view! examples, registry source inspection, or migration away from React/shadcn assumptions.
---

# Rust/UI

Use Rust/UI as a copy-and-customize registry for Rust/Leptos, not as a React
package. Preserve the host application's architecture and keep domain behavior
outside presentation components.

## Start with project context

1. Inspect `Cargo.toml`, the web framework, Tailwind setup, router, and existing
   component directory.
2. Determine whether the application is Leptos CSR, SSR, or hydrated. Do not add
   Leptos or a browser runtime to a server-rendered Rust application solely to
   copy a visual pattern.
3. Search existing project components before adding another copy.
4. Consult the current Rust/UI page and registry source for the exact component
   or block. The live catalog changes; do not rely on remembered props.

Read [references/workflow.md](references/workflow.md) before installing or
updating registry code. Read [references/leptos.md](references/leptos.md) when
writing component code.

## Compose before inventing

- Use the component that owns the behavior: `Dialog` for focused input,
  `AlertDialog` for destructive confirmation, `Sheet` for a side panel,
  `Drawer` for a mobile-first panel, and `Popover` for small contextual content.
- Preserve documented part hierarchy. Items belong in their group; dialog-like
  surfaces need titles; cards use header/content/footer; tabs triggers belong in
  a tabs list; avatars include a fallback.
- Prefer `Field`, `FieldGroup`, and `FieldSet` for forms. Use the appropriate
  input primitive rather than styled generic markup.
- Prefer Rust/UI variants, states, hooks, and utilities over custom state
  machines, overlays, scroll logic, or animations.

Read [references/composition.md](references/composition.md) for the concrete
rules and [references/catalog.md](references/catalog.md) to choose a component.

## Keep the Rust/UI contract

- Write Leptos `#[component]` functions and `view!` markup, signals, callbacks,
  and router links. Never emit React syntax such as `className`, `useState`,
  `asChild`, `"use client"`, JSX fragments, or npm imports.
- Use Rust/UI's current import paths from the copied registry source. Do not
  guess crate/module paths.
- Use Rust/UI Icons/Lucide components when the project already uses them; icons
  supplement visible labels and need accessible naming when icon-only.
- Use semantic theme variables and existing component variants. Keep layout
  overrides local; avoid raw state colors and duplicated dark-mode values.
- Maintain keyboard behavior, focus order, labels, titles, reduced motion, and
  RTL behavior exposed by the original component.

Read [references/styling.md](references/styling.md) for theme and styling rules.

## Blocks are reference compositions

Blocks demonstrate how primitives fit together. Copy their information
architecture and interaction model, then adapt routes and data to the product.
Do not copy demo-only route constants, sample accounts, placeholder search, or
fake state into production.

Pass visible identity, node, environment, and health labels from authenticated
session or host state through typed props. If that data does not exist, omit the
label instead of inventing a plausible name, email address, environment, or
connection status.

For sidenavs, preserve `SidenavWrapper -> Sidenav -> Header/Content/Footer`,
group labels and menu items, route-aware active state, and the `Sheet` mobile
pattern. See [references/blocks.md](references/blocks.md).

## Verify

- Run Rust formatting, linting, and tests required by the repository.
- Build the relevant CSR/SSR/hydration target.
- Test desktop and mobile layouts, keyboard navigation, focus visibility,
  active route state, overlays, validation, loading, empty, and error states.
- Inspect browser console output and server logs. Do not declare success from a
  static code read alone when the UI can be run locally.
