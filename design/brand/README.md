# NeoMind Brand — Square Logo (2026 redesign)

The square "Ni" mark and every raster icon derived from it. Regenerate all
assets after any change here with:

```bash
python3 design/brand/generate.py
```

## Mark geometry

100×100 square grid, uniform stroke `S = 23` (cap radius `11.5`), all strokes
fully rounded (capsules + circle):

| element | shape |
|---|---|
| left stem | capsule (11.5, 11.5) → (11.5, 88.5) — full cap height |
| diagonal | capsule (11.5, 11.5) → (88.5, 88.5) — 45°, drawn underneath |
| i stem | capsule (88.5, 46.0) → (88.5, 88.5) — bottom-right, baseline-aligned |
| dot | circle center (88.5, 11.5), r = 11.5 — top-right |

The diagonal shares its end caps with the stems' caps, so the mark reads as
one connected flowing structure (数据流 + 智能节点).

## Brand colors

| name | hex |
|---|---|
| Neo Orange | `#FF8A00` |
| Neo Red | `#F0441E` |
| Deep Red | `#D92020` |
| tile dark | `#0A0A0B` |
| tile light | `#FFFFFF` |
| mono on dark | `#FFFFFF` |
| mono on light | `#101013` |

Gradient direction: `#FF8A00 → #D92020`. Each stroke carries its own gradient
along its own axis; the diagonal starts a step deeper (`#FF8000`) so the
"fold" seam is visible where strokes meet.

## Variants

| file | background | mark |
|---|---|---|
| `icon-dark-color.svg` | dark rounded tile | brand gradient |
| `icon-light-color.svg` | light rounded tile | brand gradient |
| `icon-dark-mono.svg` | dark rounded tile | flat white |
| `icon-light-mono.svg` | light rounded tile | flat near-black |
| `neomind-mark.svg` | transparent | brand gradient |
| `neomind-mark-mono.svg` | transparent | `currentColor` (themes via CSS) |

Mark height = 54% of the tile side, centered. Tiles use 22.5% corner radius.

## Generated assets (do not hand-edit)

- `web/public/` — `logo-square.png` (128), favicons 16/32, `apple-touch-icon`
  (180), PWA `icon-192/512.png` (dark tile, color mark), `logo-mark.png`
  (512, transparent gradient mark — dark-theme in-app surfaces, e.g. the
  sidebar rail) and `logo-mark-mono.svg` (black `currentColor` mark —
  light-theme in-app surfaces; `<img>` embeds resolve currentColor to black)
- `crates/neomind-api/static/` — same set, served by the API binary
- `web/src-tauri/icons/` — Tauri bundle PNGs, `icon.ico`, `icon.icns`,
  `Square*Logo.png`/`StoreLogo.png` (Windows), `ios/AppIcon-*.png` (white
  full-bleed, iOS applies its own mask), `android/mipmap-*` (dark tile
  layers; adaptive background color stays `#fff` in
  `android/values/ic_launcher_background.xml`)

`web/dist/` copies are build output — refreshed by `npm run build`, not by
this script.

Out of scope here: the horizontal "NeoMind" wordmark (`logo-light.png` /
`logo-dark.png`) and the PWA splash screens still use the previous wordmark
artwork.
