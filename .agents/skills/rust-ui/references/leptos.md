# Leptos conventions

- Components are Rust functions annotated with `#[component]` and return
  `impl IntoView` (or the project's established return type).
- Markup belongs in `view!`; HTML uses `class=`, not React `className=`.
- Use Leptos signals (`signal`, `RwSignal`, `ReadSignal`, derived closures) for
  reactive state and callbacks such as `on:click` for events.
- Use `leptos_router` route/link primitives already present in the project for
  navigation and active-route state.
- Preserve SSR/hydration constraints. Browser-only APIs must be gated or used
  through the documented Rust/UI hook.
- Use typed component props and callbacks. Do not smuggle behavior through raw
  HTML strings or JavaScript unless the component API explicitly requires it.
- Follow the project's Rust edition and Leptos version. Rust/UI's current site
  targets Leptos 0.8 and Rust edition 2024, but the local project wins.

Minimal shape:

```rust
use leptos::prelude::*;

#[component]
pub fn StatusCard(title: &'static str, value: Signal<String>) -> impl IntoView {
    view! {
        <Card>
            <CardHeader>
                <CardTitle>{title}</CardTitle>
            </CardHeader>
            <CardContent>{move || value.get()}</CardContent>
        </Card>
    }
}
```

Always replace illustrative import paths with paths confirmed from the current
registry source and the local project.
