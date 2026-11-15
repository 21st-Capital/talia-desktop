#!/usr/bin/env python3
"""Builds talia-icons.ttf, the one icon font of Talia Desktop (decided 2026-11-14).

The icons are the Talia iPhone app's: the Tabler outline set (MIT, stroke 2 on
a 24 grid; see LICENSE-tabler-icons.txt), cut down to the glyphs icon.rs uses,
plus the app's own drawn marks (the key, the vault, the refresh arrow with its
head at the top left, and the recovery mark: three arrows chasing round a
circle) and three solid Tabler glyphs.

    python3 build_font.py <unpacked @tabler/icons-webfont package> [out.ttf]

Needs fontTools. Every glyph is enlarged by SCALE about the middle of the icon
box, so an icon takes about the room the Bootstrap glyph before it took and no
layout moves.
"""
import math
import re
import sys
from pathlib import Path

from fontTools import subset
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont
from fontTools.ttLib.tables import ttProgram

FAMILY = "talia-icons"
EM = 1000
ASCENT = 900  # Tabler's icon box runs from -100 to 900.
MID = (500.0, 400.0)
SCALE = 1.1

# Tabler outline glyphs, kept at Tabler's own codepoints.
OUTLINE = """
x arrow-down arrow-up arrow-right corner-down-right chevron-right chevron-left
chevron-down chevron-up plug download wallet currency-bitcoin box user
info-circle plus minus alert-triangle shield lock cpu trash network check
file-export file-import tool link clipboard usb device-usb server login-2
list-details building coins settings clock pencil qrcode copy repeat history
circle-check circle-x scale scan
""".split()

# Solid Tabler glyphs, moved to codepoints of our own.
FILLED = {"clock": 0xE010, "alert-triangle": 0xE011}

# Our own marks.
KEY, VAULT, REFRESH, DOT, RECOVERY = 0xE000, 0xE001, 0xE002, 0xE003, 0xE004


def codepoints(css):
    pairs = re.findall(r'\.ti-([a-z0-9-]+):before\s*\{\s*content:\s*"\\([0-9a-f]+)"', css)
    return {name: int(code, 16) for name, code in pairs}


