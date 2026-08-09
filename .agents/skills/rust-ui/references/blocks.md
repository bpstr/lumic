# Blocks

The live catalog currently groups blocks into login, sidenav, headers, footers,
FAQ, and integrations. Treat counts as volatile and inspect the live page.

## Sidenav variants

The current sidenav catalog includes grouped sections, collapsible menus,
submenus, floating and inset layouts, icon collapse, route-based nesting,
search, and right-side placement.

### Grouped sections (`sidenav01`)

The source composes:

```text
SidenavWrapper
  Sidenav
    SidenavHeader
    SidenavContent
      SidenavGroup
        SidenavGroupLabel
        SidenavGroupContent
          SidenavMenu
            SidenavLink
    SidenavFooter
  Outlet
```

It also reuses the same content in a left-side `Sheet` on mobile. Production
adaptation should:

- derive `aria-current`/active styling from the router;
- preserve user-specified group names and destinations; otherwise group by
  operator intent, not arbitrary item count;
- keep group labels visible and meaningful;
- replace demo route constants and account data;
- make search real or omit it—never ship a decorative search control;
- retain a stable content area and a keyboard-operable mobile navigation panel;
- keep account/session actions in the footer;
- supply identity, node, environment, and health text from real typed state;
- omit unavailable account or host details instead of copying demo values;
- use the project's router link primitive for client-side navigation;
- give the mobile `Sheet` a `SheetHeader` and `SheetTitle` (visually hidden is
  acceptable) so assistive technology receives a dialog name.

Source: `https://rust-ui.com/registry/blocks/sidenav01.md`.
