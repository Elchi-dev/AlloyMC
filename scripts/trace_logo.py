"""Recreates the Alloy logo 1:1 from the original artwork as a clean, animatable SVG.

    python3 scripts/trace_logo.py <logo.jpg>

1. Snap every pixel to the artwork's six flat colours (removes JPEG noise).
2. Trace with vtracer (polygon mode), then straighten edges with Ramer-Douglas-Peucker.
3. Recolour copper -> gold, keep steel, drop the background so the seam gap is real.
4. Tag the two halves (.alloy-gold / .alloy-steel) so the UI can animate them.
"""

import math
import pathlib
import re
import sys

import vtracer
from PIL import Image, ImageFilter

ROOT = pathlib.Path(__file__).resolve().parent.parent

SOURCE = {
    "bg": (0x27, 0x27, 0x2A),
    "top": (0xDE, 0x7E, 0x4A),
    "left": (0xB7, 0x58, 0x28),
    "side": (0x89, 0x41, 0x20),
    "steel_top": (0x8D, 0x96, 0x9D),
    "steel_right": (0x69, 0x72, 0x79),
}
# Same lighting, gold instead of copper.
RECOLOR = {
    "top": "#f8c95a",
    "left": "#d99b22",
    "side": "#9c6a10",
    "steel_top": "#8d969d",
    "steel_right": "#697279",
}
GOLD_PARTS = {"top", "left", "side"}


def nearest(rgb):
    return min(SOURCE, key=lambda k: sum((a - b) ** 2 for a, b in zip(SOURCE[k], rgb)))


def rdp(points, eps):
    if len(points) < 3:
        return points
    (x1, y1), (x2, y2) = points[0], points[-1]
    dx, dy = x2 - x1, y2 - y1
    norm = math.hypot(dx, dy) or 1.0
    best, idx = 0.0, 0
    for i in range(1, len(points) - 1):
        px, py = points[i]
        d = abs(dy * px - dx * py + x2 * y1 - y2 * x1) / norm
        if d > best:
            best, idx = d, i
    if best > eps:
        return rdp(points[: idx + 1], eps)[:-1] + rdp(points[idx:], eps)
    return [points[0], points[-1]]


def simplify_ring(ring, eps):
    # Split the closed ring at its farthest point pair so RDP keeps true corners.
    if len(ring) < 4:
        return ring
    far = max(range(len(ring)), key=lambda i: math.dist(ring[0], ring[i]))
    a = rdp(ring[: far + 1], eps)
    b = rdp(ring[far:] + [ring[0]], eps)
    return a[:-1] + b[:-1]


def parse_rings(d):
    rings = []
    for sub in re.findall(r"M[^M]+", d):
        nums = list(map(float, re.findall(r"-?\d+\.?\d*", sub)))
        rings.append(list(zip(nums[0::2], nums[1::2])))
    return rings


def main(src: str):
    im = Image.open(src).convert("RGB")
    s = im.width / 2000  # the artwork was composed at 2000 px wide
    im = im.crop((int(690 * s), int(215 * s), int(1300 * s), int(900 * s))).filter(ImageFilter.MedianFilter(5))
    px = im.load()
    for y in range(im.height):
        for x in range(im.width):
            px[x, y] = SOURCE[nearest(px[x, y])]
    tmp = pathlib.Path("/tmp/alloy-logo-clean.png")
    im.save(tmp)
    traced = pathlib.Path("/tmp/alloy-logo-traced.svg")
    vtracer.convert_image_to_svg_py(
        str(tmp), str(traced), colormode="color", hierarchical="cutout", mode="polygon",
        filter_speckle=8, color_precision=8, layer_difference=8, corner_threshold=60,
        length_threshold=4, splice_threshold=45, path_precision=2,
    )
    raw = traced.read_text()
    paths = re.findall(r'<path d="([^"]+)" fill="(#[0-9A-Fa-f]{6})"(?: transform="translate\(([-\d.]+),([-\d.]+)\)")?', raw)

    gold, steel = [], []
    xs, ys = [], []
    for d, fill, tx, ty in paths:
        r, g, b = (int(fill[i : i + 2], 16) for i in (1, 3, 5))
        part = nearest((r, g, b))
        if part == "bg":
            continue
        ox, oy = float(tx or 0), float(ty or 0)
        out = []
        for ring in parse_rings(d):
            ring = [(x + ox, y + oy) for x, y in ring]
            ring = simplify_ring(ring, 2.2)
            xs += [p[0] for p in ring]
            ys += [p[1] for p in ring]
            out.append("M" + " L".join(f"{x:.1f},{y:.1f}" for x, y in ring) + " Z")
        el = f'<path d="{" ".join(out)}" fill="{RECOLOR[part]}" fill-rule="evenodd" data-face="{part}"/>'
        (gold if part in GOLD_PARTS else steel).append(el)

    minx, miny, maxx, maxy = min(xs), min(ys), max(xs), max(ys)
    pad = 8
    w, h = maxx - minx + 2 * pad, maxy - miny + 2 * pad
    vb = f"{minx - pad:.0f} {miny - pad:.0f} {w:.0f} {h:.0f}"
    svg = (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{vb}" role="img" aria-label="Alloy">'
        f'<g class="alloy-gold">{"".join(gold)}</g><g class="alloy-steel">{"".join(steel)}</g></svg>'
    )
    assets = ROOT / "src" / "assets"
    assets.mkdir(parents=True, exist_ok=True)
    (assets / "logo.svg").write_text(svg)

    # App icon: the same mark centred on the charcoal tile from the artwork.
    size = 1024
    scale = size * 0.62 / max(w, h)
    icon = (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}">'
        f'<rect width="{size}" height="{size}" rx="{size * 0.22:.0f}" fill="#27272a"/>'
        f'<g transform="translate({size / 2:.1f} {size / 2:.1f}) scale({scale:.4f}) '
        f'translate({-(minx + maxx) / 2:.1f} {-(miny + maxy) / 2:.1f})">{"".join(gold)}{"".join(steel)}</g></svg>'
    )
    (ROOT / "src-tauri" / "icon-source.svg").write_text(icon)
    print(f"logo: {len(gold)} gold + {len(steel)} steel paths, viewBox {vb}")


if __name__ == "__main__":
    main(sys.argv[1])