class Draw:
    """Closed shapes on an SVG view box (y down), written as TrueType contours."""

    def __init__(self, view):
        self.pen = TTGlyphPen(None)
        self.unit = EM / view

    def _pt(self, x, y):
        return (round(x * self.unit), round(ASCENT - y * self.unit))

    def _path(self, points, hole):
        # points: ("L", x, y) or ("Q", cx, cy, x, y), clockwise on screen.
        if hole:
            rev, n = [], len(points)
            ends = [p[-2:] for p in points]
            for i in range(n - 1, -1, -1):
                prev = ends[i - 1]
                if points[i][0] == "Q":
                    rev.append(("Q", points[i][1], points[i][2], *prev))
                else:
                    rev.append(("L", *prev))
            start, points = ends[-1], rev
        else:
            start = points[-1][-2:]
        self.pen.moveTo(self._pt(*start))
        for p in points:
            if p[0] == "L":
                self.pen.lineTo(self._pt(p[1], p[2]))
            else:
                self.pen.qCurveTo(self._pt(p[1], p[2]), self._pt(p[3], p[4]))
        self.pen.closePath()

    @staticmethod
    def _arc(cx, cy, r, a0, a1, steps):
        out, step = [], (a1 - a0) / steps
        for i in range(steps):
            m, e = a0 + step * (i + 0.5), a0 + step * (i + 1)
            k = r / math.cos(step / 2)
            out.append(("Q", cx + k * math.cos(m), cy + k * math.sin(m),
                        cx + r * math.cos(e), cy + r * math.sin(e)))
        return out

    def disc(self, cx, cy, r, hole=False):
        self._path(self._arc(cx, cy, r, 0, 2 * math.pi, 8), hole)

    def ring(self, cx, cy, r, width):
        self.disc(cx, cy, r + width / 2)
        self.disc(cx, cy, r - width / 2, hole=True)

    def round_rect(self, x, y, w, h, r, hole=False):
        q = math.pi / 2
        pts = [("L", x + w - r, y)] + self._arc(x + w - r, y + r, r, -q, 0, 2)
        pts += [("L", x + w, y + h - r)] + self._arc(x + w - r, y + h - r, r, 0, q, 2)
        pts += [("L", x + r, y + h)] + self._arc(x + r, y + h - r, r, q, 2 * q, 2)
        pts += [("L", x, y + r)] + self._arc(x + r, y + r, r, 2 * q, 3 * q, 2)
        self._path(pts, hole)

    def rect(self, x, y, w, h):
        self._path([("L", x + w, y), ("L", x + w, y + h), ("L", x, y + h), ("L", x, y)], False)

    def line(self, x1, y1, x2, y2, width, round_caps=False):
        """A horizontal or vertical stroke."""
        half = width / 2
        x, y = min(x1, x2) - half, min(y1, y2) - half
        w, h = abs(x2 - x1) + width, abs(y2 - y1) + width
        if round_caps:
            self.round_rect(x, y, w, h, half)
        elif x1 == x2:
            self.rect(x, y + half, w, h - width)
        else:
            self.rect(x + half, y, w - width, h)

    def polygon(self, pts):
        """A straight-edged shape, turned clockwise on screen whatever its order."""
        area = sum(x1 * y2 - x2 * y1 for (x1, y1), (x2, y2) in zip(pts, pts[1:] + pts[:1]))
        if area < 0:
            pts = pts[::-1]
        self._path([("L", x, y) for x, y in pts], False)

    def stroke(self, x1, y1, x2, y2, width):
        """A straight stroke in any direction, with round caps."""
        length = math.hypot(x2 - x1, y2 - y1)
        nx, ny = -(y2 - y1) / length * width / 2, (x2 - x1) / length * width / 2
        self.polygon([(x1 + nx, y1 + ny), (x2 + nx, y2 + ny), (x2 - nx, y2 - ny), (x1 - nx, y1 - ny)])
        self.disc(x1, y1, width / 2)
        self.disc(x2, y2, width / 2)

    def arc_stroke(self, cx, cy, r, width, a0, a1):
        """A stroke along a circle from angle a0 to a1 (radians, clockwise on
        screen), with a round cap at its start."""
        ro, ri = r + width / 2, r - width / 2
        pts = [("L", cx + ro * math.cos(a0), cy + ro * math.sin(a0))]
        pts += self._arc(cx, cy, ro, a0, a1, 4)
        pts += [("L", cx + ri * math.cos(a1), cy + ri * math.sin(a1))]
        pts += self._arc(cx, cy, ri, a1, a0, 4)
        self._path(pts, False)
        self.disc(cx + r * math.cos(a0), cy + r * math.sin(a0), width / 2)

    def glyph(self):
        g = self.pen.glyph()
        g.program = ttProgram.Program()
        g.program.fromBytecode(b"")
        # The strokes overlap: tell the rasterizer (OVERLAP_SIMPLE).
        g.flags[0] |= 0x40
        return g


def key_glyph():
    """KeyIcon.tsx, "THE key icon" of the app: a bow, a shaft and two teeth."""
    d = Draw(24)
    d.ring(8, 12, 4.5, 2.4)
    d.line(12.5, 12, 20.5, 12, 2.4, round_caps=True)
    d.line(17, 12, 17, 16, 2.4, round_caps=True)
    d.line(20.5, 12, 20.5, 15, 2.4, round_caps=True)
    return d.glyph()


def vault_glyph():
    """VaultIcon.tsx (assets/tabs/vault.svg), the app's Vault tab: a safe."""
    d = Draw(48)
    d.round_rect(4.5, 4.5, 39, 35, 5.5)
    d.round_rect(7.5, 7.5, 33, 29, 2.5, hole=True)
    d.ring(22, 22, 9, 3)
    d.disc(22, 22, 2)
    d.line(22, 13, 22, 17, 2.5)
    d.line(22, 27, 22, 31, 2.5)
    d.line(13, 22, 17, 22, 2.5)
    d.line(27, 22, 31, 22, 2.5)
    d.line(37, 17, 37, 27, 3, round_caps=True)
    d.rect(10, 38, 7, 4)
    d.rect(31, 38, 7, 4)
    return d.glyph()


def recovery_glyph():
    """Recovery: three arrows chasing round a circle (RK 2026-11-14: arrows
    like the "process" mark, without the gear in the middle)."""
    d = Draw(24)
    cx = cy = 12
    r, width, arm = 8.4, 2.0, 3.6
    for start in (190, 310, 70):
        a0, a1 = math.radians(start), math.radians(start + 92)
        d.arc_stroke(cx, cy, r, width, a0, a1)
        tip = (cx + r * math.cos(a1), cy + r * math.sin(a1))
        back = (math.sin(a1), -math.cos(a1))  # against the clockwise motion
        for turn in (math.radians(45), math.radians(-45)):
            bx = back[0] * math.cos(turn) - back[1] * math.sin(turn)
            by = back[0] * math.sin(turn) + back[1] * math.cos(turn)
            d.stroke(tip[0], tip[1], tip[0] + arm * bx, tip[1] + arm * by, width)
    return d.glyph()


