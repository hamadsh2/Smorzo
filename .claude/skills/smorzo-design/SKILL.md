---
name: smorzo-design
description: Smorzo's branding and design system — color tokens and the four user-selectable themes (Warm stone default, Midnight, Ink & ember, Paper), typography, spacing, radius, elevation, motion, meter behavior, logo/app/tray icon specs, and a screenshot review checklist. Load whenever working on UI (GPUI/gpui-component), icons, the logo, or README visuals, and whenever the user pastes a screenshot of the window.
---

# Smorzo design system

**Mood:** calm, minimal, classy, easy on the eyes. One muted accent per theme
(sage by default), no pure black (`#000`), no pure white (`#FFF`), no gradients, no heavy borders.

**Rule zero:** every color, size, radius, and duration comes from a token defined here.
Define tokens first (one `theme` module), then apply them everywhere. Never leave a
gpui-component on its default styling, and never hard-code a hex value in a component.

## Color tokens and themes

The user picks a theme in *Settings → Appearance*; **Warm stone is the default**. All
four use the same tokens, defined once in `src/ui/theme.rs` (`PALETTES`). Components ask
for a token with `c(ACCENT)` and never for a hex. The test
`every_theme_keeps_text_readable` checks the contrast of every theme, so a new or
changed palette must still pass it.

| Token | Warm stone (default) | Midnight | Ink & ember | Paper (light) | Use |
|---|---|---|---|---|---|
| `bg` | `#1C1B1A` | `#121519` | `#171412` | `#F4F1EA` | Window background |
| `surface` | `#252422` | `#1A1E24` | `#211D1A` | `#FBF9F5` | Panels, cards |
| `elevated` | `#2E2C29` | `#252B34` | `#2C2622` | `#ECE7DE` | Popovers, hover fills, selected chip |
| `border` | `#3A3733` | `#313844` | `#3A332D` | `#DDD7CC` | Hairlines (1px), off switch track |
| `text_primary` | `#E8E4DC` | `#E4E8EE` | `#EEE6DC` | `#2B2925` | Body text, values |
| `text_secondary` | `#A39E94` | `#98A1AE` | `#ABA094` | `#66615A` | Labels, captions, hints |
| `accent` | `#8FA89A` | `#7FB3C4` | `#D49A6A` | `#557263` | Active toggles, slider fill, focus ring, selected item, the mark in the header |
| `accent_hover` | `#A3BBAE` | `#97C3D1` | `#DFAE85` | `#4A6557` | Hover on accent elements |
| `on_accent` | `#1C1B1A` | `#121519` | `#171412` | `#FBF9F5` | Text on an accent fill (selected segment, primary button) |
| `meter_peak` | `#C9B79C` | `#D9B97E` | `#E6CC9A` | `#8F7650` | Meter peak-hold marker / hot zone |
| `warning` | `#C48A72` | `#D08C76` | `#D47F74` | `#A4634E` | Calm warnings |

**Theme rules:**
- The meter body uses `accent`. The peak-hold tick and the top ~10% of the scale use `meter_peak`.
- The muted state uses `text_secondary`, never red.
- The window's own title bar follows the theme: dark for the dark themes, light for Paper (`theme::match_title_bars`).
- The app icon and tray icon do **not** follow the theme. The app icon is always the sage mark on `#1C1B1A`. The tray icon follows the taskbar.

**Contrast:** the minimums below are enforced by the test.

| Pair | Minimum (all themes) |
|---|---|
| `text_primary` on bg / surface / elevated | 7:1 |
| `text_secondary` on bg / surface / elevated | 4.5:1 |
| `accent`, `meter_peak` and `warning` on bg / surface | 3:1 (UI graphics) |
| `on_accent` on `accent` | 4.5:1 |

**Text color in Paper:** `accent` and `warning` are graphics-only, so never render body text in them. Use `text_primary` for the message, and the token color for an icon, a dot or a left rule.

## Typography
- **Font:** Inter, embedded in the binary (SIL OFL 1.1 — include its license in the
  third-party licenses page).
- **Tabular numerals** (`tnum`) for every numeric readout: CPU %, latency ms, dB, slider
  values. Numbers must not jitter in width as they change.
