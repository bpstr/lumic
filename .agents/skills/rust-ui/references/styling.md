# Styling and accessibility

- Use the existing semantic CSS variables (`background`, `foreground`, card,
  muted, accent, destructive, border, input, ring, chart, and sidenav tokens).
- Prefer built-in variants and sizes, then small layout classes, then a
  deliberate component variant. Avoid raw colors for status and action meaning.
- Change theme variables centrally; do not scatter duplicate light/dark colors
  through component call sites.
- Preserve the radius and font selected by the Rust/UI theme builder when a
  project uses a generated preset.
- Use gaps for layout and leave component internals responsible for icon sizing,
  overlay stacking, and interaction-state styling.
- Every interactive element has a visible focus state, an accessible name, and
  an appropriate native element. Icon-only controls need a label.
- Preserve `prefers-reduced-motion` behavior and documented RTL support.
- Responsive behavior must remain functional, not merely visually hidden. For
  modal mobile navigation, manage focus and expose an explicit close action.
