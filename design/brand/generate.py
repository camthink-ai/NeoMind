#!/usr/bin/env python3
"""NeoMind brand asset generator.

Renders the square "Ni" logo (2026 redesign) and writes every raster brand
asset in the repo: web favicons/PWA icons, the API-served static copies, and
the full Tauri desktop icon set (PNG / .ico / .icns / iOS / Android).

Geometry (mark space, 100x100 square grid, stroke S=23, cap radius R=11.5):
  left stem  capsule (11.5, 11.5) -> (11.5, 88.5)
  diagonal   capsule (11.5, 11.5) -> (88.5, 88.5)   [drawn first, under stems]
  i stem     capsule (88.5, 46.0) -> (88.5, 88.5)
  dot        circle  center (88.5, 11.5) r=11.5

Brand colors (spec): Neo Orange #FF8A00, Neo Red #F0441E, Deep Red #D92020.
Gradient direction: #FF8A00 -> #D92020 (each stroke along its own axis; the
diagonal starts slightly deeper so the "fold" seam shows where strokes meet).

Usage:  python3 generate.py [--out-root PATH]
Requires: Pillow, numpy, ImageMagick (magick) for .ico, iconutil for .icns.
"""

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

REPO_ROOT = Path(__file__).resolve().parents[2]
OUT_ROOT = REPO_ROOT

# ---------------------------------------------------------------- constants

# Mark geometry (mark space 0..100)
S = 23.0            # stroke width
R = S / 2           # cap radius
STEM = ((11.5, 11.5), (11.5, 88.5))
DIAG = ((11.5, 11.5), (88.5, 88.5))
ISTEM = ((88.5, 46.0), (88.5, 88.5))
DOT = ((88.5, 11.5), R)

# Colors
ORANGE = "#FF8A00"
RED = "#F0441E"
DEEP_RED = "#D92020"
TILE_DARK = "#0A0A0B"
TILE_LIGHT = "#FFFFFF"
MONO_ON_DARK = "#FFFFFF"
MONO_ON_LIGHT = "#101013"

MARK_FRAC = 0.54     # mark height as fraction of the tile side (spec ~0.50)

# Tile styles: (tile side as fraction of canvas, corner radius as fraction of tile)
STYLE_WEB = dict(tile=1.0, radius=0.225)         # full-bleed rounded tile
STYLE_DESKTOP = dict(tile=0.895, radius=0.20)    # padded tile (Tauri/Windows)
STYLE_SOURCE = dict(tile=0.82, radius=0.20)      # icon-source-rounded / ico/icns master
STYLE_ANDROID_LAUNCHER = dict(tile=0.75, radius=0.21)
STYLE_ANDROID_ROUND = dict(tile=0.82, radius=0.26)
STYLE_ANDROID_FOREGROUND = dict(tile=0.894, radius=0.20)
STYLE_IOS = dict(tile=1.0, radius=0.0)           # full-bleed square, OS applies mask

GRADIENTS = {
    # stroke -> (axis start, axis end, stops [(t, hex), ...]) in mark space
    "stem": ((0, 0), (0, 100), [(0.0, ORANGE), (0.45, "#FA6805"), (1.0, DEEP_RED)]),
    "diag": ((0, 0), (100, 100), [(0.0, "#FF8000"), (1.0, DEEP_RED)]),
    "istem": ((0, 34.5), (0, 100), [(0.0, ORANGE), (0.45, "#FA6805"), (1.0, DEEP_RED)]),
    "dot": ((0, 0), (0, 23), [(0.0, ORANGE), (1.0, RED)]),
}
# draw order: diagonal under, then stems, then dot
DRAW_ORDER = ["diag", "stem", "istem", "dot"]
SHAPES = {"stem": STEM, "diag": DIAG, "istem": ISTEM, "dot": DOT}


def hex_rgb(h):
    h = h.lstrip("#")
    return np.array([int(h[i:i + 2], 16) for i in (0, 2, 4)], dtype=np.float64)