def dot_glyph():
    """A solid dot that fills the em, as Bootstrap's circle-fill did."""
    d = Draw(24)
    d.disc(12, 12, 10.9)
    return d.glyph()


def add(font, name, glyph, code):
    font.setGlyphOrder(font.getGlyphOrder() + [name])
    font["glyf"][name] = glyph
    font["hmtx"][name] = (EM, 0)
    for table in font["cmap"].tables:
        if table.isUnicode():
            table.cmap[code] = name


def main():
    package = Path(sys.argv[1])
    out = Path(sys.argv[2]) if len(sys.argv) > 2 else Path(__file__).with_name("talia-icons.ttf")
    dist = package / "dist"
    names = codepoints((dist / "tabler-icons.css").read_text())
    filled_names = codepoints((dist / "tabler-icons-filled.css").read_text())

    font = TTFont(dist / "fonts" / "tabler-icons.ttf")
    options = subset.Options()
    options.layout_features = []
    options.glyph_names = True
    options.notdef_outline = False
    options.name_IDs = [1, 2, 3, 4, 5, 6]
    options.drop_tables += ["GSUB"]
    cut = subset.Subsetter(options)
    cut.populate(unicodes=[names[n] for n in OUTLINE])
    cut.subset(font)

    solid = TTFont(dist / "fonts" / "tabler-icons-filled.ttf")
    solid_cmap, solid_glyf = solid.getBestCmap(), solid["glyf"]
    for name, code in FILLED.items():
        glyph = solid_glyf[solid_cmap[filled_names[name]]]
        glyph.expand(solid_glyf)
        assert glyph.numberOfContours > 0, name
        add(font, name + "-filled", glyph, code)

    glyf = font["glyf"]
    # The app's RefreshIcon is Tabler's rotate-clockwise upside down.
    whole = TTFont(dist / "fonts" / "tabler-icons.ttf")
    refresh = TTGlyphPen(None)
    whole.getGlyphSet()[whole.getBestCmap()[names["rotate-clockwise"]]].draw(refresh)
    refresh = refresh.glyph()
    refresh.coordinates.scale((1, -1))
    refresh.coordinates.translate((0, 2 * MID[1]))
    add(font, "talia-refresh", refresh, REFRESH)

    for name in font.getGlyphOrder():
        glyph = glyf[name]
        if glyph.numberOfContours == 0:
            continue
        assert glyph.numberOfContours > 0, f"composite glyph {name}"
        glyph.expand(glyf)
        glyph.coordinates.translate((-MID[0], -MID[1]))
        glyph.coordinates.scale((SCALE, SCALE))
        glyph.coordinates.translate(MID)
        glyph.coordinates.toInt()

    add(font, "talia-key", key_glyph(), KEY)
    add(font, "talia-vault", vault_glyph(), VAULT)
    add(font, "talia-recovery", recovery_glyph(), RECOVERY)
    for name in ("talia-key", "talia-vault", "talia-recovery"):
        glyph = glyf[name]
        glyph.coordinates.translate((-MID[0], -MID[1]))
        glyph.coordinates.scale((SCALE, SCALE))
        glyph.coordinates.translate(MID)
        glyph.coordinates.toInt()
    add(font, "talia-dot", dot_glyph(), DOT)

    for name in font.getGlyphOrder():
        glyph = glyf[name]
        glyph.recalcBounds(glyf)
        font["hmtx"][name] = (EM, getattr(glyph, "xMin", 0) if glyph.numberOfContours else 0)

    for record in font["name"].names:
        if record.nameID in (1, 4):
            record.string = FAMILY
        elif record.nameID == 6:
            record.string = FAMILY
        elif record.nameID == 3:
            record.string = FAMILY + " (Tabler Icons subset, MIT)"
    font["maxp"].numGlyphs = len(font.getGlyphOrder())
    font.save(out)
    print(out, out.stat().st_size, "bytes,", len(font.getGlyphOrder()), "glyphs")


if __name__ == "__main__":
    main()