- Scale (px): `caption 11` · `label 12` · `body 13` · `title 15` · `heading 18`.
- Weights: 400 body, 500 labels/values, 600 headings. No bold-everywhere.
- Sentence case for all labels ("Noise gate", not "NOISE GATE" or "Noise Gate").

## Spacing, radius, elevation
- Spacing scale only: `4 / 8 / 12 / 16 / 24 / 32`. Nothing in between.
  - Inside controls: 8–12. Between related controls: 12. Between sections: 24.
    Window padding: 16–24.
- Radius: **8** for controls (buttons, inputs, toggles, dropdowns), **12** for panels.
- Elevation: very soft only — e.g. shadow `0 2 8` at ≤20% opacity of `bg`'s darker
  neighbor in dark, ≤8% black-ish warm in light. Prefer a surface color step over a shadow.
- Borders: 1px `border` hairlines only where a surface step isn't enough.
- Generous whitespace: when in doubt, remove a divider and add 8px instead.

## Motion
- Durations 150–200 ms, ease-out. No bounces, springs, or looping animations.
- Hover/press: color transitions only (150 ms). No scale effects.
- **Meters:** fast attack (~10 ms), slow release (~300 ms), peak-hold ~1 s then fall.
  Meters animate only while the window is visible; stop all animation frames when
  hidden (zero GPU while hidden is a hard requirement).

## Components (apply to gpui-component)
- **Engine selector:** segmented control — "Off" · "Light — for gaming" ·
  "High quality — for recording & calls". Selected segment: `accent` fill or underline.
- **Strength slider:** track `elevated`, fill `accent`, thumb `text_primary`, value in
  tabular numerals to the right.
- **Toggles (chain stages):** off track `border`, on track `accent`. Label left in
  `text_primary`, optional hint below in `text_secondary`.
- **Device pickers:** dropdown on `surface`, menu on `elevated`, radius 8.
- **Readouts (CPU / latency):** `caption`, `text_secondary` label + `text_primary`
  tabular value, e.g. `CPU 0.6%  ·  Latency 21 ms`.
- **Mute state:** subtle — a small pill or the meter dimming to `text_secondary`, plus
  "Muted" label. Never a red flashing banner.
- **Warnings:** calm card on `surface` with a 2px `warning` left rule, one sentence of
  what's wrong + one sentence of how to fix it.
- Focus ring: 2px `accent`, offset 2px, for keyboard accessibility.

## Logo & icons
- **Mark:** a single soft sound wave that gradually flattens into a straight line —
  noise becoming calm. Left side: 2–3 gentle oscillations of decreasing amplitude;
  right side: a flat line. Single stroke, round caps and joins, uniform stroke width.
- Master as SVG (`assets/icon/smorzo-mark.svg`), 24×24 viewBox, stroke-based.
- **App icon:** sage (`#8FA89A`) mark centered on a `#1C1B1A` rounded square
  (corner radius ≈ 22% of size), mark occupying ~60% of width.
  Export `.ico` with sizes 16, 20, 24, 32, 40, 48, 64, 128, 256.
  At 16/20/24 px, thicken the stroke and reduce to one oscillation so it stays legible.
- **Tray icon:** monochrome mark (follows Windows light/dark taskbar: light glyph on dark
  taskbar, dark glyph on light). **Muted state:** the same mark with the wave fully flat
  plus a small diagonal slash, or reduced opacity — distinct at 16 px.
- Wordmark: "Smorzo" in Inter 600, `text_primary`, slight positive tracking.
- Tagline: "Just your voice." — `text_secondary`.

## README visuals
- Use the dark-theme app icon and a real screenshot (no mockups) at 1× and 2×.
- No gradients, badges wall, or stock imagery. Keep it as calm as the app.

## Screenshot review checklist
When the user pastes a screenshot, check each and give concrete fixes (token / px):
1. Any default gpui-component styling left (blue accents, default greys, default radii)?
2. Any color not in the token table? Any pure black/white?
3. Spacing on the 4/8/12/16/24/32 scale? Consistent section gaps (24)?
4. Radii: 8 on controls, 12 on panels?
5. Numbers in tabular numerals; no width jitter?
6. Text contrast meets the table above (no accent/warning text in light theme)?
7. Hierarchy: one clear primary control (engine selector), secondary info quieter?
8. Heavy borders, dividers, or shadows that could be removed?
9. Meter: fast attack / slow release, peak color only at the top?
10. Mute and warning states calm and legible?
