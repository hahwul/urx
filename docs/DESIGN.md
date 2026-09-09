# urx docs design contract

The decisions the stylesheet depends on. Read this before restyling anything.

## Direction

Deep-space terminal. Graphite neutrals, one amber accent, jet motif. The site is a
reference people read while working, so reading beats spectacle: motion is a
courtesy, never the point.

## Palette

Every value is sampled from the logo (`tools/brand/src/mark-source.jpg`).

| Token | Light | Dark | Role |
|---|---|---|---|
| `--accent` | `#FCA428` | `#FCA428` | Fills, borders, markers. The thruster ring. |
| `--accent-text` | `#9C5A0A` | `#FCA428` | Any amber **text** |
| `--bg` | `#FBFBF9` | `#0B0B0E` | Page |
| `--text` | `#1F222A` | `#D7DAE3` | Body |
| `--heading` | `#0F1116` | `#F9F9FA` | Headings |
| `--muted` | `#5C6270` | `#A2A8B8` | Secondary |
| `--quiet` | `#6A7080` | `#7F8695` | Small labels |

### The one hard rule

Raw `#FCA428` on a light surface is **1.94:1 and fails WCAG AA**. On light backgrounds
amber is a fill, border or marker colour only. Amber text uses `--accent-text`, which
drops to `#9C5A0A` (5.23:1). `--accent` and `--accent-text` look redundant. They are not.
Do not collapse them.

Measured: accent text 9.81:1 dark / 5.22:1 light · body 14.06:1 dark / 15.35:1 light ·
muted 8.26:1 dark / 5.90:1 light · quiet 5.38:1 dark / 4.78:1 light · primary button
label on amber 9.46:1. `--quiet` carries 11-12px labels, which WCAG counts as normal
text, so it clears 4.5:1 rather than the 3:1 large-text threshold.

Primary buttons take near-black text on the amber fill in **both** themes: white on
amber is 2:1.

## Theming

One declaration per token via `light-dark()`, with `color-scheme: light dark` on
`:root`. No stored preference means no `data-theme` attribute, so the OS decides.
The toggle sets `data-theme` and persists to `localStorage["urx-theme"]`; a blocking
script in `header.html` applies it before first paint.

Hairlines derive from the text colour with `color-mix()` rather than being a second
palette, so they flip for free.

`light-dark()` is colour-only. The logo swap in `.brand-mark` therefore needs the two
explicit theme rules; that duplication is deliberate and is the only one in the file.

## Type

- Display: **Space Grotesk**, self-hosted, latin subset, variable. Its default instance
  is **300**, so every rule that wants weight must say so.
- Body: system sans stack. No webfont.
- Mono: **JetBrains Mono**, self-hosted, latin subset.

No Google Fonts CDN: two local woff2 files, 53 KB total.

## Shape

One radius system: `--r-sharp: 3px` for nav and docs chrome, `--r-code: 4px` for code,
`--r-card: 12px` for panels, `--r-pill` for anything interactive. Docs surfaces stay
near-square for the terminal feel; buttons and chips stay pill.

## The contrail

The logo's amber exhaust line is the site's one recurring device:

- a short segment under section and page headings
- a 2px left edge on the active sidebar link
- a segment on each provider rule that runs to full width on hover
- a gradient fading across the four pipeline stages

If a new component needs to signal "current" or "start here", use the contrail rather
than inventing another marker.

## Motion

`MOTION_INTENSITY` is deliberately low. Scroll reveals on landing sections, hover
transitions, and nothing else. Everything animated sits behind both
`prefers-reduced-motion: no-preference` and a `has-js` class, so with either absent the
content is simply visible. A global reduce kill-switch closes the stylesheet.

No scroll hijacking, no parallax, no marquees, no infinite loops.

## Layout

3-column grid: sidebar 238px / content max 780px / TOC 196px, container max 1600px.
Both rails are `position: sticky`, not fixed, so the footer spans the true page width.

Breakpoints: 1200px TOC rail appears · 1199px TOC moves inline under the heading ·
860px header collapses to a hamburger and the sidebar becomes a disclosure · 600px
padding tightens.

## Navigation is derived, never hand-maintained

The sidebar, prev/next and section listings all come from `site.sections` and
front-matter `weight`. A new page needs `title`, `description`, `weight` and
`toc = true`, and it appears everywhere on its own. This is the most fragile part of
the design: a missing `weight` silently reorders the nav.

## Assets

`tools/brand/generate.sh` regenerates every logo, icon and the OG card from one source
illustration. Notes worth keeping:

- The background cut is a corner floodfill at **14%** fuzz. At 24% it breaks through
  the hull.
- The mark is 1.68:1. Fitted bare into a square it turns to mush at 16px, so icons
  rotate the jet onto the square's diagonal, brighten the hull, and add an amber bloom
  at the thruster. At small sizes colour is the recognition cue, not form.
- The artwork is flat, so a 256-colour palette is visually lossless (RMSE < 1%) and
  cuts the PNGs by 3-4x.
- On light backgrounds the near-white highlight strokes vanish and split the
  silhouette, which is what `mark-light` exists to fix.

## Checks before shipping a visual change

1. Both themes, at 1440 / 1024 / 390 px.
2. `guide/cli-options` is the stress test: 702 lines, the longest TOC.
3. Every amber text pair against its actual background.
4. `hwaro doctor` and `hwaro tool check-links`.
5. Keyboard: tab to the search button, Cmd-K, arrows, Escape.