# ------------------------------------------------------------ mark renderer

def _seg_distance_fields(x, y, seg):
    """Distance from each (x,y) point to a capsule center segment."""
    (x0, y0), (x1, y1) = seg
    dx, dy = x1 - x0, y1 - y0
    ll = dx * dx + dy * dy
    t = ((x - x0) * dx + (y - y0) * dy) / ll
    t = np.clip(t, 0.0, 1.0)
    return np.hypot(x - (x0 + t * dx), y - (y0 + t * dy))


def render_mark(size, mono=None):
    """RGBA mark layer on transparent bg. mono=hex renders flat single color."""
    hi = size * (4 if size <= 128 else 2)  # internal supersample
    ys, xs = np.mgrid[0:hi, 0:hi]
    u = ((xs + 0.5) * 100.0 / hi).astype(np.float32)
    v = ((ys + 0.5) * 100.0 / hi).astype(np.float32)

    def field(name):
        if name == "dot":
            return (np.hypot(u - DOT[0][0], v - DOT[0][1]) <= R + 1e-9)
        return (_seg_distance_fields(u, v, SHAPES[name]) <= R + 1e-9)

    canvas = np.zeros((hi, hi, 4), dtype=np.float64)
    for name in DRAW_ORDER:
        mask = field(name)
        if not mask.any():
            continue
        if mono:
            rgb = np.tile(hex_rgb(mono), (hi, hi, 1))
        else:
            (ax0, ay0), (ax1, ay1), stops = GRADIENTS[name]
            gx, gy = ax1 - ax0, ay1 - ay0
            glen2 = gx * gx + gy * gy
            t = ((u - ax0) * gx + (v - ay0) * gy) / glen2
            t = np.clip(t, 0.0, 1.0)
            rgb = np.zeros((hi, hi, 3), dtype=np.float64)
            for (t0, c0), (t1, c1) in zip(stops, stops[1:]):
                seg = (t >= t0) & (t <= t1)
                f = np.where(seg, (t - t0) / max(t1 - t0, 1e-9), 0.0)
                c_a, c_b = hex_rgb(c0), hex_rgb(c1)
                for ch in range(3):
                    rgb[:, :, ch] = np.where(
                        seg, c_a[ch] + (c_b[ch] - c_a[ch]) * f, rgb[:, :, ch])
        alpha = mask.astype(np.float64) * 255.0
        a_prev = canvas[:, :, 3:4] / 255.0
        rgb_new = rgb * (alpha / 255.0)[:, :, None] + canvas[:, :, :3] * a_prev * (1 - alpha / 255.0)[:, :, None]
        a_new = alpha + canvas[:, :, 3] * (1 - alpha / 255.0)
        canvas = np.dstack([rgb_new, a_new])

    img = Image.fromarray(np.clip(canvas, 0, 255).astype(np.uint8), "RGBA")
    if hi != size:
        img = img.resize((size, size), Image.LANCZOS)
    return img


# ------------------------------------------------------------ tile renderer

