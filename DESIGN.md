# Paperloom Design System

This replaces the first-pass theme (Iced stock blue), which was picked without real
reference and correctly called out for it. Everything below is sourced from the
`ui-ux-pro-max` skill's design database — real matched product palettes and
typography pairings, not invented on the spot — adapted for a native Rust/Iced
desktop app rather than a web page.

## Direction

**Warm, minimal, professional utility tool.** No blue, no purple, anywhere —
explicit hard constraint. Style reference: *Minimalism & Swiss Style*
(clean, spacious, grid-based, single accent color used purposefully, functional
over decorative). Reference product category for the palette: warm productivity/
creative tools (recipe, notes, bookmark apps in the database all converged on the
same terracotta/amber family).

## Color

### Light

| Token | Hex | Use |
|---|---|---|
| Background | `#FFFBEB` | App background — warm cream, not white |
| Surface (sidebar/cards) | `#FFFFFF` | Sidebar, content cards |
| Muted | `#F8F2F0` | Secondary surfaces, disabled states |
| Border | `#F2E6E2` | Dividers, input borders |
| Foreground (text) | `#1C1410` | Primary text — warm near-black, not pure `#000` |
| Muted foreground | `#78716C` | Secondary text, placeholders |
| Primary (accent) | `#9A3412` | Terracotta — primary actions, active nav, focus |
| Primary hover | `#C2410C` | Lighter burnt-orange, hover/press state |
| Success | `#059669` | Positive state (e.g. "now playing") |
| Destructive | `#DC2626` | Remove/stop, destructive actions only |

### Dark

| Token | Hex | Use |
|---|---|---|
| Background | `#1C1917` | Warm near-black (brown undertone, not cool slate) |
| Surface (sidebar/cards) | `#262220` | Sidebar, content cards |
| Muted | `#2E2824` | Secondary surfaces |
| Border | `rgba(255,255,255,0.08)` | Dividers |
| Foreground (text) | `#F5EFE9` | Warm off-white, not pure `#FFF` |
| Muted foreground | `#A8998E` | Secondary text |
| Primary (accent) | `#FB923C` | Brighter warm orange — needs the lift for dark-bg contrast |
| Primary hover | `#FDBA74` | Lighter still, hover/press |
| Success | `#34D399` | Positive state |
| Destructive | `#F87171` | Remove/stop |

Rationale for the dark background specifically: the design database's "dark mode"
palettes defaulted to cool navy/slate (`#0F172A` and similar) even on warm-accent
products — that's a web-dashboard convention, not a requirement. `#1C1917` is a
real value from the matched "warm" results, just applied as the base instead of
an accent.

## Typography

**Inter**, single family, weight-driven hierarchy (matched reference: "high-end
utility" / developer-tool pairing). No second display font — one typeface, used
consistently, is more in line with Swiss-style minimalism than mixing families.

- Page titles: Inter, 600 weight, 22px
- Section headers: Inter, 600 weight, 15px
- Body/labels: Inter, 400 weight, 14px
- Muted/secondary text: Inter, 400 weight, 13px

Not yet embedded as a bundled font in the Iced build (ships as a system/fallback
font for now) — bundling it is a reasonable fast-follow, not done in this pass.

## Layout

- **Sidebar**: fixed width, `Surface` background (visually distinct from the
  `Background` content area — this is what was missing before; everything was
  one flat color). App name at top. Nav items: generous padding, bold label +
  left accent bar when active, not just a filled button.
- **Top bar**: a real header region per page — page title (large) on the left,
  primary actions grouped on the right. Not just a loose row of buttons floating
  under the OS title bar, which is what existed before.
- **Content**: below the top bar, scrollable, generous spacing (Swiss-style —
  breathing room, not dense).
- Native OS window decorations are kept (no custom title bar) — replacing those
  is a much bigger, riskier change (manual drag-to-move, platform-specific) not
  justified by what was actually asked for here.

## Motion

Real interaction feedback, scoped to what's achievable with Iced's actual
primitives (`iced::Animation`, per-widget `Status`-based styling) rather than
promising a fully custom animated widget set:

- **Settings search → highlight**: already existed, kept — fades in over ~200ms.
- **Toggle flipped**: the row flashes the accent color briefly (same
  `Animation<bool>` technique as the search highlight) as confirmation feedback,
  since Iced's `toggler` widget itself has no exposed animation hook for its
  thumb — building a fully custom animated toggle widget from scratch was judged
  not worth the risk/complexity for this pass.
- **Hover/press**: immediate but deliberate style changes per Iced's `Status`
  (Active/Hovered/Pressed) — not tweened, but real, distinct, consistent states
  everywhere instead of the flat single-state look before.
- **Destructive actions get a confirmation step** (Remove didn't have one before
  — direct hit on a flagged UX anti-pattern: "no confirmation before delete").

## What this pass does *not* do

- No custom-animated toggle thumb (needs a hand-built Widget impl — real scope,
  deferred).
- No page cross-fade transition (needs generic opacity compositing Iced doesn't
  expose simply — deferred).
- No bundled font file yet (Inter specified, not embedded).
- No custom title bar / window chrome (native decorations kept).

These are honest scope cuts, not oversights — each would be a substantial side
quest on its own.
