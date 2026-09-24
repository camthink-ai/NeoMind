#!/usr/bin/env node
/**
 * WCAG contrast audit for the semantic color tokens in src/index.css.
 *
 * Parses the light (before ".dark {") and dark (".dark {" onwards) token
 * blocks directly, so the check always reflects what ships. Verifies every
 * "colored text on its carrier" combination the UI renders:
 *
 *   text-<role> on background        — plain colored text on the page bg
 *   text-<role> on card              — colored text on cards/table rows
 *   text-<role> on <role>-bg pill    — the tinted-badge pattern (.badge-*)
 *
 * WCAG 2.1 AA: ≥ 4.5:1 for normal-size text (badge text is 12px — strict
 * side). Alpha tokens composite onto their real carrier: background over
 * white, card over background, wash over card.
 */
import { readFileSync } from "node:fs"

const css = readFileSync(new URL("../src/index.css", import.meta.url), "utf8")
const DARK_MARK = ".dark {"
const light = css.slice(0, css.indexOf(DARK_MARK))
const dark = css.slice(css.indexOf(DARK_MARK))

function varsOf(block) {
  const vars = {}
  for (const m of block.matchAll(/(--[a-z-]+):\s*([^;]+);/g)) vars[m[1]] = m[2].trim()
  return vars
}

// oklch(L C H [/ alpha]) → sRGB triple; alpha returned separately.
function parseOklch(str) {
  const m = str.match(/oklch\(\s*([\d.]+)\s+([\d.]+)\s+([\d.]+)(?:\s*\/\s*([\d.]+%?))?\)/)
  if (!m) return null
  const L = +m[1], C = +m[2], h = (+m[3] * Math.PI) / 180
  const a = C * Math.cos(h), b = C * Math.sin(h)
  const l_ = L + 0.3963377774 * a + 0.2158037573 * b
  const m_ = L - 0.1055613458 * a - 0.0638541728 * b
  const s_ = L - 0.0894841775 * a - 1.291485548 * b
  const enc = (c) => (c <= 0.0031308 ? 12.92 * c : 1.055 * Math.pow(c, 1 / 2.4) - 0.055)
  const clamp = (c) => Math.min(1, Math.max(0, enc(c)))
  return {
    rgb: [
      clamp(4.0767416621 * l_ ** 3 - 3.3077115913 * m_ ** 3 + 0.2309699292 * s_ ** 3),
      clamp(-1.2684380046 * l_ ** 3 + 2.6097574011 * m_ ** 3 - 0.3413193965 * s_ ** 3),
      clamp(-0.0041960863 * l_ ** 3 - 0.7034186147 * m_ ** 3 + 1.707614701 * s_ ** 3),
    ],
    alpha: m[4] ? (m[4].endsWith("%") ? parseFloat(m[4]) / 100 : parseFloat(m[4])) : 1,
  }
}

// Composite fg (with alpha) over bg.
function over(fg, bg, alpha) {
  return fg.map((c, i) => c * alpha + bg[i] * (1 - alpha))
}

function luminance([r, g, b]) {
  const f = (c) => (c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4))
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
}

function contrast(a, b) {
  const [l1, l2] = [luminance(a), luminance(b)].sort((x, y) => y - x)
  return (l1 + 0.05) / (l2 + 0.05)
}

// Resolve a var to composited sRGB over `base`.
function resolve(vars, name, base) {
  const raw = vars[name]
  if (!raw) return null
  const parsed = parseOklch(raw)
  if (!parsed) return null
  return over(parsed.rgb, base, parsed.alpha)
}

const WHITE = [1, 1, 1]
// The variable NAMES differ per role: the semantic four are `--color-<role>`,
// but `primary` predates that convention and is plain `--primary` /
// `--primary-bg`. Spelling both out beats deriving one and silently skipping
// the other — `bg-primary-light text-primary` is the tinted-callout pattern
// used across the app, and nothing was checking it.
const ROLES = [
  { role: "success", text: "--color-success", wash: "--color-success-bg" },
  { role: "warning", text: "--color-warning", wash: "--color-warning-bg" },
  { role: "error", text: "--color-error", wash: "--color-error-bg" },
  { role: "info", text: "--color-info", wash: "--color-info-bg" },
  { role: "primary", text: "--primary", wash: "--primary-bg" },
]
let failures = 0

for (const [theme, block] of [["light", light], ["dark", dark]]) {
  const vars = varsOf(block)
  const bg = resolve(vars, "--background", WHITE)
  const card = resolve(vars, "--card", bg)
  console.log(`\n== ${theme} ==`)
  for (const { role, text: textVar, wash: washVar } of ROLES) {
    const text = resolve(vars, textVar, WHITE)
    if (!text) { console.log(`  !! missing ${textVar}`); failures++; continue }
    const rows = [
      [`background`, bg],
      [`card`, card],
    ]
    const wash = resolve(vars, washVar, card)
    if (wash) rows.push([`${role}-pill`, wash])
    for (const [name, carrier] of rows) {
      const c = contrast(text, carrier)
      const pass = c >= 4.5
      if (!pass) failures++
      console.log(`  text-${role.padEnd(8)} on ${name.padEnd(11)} ${c.toFixed(2)}:1  ${pass ? "AA ✓" : "AA ✗ FAIL"}`)
    }
  }
}

console.log(failures === 0 ? "\nAll combinations pass AA (4.5:1)." : `\n${failures} combination(s) below AA (4.5:1).`)
process.exit(failures === 0 ? 0 : 1)