def render_tile(size, style, bg):
    """RGBA tile layer (rounded square, transparent outside)."""
    ss = 4 if size <= 512 else 2
    hi = size * ss
    side = style["tile"] * hi
    radius = style["radius"] * side
    off = (hi - side) / 2
    img = Image.new("RGBA", (hi, hi), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    if side > 0:
        d.rounded_rectangle([off, off, off + side, off + side],
                            radius=radius, fill=bg)
    if hi != size:
        img = img.resize((size, size), Image.LANCZOS)
    return img


def render_icon(size, style, bg, mono=None):
    tile = render_tile(size, style, bg)
    side = style["tile"] * size
    mark_side = int(round(MARK_FRAC * side))
    mark = render_mark(mark_side, mono=mono)
    off = int(round((size - mark_side) / 2))
    tile.alpha_composite(mark, (off, off))
    return tile


def save(img, path):
    path.parent.mkdir(parents=True, exist_ok=True)
    img.save(path)
    print(f"  {path.relative_to(OUT_ROOT)}  {img.size[0]}x{img.size[1]}")


# ------------------------------------------------------------------- output

def write_svgs(out_dir):
    def stroke(el, fill):
        (x0, y0), (x1, y1) = SHAPES[el]
        return (f'<line x1="{x0}" y1="{y0}" x2="{x1}" y2="{y1}" '
                f'stroke="{fill}" stroke-width="{S}" stroke-linecap="round"/>')

    def circle_el(fill):
        (cx, cy), r = DOT
        return f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="{fill}"/>'

    def gradient_defs():
        defs = []
        for name in DRAW_ORDER:
            (ax0, ay0), (ax1, ay1), stops = GRADIENTS[name]
            stop_tags = "".join(
                f'<stop offset="{t:g}" stop-color="{c}"/>' for t, c in stops)
            defs.append(
                f'<linearGradient id="g-{name}" gradientUnits="userSpaceOnUse" '
                f'x1="{ax0}" y1="{ay0}" x2="{ax1}" y2="{ay1}">{stop_tags}</linearGradient>')
        return "<defs>" + "".join(defs) + "</defs>"

    grad_marks = {n: f"url(#g-{n})" for n in DRAW_ORDER}
    mono_marks = {n: "currentColor" for n in DRAW_ORDER}

    def mark_group(fills):
        return "".join(
            stroke(n, fills[n]) if n != "dot" else circle_el(fills[n])
            for n in DRAW_ORDER)

    svgs = {
        # bare mark, brand gradient — for inline/embedded use
        "neomind-mark.svg":
            f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">'
            f"{gradient_defs()}{mark_group(grad_marks)}</svg>",
        # bare mark, single color via currentColor
        "neomind-mark-mono.svg":
            f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">'
            f"{mark_group(mono_marks)}</svg>",
    }
    for name, (bg, mono) in {
        "icon-dark-color.svg": (TILE_DARK, None),
        "icon-light-color.svg": (TILE_LIGHT, None),
        "icon-dark-mono.svg": (TILE_DARK, MONO_ON_DARK),
        "icon-light-mono.svg": (TILE_LIGHT, MONO_ON_LIGHT),
    }.items():
        if mono:
            fills = {n: mono for n in DRAW_ORDER}
            inner = mark_group(fills)
        else:
            inner = gradient_defs() + mark_group(grad_marks)
        rx = 22.5
        svgs[name] = (
            f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">'
            f'<rect width="100" height="100" rx="{rx}" fill="{bg}"/>'
            f'<g transform="translate({50 * (1 - MARK_FRAC):.1f} '
            f'{50 * (1 - MARK_FRAC):.1f}) scale({MARK_FRAC})">{inner}</g></svg>')

    for fname, content in svgs.items():
        p = out_dir / fname
        p.write_text(content)
        print(f"  {p.relative_to(OUT_ROOT)}")


def emit_web_icons(pub):
    print("web/public + API static:")
    web = [
        ("logo-square.png", 128, STYLE_WEB),
        ("favicon-16x16.png", 16, STYLE_WEB),
        ("favicon-32x32.png", 32, STYLE_WEB),
        ("apple-touch-icon.png", 180, STYLE_WEB),
        ("icon-192.png", 192, STYLE_WEB),
        ("icon-512.png", 512, STYLE_WEB),
    ]
    for fname, size, style in web:
        img = render_icon(size, style, TILE_DARK)
        save(img, pub / fname)
        save(img, api_static_dir / fname)
    # transparent-background mark for in-app surfaces (sidebar rail, etc.);
    # display size is capped by the consuming components (w-6 h-6 etc.)
    for fname, size in (("logo-mark.png", 512),):
        img = render_mark(size)
        save(img, pub / fname)
        save(img, api_static_dir / fname)
    # black mono mark (current-color SVG) for light-theme in-app surfaces
    mono_svg = (Path(__file__).resolve().parent / "neomind-mark-mono.svg").read_text()
    (pub / "logo-mark-mono.svg").write_text(mono_svg)
    print(f"  {pub / 'logo-mark-mono.svg'}")


def emit_tauri_icons(icons):
    print("tauri icons (PNG):")
    padded = [
        ("32x32.png", 32), ("64x64.png", 64), ("128x128.png", 128),
        ("128x128@2x.png", 256), ("256x256.png", 256), ("512x512.png", 512),
        ("icon.png", 512), ("master-80.png", 1024),
        ("Square30x30Logo.png", 30), ("Square44x44Logo.png", 44),
        ("Square71x71Logo.png", 71),
        ("Square89x89Logo.png", 89), ("Square107x107Logo.png", 107),
        ("Square142x142Logo.png", 142), ("Square150x150Logo.png", 150),
        ("Square284x284Logo.png", 284), ("Square310x310Logo.png", 310),
        ("StoreLogo.png", 50),
    ]
    for fname, size in padded:
        fname = fname.split(" := ")[-1]
        save(render_icon(size, STYLE_DESKTOP, TILE_DARK), icons / fname)
    save(render_icon(1024, STYLE_SOURCE, TILE_DARK), icons / "icon-source-rounded.png")

    print("ios:")
    for p in sorted((icons / "ios").glob("AppIcon-*.png")):
        size = Image.open(p).size[0]
        save(render_icon(size, STYLE_IOS, TILE_LIGHT), p)

    print("android:")
    dens = {"mdpi": 1, "hdpi": 1.5, "xhdpi": 2, "xxhdpi": 3, "xxxhdpi": 4}
    for d, k in dens.items():
        for name, style in (("ic_launcher.png", STYLE_ANDROID_LAUNCHER),
                            ("ic_launcher_round.png", STYLE_ANDROID_ROUND),
                            ("ic_launcher_foreground.png", STYLE_ANDROID_FOREGROUND)):
            base = {"ic_launcher.png": 48, "ic_launcher_round.png": 48,
                    "ic_launcher_foreground.png": 108}[name]
            size = int(base * k)
            save(render_icon(size, style, TILE_DARK),
                 icons / "android" / f"mipmap-{d}" / name)


def emit_ico_icns(icons):
    print("ico/icns:")
    master = render_icon(256, STYLE_SOURCE, TILE_DARK)
    with tempfile.TemporaryDirectory() as td:
        td = Path(td)
        sizes = [16, 24, 32, 48, 64, 128, 256]
        frames = [master.resize((s, s), Image.LANCZOS) for s in sizes]
        frames[-1].save(td / "icon.ico", sizes=[(s, s) for s in sizes])
        (icons / "icon.ico").write_bytes((td / "icon.ico").read_bytes())
        iconset = td / "icon.iconset"
        iconset.mkdir()
        for s in [16, 32, 64, 128, 256, 512, 1024]:
            render_icon(s, STYLE_SOURCE, TILE_DARK).save(iconset / f"icon_{s}x{s}.png")
            if s <= 512:
                render_icon(s, STYLE_SOURCE, TILE_DARK).save(
                    iconset / f"icon_{s}x{s}@2x.png")
        subprocess.run(["iconutil", "-c", "icns", str(iconset),
                        "-o", str(icons / "icon.icns")], check=True)
    print(f"  {icons / 'icon.ico'}")
    print(f"  {icons / 'icon.icns'}")


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--out-root", type=Path, default=OUT_ROOT)
    args = ap.parse_args()
    OUT_ROOT = args.out_root.resolve()

    api_static_dir = OUT_ROOT / "crates" / "neomind-api" / "static"
    brand_dir = Path(__file__).resolve().parent

    print("SVG sources:")
    write_svgs(brand_dir)
    emit_web_icons(OUT_ROOT / "web" / "public")
    emit_tauri_icons(OUT_ROOT / "web" / "src-tauri" / "icons")
    emit_ico_icns(OUT_ROOT / "web" / "src-tauri" / "icons")
    print("done.")
