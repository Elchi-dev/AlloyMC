"""Recreates the Alloy forge background 1:1 from the original artwork.

    python3 scripts/trace_background.py assets-src/background.png

- Edges and shapes: traced to vectors (vtracer), rendered at 3840 px so it is sharp on 4K.
- Atmosphere (smoke, glow on the rock): the original's low-frequency light, added back as a
  blurred residual. Blurry by nature, so it looks identical at any resolution.
- Recolour: the monument's copper half becomes gold, like the logo. Molten metal stays molten.
- Animated parts (molten streams, seam, brazier flame) are also written to an overlay SVG
  with classes the UI animates (src/components/Backdrop.tsx).

Outputs: src/assets/forge.webp, forge-blur.webp, forge-overlay.svg
"""

import colorsys
import io
import pathlib
import re
import sys

import cairosvg
import numpy as np
import vtracer
from PIL import Image, ImageFilter

from trace_logo import parse_rings, simplify_ring

ROOT = pathlib.Path(__file__).resolve().parent.parent
UP = 3  # tracing scale
OUT_W = 3840
GAMMA = 0.55  # lift shadows before tracing so dark rock separates

# Regions in source pixels (1024 x 434)
MONUMENT = (350, 80, 545, 352)
BRAZIER = (795, 285, 845, 325)


def hsv(hex6):
    r, g, b = (int(hex6[i : i + 2], 16) / 255 for i in (0, 2, 4))
    return colorsys.rgb_to_hsv(r, g, b)


def to_hex(h, s, v):
    r, g, b = colorsys.hsv_to_rgb(h % 1.0, max(0, min(1, s)), max(0, min(1, v)))
    return "#%02x%02x%02x" % (round(r * 255), round(g * 255), round(b * 255))


def is_molten(h, s, v):
    return s > 0.55 and v > 0.55 and (h < 0.12 or h > 0.97)


def inside(box, x, y):
    return box[0] * UP <= x <= box[2] * UP and box[1] * UP <= y <= box[3] * UP


def path_anchor(d):
    m = re.search(r"M\s*(-?[\d.]+)[ ,](-?[\d.]+)", d)
    return (float(m.group(1)), float(m.group(2))) if m else (0.0, 0.0)


def path_anchor_abs(el):
    d = re.search(r' d="([^"]+)"', el).group(1)
    x, y = path_anchor(d)
    t = re.search(r'translate\(([-\d.]+),([-\d.]+)\)', el)
    if t:
        x += float(t.group(1))
        y += float(t.group(2))
    return x, y


def recolor(fill, d, tx, ty):
    h, s, v = hsv(fill)
    x, y = path_anchor(d)
    x += tx
    y += ty
    if inside(MONUMENT, x, y) and 0.01 < h < 0.11 and s > 0.45 and not (v > 0.85 and s > 0.5 and h > 0.05):
        # copper face -> gold face, same light
        return to_hex(0.102, min(1.0, s * 1.05), min(1.0, v * 1.12)), None
    if is_molten(h, s, v):
        kind = "flame" if inside(BRAZIER, x, y) else "molten"
        return to_hex(h + 0.012, s, v), kind
    return "#" + fill, None


MON_CROP = (335, 60, 650, 365)  # source px around the monument


