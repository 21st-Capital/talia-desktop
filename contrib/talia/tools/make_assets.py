"""Generate Talia Desktop logo files for the Liana-based desktop app.

Usage: python3 -I make_assets.py <assets_dir> <out_dir>
Reads talia-icon.svg and the Bricolage fonts from <assets_dir>, writes SVGs to <out_dir>.
"""
import sys
from pathlib import Path

from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont

assets = Path(sys.argv[1])
out = Path(sys.argv[2])
out.mkdir(parents=True, exist_ok=True)

CREAM = "#F4F1E8"
GOLD = "#F2A900"

# The Talia mark, in the 120-unit space of the app's talia_icon.svg.
MARK = (
    f'<g fill="none" stroke="{CREAM}" stroke-width="10" stroke-linecap="round">'
    '<path d="M31 44 h50"/>'
    '<path d="M54 22 v56 a17 17 0 1 0 17 -17"/>'
    "</g>"
    f'<circle cx="93" cy="44" r="6" fill="{GOLD}"/>'
)
# Ink bounds of the mark: x 26..99, y 17..100.
MX0, MY0, MX1, MY1 = 26, 17, 99, 100

# 1. Symbol only (collapsed sidebar).
pad = 2
(out / "TALIA_SYMBOL.svg").write_text(
    f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{MX0 - pad} {MY0 - pad} '
    f'{MX1 - MX0 + 2 * pad} {MY1 - MY0 + 2 * pad}">{MARK}</svg>\n'
)


def text_path(font_file, text, size, x, baseline):
    """Return (svg path d, advance width) for text set at font size `size`."""
    font = TTFont(font_file)
    gs = font.getGlyphSet()
    cmap = font.getBestCmap()
    upm = font["head"].unitsPerEm
    scale = size / upm
    hmtx = font["hmtx"]
    pen = SVGPathPen(gs)
    cursor = 0.0
    for ch in text:
        name = cmap[ord(ch)]
        # flip y (font units are y-up), move to the baseline
        tpen = TransformPen(pen, (scale, 0, 0, -scale, x + cursor * scale, baseline))
        gs[name].draw(tpen)
        cursor += hmtx[name][0]
    return pen.getCommands(), cursor * scale


# 2. Lockup (sidebar and top bar): the knot is the t, "alia" completes the word
#    (RK 2026-11-14). Same geometry as the iPhone app's TaliaLogo variant="lockup"
#    (viewBox 0 0 134 70, strokes 7, "alia" at x 54 / baseline 53, size 40,
#    letter spacing 3) and the splash's Bricolage Grotesque SemiBold.
def spaced_text_path(font_file, text, size, x, baseline, spacing):
    font = TTFont(font_file)
    gs = font.getGlyphSet()
    cmap = font.getBestCmap()
    scale = size / font["head"].unitsPerEm
    hmtx = font["hmtx"]
    pen = SVGPathPen(gs)
    cursor = x
    for ch in text:
        name = cmap[ord(ch)]
        gs[name].draw(TransformPen(pen, (scale, 0, 0, -scale, cursor, baseline)))
        cursor += hmtx[name][0] * scale + spacing
    return pen.getCommands()


alia = spaced_text_path(assets / "Bricolage-SemiBold.ttf", "alia", 40, 54, 53, 3)
lockup = (
    f'<g fill="none" stroke="{CREAM}" stroke-width="7" stroke-linecap="round">'
    '<path d="M8 22 h30"/><path d="M22 8 v34 a10 10 0 1 0 10 -10"/></g>'
    f'<circle cx="45" cy="22" r="4" fill="{GOLD}"/>'
    f'<path d="{alia}" fill="{CREAM}"/>'
)
# Ink spans y 4.5..55.5 of the 134x70 box. Fit it 44 units tall in a 240.8 x 50
# canvas (the width the first version had), left aligned, so the logo keeps
# its size on screen wherever Liana draws its own wide logo.
H = 50.0
W = 240.8
k = 44.0 / 51.0
tx = -4.5 * k + 1
ty = (H - 44.0) / 2 - 4.5 * k
(out / "talia_logo.svg").write_text(
    f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H:.0f}" '
    f'viewBox="0 0 {W} {H:.0f}">'
    f'<g transform="translate({tx:.3f} {ty:.3f}) scale({k:.5f})">{lockup}</g>'
    "</svg>\n"
)

# 3. Desktop app icon: the App Store icon drawn as a macOS tile
#    (824 px rounded square centred on a 1024 canvas, transparent margin).
icon_svg = (assets / "talia-icon.svg").read_text()
inner = icon_svg.split(">", 1)[1].rsplit("</svg>", 1)[0]
tile = 824.0
off = (1024 - tile) / 2
s = tile / 1024
(out / "talia-desktop-icon-1024.svg").write_text(
    '<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">'
    f'<defs><clipPath id="tile"><rect x="{off}" y="{off}" width="{tile}" height="{tile}" rx="185"/></clipPath></defs>'
    f'<g clip-path="url(#tile)"><g transform="translate({off} {off}) scale({s})">{inner}</g></g>'
    "</svg>\n"
)

# 4. Windows / window icon: same tile with Liana's own small margin (960 on 1024).
tile = 960.0
off = (1024 - tile) / 2
s = tile / 1024
(out / "talia-desktop-icon-win.svg").write_text(
    '<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">'
    f'<defs><clipPath id="tile"><rect x="{off}" y="{off}" width="{tile}" height="{tile}" rx="212"/></clipPath></defs>'
    f'<g clip-path="url(#tile)"><g transform="translate({off} {off}) scale({s})">{inner}</g></g>'
    "</svg>\n"
)