def monument_layer(original):
    """The monument as straight-edged polygons (like the logo), drawn over the traced scene.
    Pixels are snapped to the monument's flat colours first, so edges come out as lines."""
    x0, y0, x1, y1 = MON_CROP
    crop = original.crop(MON_CROP)
    # flat colours of the monument, measured from the artwork
    palette = {
        "side": (0x2D, 0x11, 0x09),     # copper, shadowed left side
        "front": (0x78, 0x2F, 0x0F),    # copper, front face
        "seam_hot": (0xFF, 0x8C, 0x22), # molten, bright
        "seam": (0xD8, 0x5A, 0x14),     # molten, deeper
        "seam_dark": (0x8C, 0x2E, 0x0E),
        "steel": (0x43, 0x4C, 0x53),
        "steel_side": (0x38, 0x3D, 0x44),
        "dark": (0x0E, 0x0F, 0x13),
        "feet": (0x1E, 0x22, 0x24),
    }
    keys = list(palette)
    arr = np.asarray(crop).astype(np.int32)
    cols = np.array([palette[k] for k in keys])
    idx = ((arr[:, :, None, :] - cols[None, None, :, :]) ** 2).sum(-1).argmin(-1)
    snapped = cols[idx].astype(np.uint8)
    big = Image.fromarray(snapped).resize((crop.width * 4, crop.height * 4), Image.NEAREST).filter(ImageFilter.ModeFilter(9))
    big.save("/tmp/alloy-mon.png")
    vtracer.convert_image_to_svg_py(
        "/tmp/alloy-mon.png", "/tmp/alloy-mon.svg", colormode="color", hierarchical="stacked", mode="polygon",
        filter_speckle=40, color_precision=8, layer_difference=4, corner_threshold=60, length_threshold=6,
        splice_threshold=45, path_precision=1,
    )
    raw = pathlib.Path("/tmp/alloy-mon.svg").read_text()
    gold = {"side": "#433006", "front": "#a9790f"}  # same light, gold
    out, hot = [], []
    pat = re.compile(r'<path d="([^"]+)" fill="#([0-9A-Fa-f]{6})"(?: transform="translate\(([-\d.]+),([-\d.]+)\)")?/>')
    for d, fill, tx, ty in pat.findall(raw):
        rgb = tuple(int(fill[i : i + 2], 16) for i in (0, 2, 4))
        key = keys[int(np.argmin(((cols - np.array(rgb)) ** 2).sum(-1)))]
        if key in ("dark",) or re.match(r"M0,0 L\d+,0 L\d+,\d+ L0,\d+ Z", d.strip()):
            continue  # the tracer's full-crop backdrop and the cave: let the traced scene show
        color = gold.get(key, "#%02x%02x%02x" % palette[key])
        # straighten the pixel staircase into the artwork's real lines (1.6 source px tolerance)
        rings = [simplify_ring(r, 6.5) for r in parse_rings(d)]
        d = " ".join("M" + " L".join(f"{x:.1f},{y:.1f}" for x, y in r) + " Z" for r in rings if len(r) >= 3)
        # source px * UP, from 4x crop space
        k = UP / 4
        t = f'translate({x0 * UP + float(tx or 0) * k:.2f},{y0 * UP + float(ty or 0) * k:.2f}) scale({k})'
        el = f'<path d="{d}" fill="{color}" transform="{t}"/>'
        out.append(el)
        if key.startswith("seam") and key != "seam_dark":
            hot.append(el.replace("<path ", '<path class="bg-molten" ', 1))
    return out, hot


def main(src):
    original = Image.open(src).convert("RGB")
    w0, h0 = original.size
    up = original.resize((w0 * UP, h0 * UP), Image.LANCZOS)
    a = np.asarray(up).astype(np.float32) / 255.0
    Image.fromarray((np.power(a, GAMMA) * 255).clip(0, 255).astype(np.uint8)).save("/tmp/alloy-bg-lift.png")
    vtracer.convert_image_to_svg_py(
        "/tmp/alloy-bg-lift.png", "/tmp/alloy-bg.svg", colormode="color", hierarchical="stacked", mode="spline",
        filter_speckle=11, color_precision=7, layer_difference=6, corner_threshold=60, length_threshold=4,
        max_iterations=10, splice_threshold=45, path_precision=1,
    )
    raw = pathlib.Path("/tmp/alloy-bg.svg").read_text()

    def undo_gamma(hex6):
        r, g, b = (int(hex6[i : i + 2], 16) / 255 for i in (0, 2, 4))
        return "%02x%02x%02x" % tuple(round((c ** (1 / GAMMA)) * 255) for c in (r, g, b))

    base, overlay = [], []
    pat = re.compile(r'<path d="([^"]+)" fill="#([0-9A-Fa-f]{6})"(?: transform="translate\(([-\d.]+),([-\d.]+)\)")?/>')
    for d, fill, tx, ty in pat.findall(raw):
        tx, ty = float(tx or 0), float(ty or 0)
        color, kind = recolor(undo_gamma(fill), d, tx, ty)
        # the crisp monument layer replaces the traced monument faces and seam
        ax, ay = path_anchor(d)
        if inside(MON_CROP, ax + tx, ay + ty) and (kind == "molten" or color != "#" + undo_gamma(fill)):
            continue
        t = f' transform="translate({tx:g},{ty:g})"' if (tx or ty) else ""
        el = f'<path d="{d}" fill="{color}"{t}/>'
        base.append(el)
        if kind:
            overlay.append(f'<path class="bg-{kind}" d="{d}" fill="{color}"{t}/>')

    mon, mon_hot = monument_layer(original)
    # the traced (wobbly) monument's molten bits are replaced by the crisp ones
    overlay = [o for o in overlay if not inside(MON_CROP, *path_anchor_abs(o))] + mon_hot

    W, H = w0 * UP, h0 * UP
    svg = f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}">{"".join(base)}{"".join(mon)}</svg>'

    # Atmosphere: what the vectors miss, as a soft additive layer (computed at source size).
    vec_small = Image.open(io.BytesIO(cairosvg.svg2png(bytestring=svg.encode(), output_width=w0))).convert("RGB")
    vec_small_orig = Image.open(io.BytesIO(cairosvg.svg2png(bytestring=raw.encode(), output_width=w0))).convert("RGB")
    # residual is measured against the un-recoloured trace so gold faces stay gold
    lifted_back = np.power(np.asarray(vec_small_orig).astype(np.float32) / 255.0, 1 / GAMMA)
    residual = np.asarray(original).astype(np.float32) / 255.0 - lifted_back
    residual = np.clip(residual, -0.1, 0.4)
    # The original's copper light on the monument would tint the new gold; drop it there.
    mx0, my0, mx1, my1 = MONUMENT
    residual[my0:my1, mx0 : mx1 - 70] *= 0.15
    res_img = Image.fromarray(((residual + 0.5) * 255).clip(0, 255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(5))

    out_h = round(OUT_W * H / W)
    vec_big = np.asarray(
        Image.open(io.BytesIO(cairosvg.svg2png(bytestring=svg.encode(), output_width=OUT_W))).convert("RGB").resize((OUT_W, out_h))
    ).astype(np.float32) / 255.0
    res_big = np.asarray(res_img.resize((OUT_W, out_h), Image.BICUBIC).filter(ImageFilter.GaussianBlur(6))).astype(np.float32) / 255.0 - 0.5
    final = np.clip(vec_big + res_big, 0, 1)
    final_img = Image.fromarray((final * 255).astype(np.uint8))

    assets = ROOT / "src" / "assets"
    final_img.save(assets / "forge.webp", quality=86, method=6)
    final_img.resize((960, round(960 * H / W)), Image.LANCZOS).filter(ImageFilter.GaussianBlur(10)).save(
        assets / "forge-blur.webp", quality=80
    )
    (assets / "forge-overlay.svg").write_text(
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" preserveAspectRatio="xMidYMid slice">{"".join(overlay)}</svg>'
    )
    final_img.resize((1024, round(1024 * H / W))).save("/tmp/forge-final.png")
    sizes = {p.name: p.stat().st_size // 1024 for p in assets.glob("forge*")}
    print(f"{len(base)} paths, {len(overlay)} animated; sizes KB: {sizes}")


if __name__ == "__main__":
    main(sys.argv[1])
