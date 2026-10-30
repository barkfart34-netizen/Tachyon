"""Kinetica's content: textures drawn from palettes, models, blockstates,
items, recipes, loot tables, tags and names, written into
tachyon/content/kinetica (a Horizon content mod, API 0.2).

    python tachyon/art/kinetica.py

Everything here is Tachyon's own: no file comes from Create, whose assets are
not free to copy. Run it again after a change; it rewrites the whole mod.
"""

import json
import math
import random
import shutil
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[1] / "content" / "kinetica"
NS = "kinetica"


# Palettes, dark to light.
IRON = [(38, 40, 46), (62, 66, 75), (92, 97, 108), (128, 134, 146), (170, 176, 186), (214, 219, 226)]
STONE = [(52, 54, 58), (74, 77, 82), (96, 99, 104), (117, 120, 124), (138, 141, 144)]
WOOD = [(58, 38, 22), (86, 58, 33), (114, 79, 45), (139, 99, 58), (165, 122, 74)]
BRONZE = [(70, 42, 20), (112, 70, 30), (156, 104, 44), (196, 142, 66), (230, 186, 104)]
CORE = [(10, 40, 70), (20, 90, 140), (40, 160, 210), (110, 220, 255), (210, 248, 255)]
TIN = [(58, 66, 74), (98, 110, 120), (140, 154, 162), (178, 192, 198), (210, 222, 226), (236, 244, 246)]
COPPER = [(84, 40, 24), (140, 70, 40), (192, 104, 64), (226, 140, 96), (246, 184, 140), (252, 220, 190)]
GOLD = [(96, 64, 8), (160, 112, 20), (214, 164, 40), (240, 206, 80), (252, 236, 140), (255, 250, 210)]
# The ground the ores are in: a stone's greys, a deepslate's darker blue-greys.
ROCK = [(96, 96, 96), (108, 108, 108), (119, 119, 119), (129, 129, 129), (140, 140, 140)]
DEEP = [(48, 48, 54), (58, 58, 64), (69, 69, 75), (80, 80, 86), (92, 92, 98)]


def image():
    return Image.new("RGBA", (16, 16), (0, 0, 0, 0))


def noise(img, palette, rng, lo=1, hi=3, box=(0, 0, 16, 16)):
    x0, y0, x1, y1 = box
    for y in range(y0, y1):
        for x in range(x0, x1):
            img.putpixel((x, y), palette[rng.randint(lo, hi)] + (255,))


def frame(img, palette, box=(0, 0, 16, 16)):
    """A metal frame: dark outer edge, light top-left bevel, dark bottom-right."""
    x0, y0, x1, y1 = box
    for i in range(x0, x1):
        img.putpixel((i, y0), palette[0] + (255,))
        img.putpixel((i, y1 - 1), palette[0] + (255,))
        if x0 < i < x1 - 1:
            img.putpixel((i, y0 + 1), palette[4] + (255,))
            img.putpixel((i, y1 - 2), palette[1] + (255,))
    for j in range(y0, y1):
        img.putpixel((x0, j), palette[0] + (255,))
        img.putpixel((x1 - 1, j), palette[0] + (255,))
        if y0 < j < y1 - 1:
            img.putpixel((x0 + 1, j), palette[4] + (255,))
            img.putpixel((x1 - 2, j), palette[1] + (255,))


def rivets(img, palette, at):
    for x, y in at:
        img.putpixel((x, y), palette[5] + (255,))
        img.putpixel((x + 1, y + 1), palette[0] + (255,))


def casing():
    rng = random.Random(1)
    img = image()
    noise(img, STONE, rng)
    frame(img, IRON)
    rivets(img, IRON, [(2, 2), (12, 2), (2, 12), (12, 12)])
    return img


def casing_hole():
    img = casing()
    for y in range(16):
        for x in range(16):
            d = math.hypot(x - 7.5, y - 7.5)
            if d < 3.2:
                img.putpixel((x, y), IRON[0] + (255,))
            elif d < 4.2:
                img.putpixel((x, y), (IRON[4] if x + y < 15 else IRON[1]) + (255,))
    return img


def shaft_side():
    """A rod along the texture's height: shaded across, bands every 8 pixels."""
    img = image()
    shade = [IRON[1], IRON[3], IRON[4], IRON[2]]
    for y in range(16):
        for x in range(16):
            c = shade[(x - 6) % 4] if 6 <= x < 10 else IRON[2]
            if y % 8 == 7:
                c = IRON[1]
            img.putpixel((x, y), c + (255,))
    return img


def shaft_end():
    img = image()
    for y in range(16):
        for x in range(16):
            d = math.hypot(x - 7.5, y - 7.5)
            c = IRON[4] if d < 1.0 else IRON[3] if d < 2.2 else IRON[1]
            img.putpixel((x, y), c + (255,))
    return img


def planks():
    rng = random.Random(7)
    img = image()
    for y in range(16):
        row = WOOD[2] if (y // 4) % 2 == 0 else WOOD[3]
        for x in range(16):
            c = row
            r = rng.random()
            if r < 0.18:
                c = WOOD[1] if row == WOOD[2] else WOOD[2]
            elif r < 0.25:
                c = WOOD[4]
            if y % 4 == 3:
                c = WOOD[0]
            img.putpixel((x, y), c + (255,))
    return img


def teeth():
    img = image()
    for y in range(16):
        for x in range(16):
            c = WOOD[1] if (x + y) % 5 else WOOD[0]
            if y < 2:
                c = WOOD[3]
            img.putpixel((x, y), c + (255,))
    return img


def motor_side():
    rng = random.Random(3)
    img = image()
    noise(img, IRON, rng, 1, 2)
    frame(img, IRON)
    # A glowing core window across the middle.
    for y in range(5, 11):
        for x in range(3, 13):
            t = abs(y - 7.5) / 3
            c = CORE[4] if t < 0.3 else CORE[3] if t < 0.6 else CORE[2]
            if (x + y) % 4 == 0:
                c = CORE[1]
            img.putpixel((x, y), c + (255,))
    return img


def motor_back():
    rng = random.Random(4)
    img = image()
    noise(img, IRON, rng, 1, 2)
    frame(img, IRON)
    for x in range(4, 12, 2):
        for y in range(4, 12):
            img.putpixel((x, y), IRON[0] + (255,))
    return img


def banded(color):
    """A casing with a band across: a clutch's or a gearshift's sides."""
    def draw():
        img = casing()
        for x in range(2, 14):
            for y in (7, 8):
                img.putpixel((x, y), color[3 if y == 7 else 2] + (255,))
            img.putpixel((x, 6), color[1] + (255,))
            img.putpixel((x, 9), color[0] + (255,))
        return img
    return draw


REDSTONE = [(70, 8, 8), (120, 14, 10), (176, 24, 16), (226, 54, 40), (255, 120, 100)]


def crank_wood():
    rng = random.Random(11)
    img = image()
    for y in range(16):
        for x in range(16):
            c = WOOD[3] if (x // 2) % 2 else WOOD[2]
            if rng.random() < 0.15:
                c = WOOD[1]
            img.putpixel((x, y), c + (255,))
    return img


def canvas():
    """A windmill sail's cloth: off-white weave, a darker seam each side."""
    rng = random.Random(23)
    img = image()
    cloth = [(150, 140, 120), (186, 176, 154), (212, 204, 184), (232, 226, 210)]
    for y in range(16):
        for x in range(16):
            c = cloth[2] if (x + y) % 2 else cloth[3]
            if rng.random() < 0.12:
                c = cloth[1]
            if x in (0, 15) or y in (0, 15):
                c = cloth[0]
            img.putpixel((x, y), c + (255,))
    return img


def goggles():
    """Engineer's goggles: two lenses in a brass frame on a leather strap."""
    img = image()
    rows = [
        "...aaaa..aaaa...",
        "..abbbbaabbbba..",
        "ssabhcbaabhcbass",
        "ssabccbaabccbass",
        "..abbbbaabbbba..",
        "...aaaa..aaaa...",
    ]
    colors = {"a": BRONZE[1], "b": BRONZE[3], "c": CORE[2], "h": CORE[4], "s": WOOD[1]}
    for y, row in enumerate(rows):
        for x, ch in enumerate(row):
            if ch in colors:
                img.putpixel((x, y + 5), colors[ch] + (255,))
    return img


def alloy():
    """An ingot: ferrostone grey, an iron sheen."""
    return ingot({"a": STONE[0], "b": IRON[5], "c": IRON[3], "d": STONE[3], "e": STONE[1]})


def ingot(colors):
    """An ingot: `a` its outline, `b` the sheen, `c` the top, `d` and `e` the shaded sides."""
    img = image()
    rows = [
        "................",
        "................",
        "................",
        "................",
        "......aaaaaaa...",
        ".....abbbbbbca..",
        "....abbbcccbcda.",
        "...abbccccccdda.",
        "..abbccccccddda.",
        ".abbccccccdddea.",
        ".acccccccddddea.",
        ".aeddddddddeea..",
        "..aeeeeeeeeaa...",
        "...aaaaaaaa.....",
        "................",
        "................",
    ]
    for y, row in enumerate(rows):
        for x, ch in enumerate(row):
            if ch in colors:
                img.putpixel((x, y), colors[ch] + (255,))
    return img


def ground(palette, seed, streaks=False):
    """Rock: clumps of its greys, in streaks across for deepslate."""
    rng = random.Random(seed)
    img = image()
    for y in range(16):
        for x in range(16):
            i = rng.choice([1, 2, 2, 3])
            if streaks and (y + rng.randint(0, 1)) % 5 == 0:
                i = 0
            img.putpixel((x, y), palette[i] + (255,))
    for _ in range(10):
        x, y = rng.randrange(15), rng.randrange(15)
        img.putpixel((x, y), palette[4] + (255,))
        img.putpixel((x + 1, y + 1), palette[0] + (255,))
    return img


def ore(palette, seed, metal, streaks=False):
    """Rock with veins of a metal: clumps of two to four pixels, outlined dark."""
    def draw():
        rng = random.Random(seed + 100)
        img = ground(palette, seed, streaks)
        for cx, cy in [(3, 3), (10, 2), (6, 8), (12, 10), (2, 12), (9, 13)]:
            cells = [(cx, cy), (cx + 1, cy), (cx, cy + 1), (cx + 1, cy + 1)][:rng.randint(2, 4)]
            for x, y in cells:
                for dx, dy in [(-1, 0), (1, 0), (0, -1), (0, 1)]:
                    n = (x + dx, y + dy)
                    if 0 <= n[0] < 16 and 0 <= n[1] < 16 and n not in cells:
                        img.putpixel(n, metal[0] + (255,))
            for i, (x, y) in enumerate(cells):
                img.putpixel((x, y), metal[[4, 3, 2, 3][i]] + (255,))
            img.putpixel(cells[0], metal[5] + (255,))
        return img
    return draw


def metal_block(palette, seed):
    """A block of a metal: plates in a frame."""
    def draw():
        rng = random.Random(seed)
        img = image()
        noise(img, palette, rng, 2, 4)
        frame(img, palette)
        for i in range(2, 14):
            img.putpixel((i, 8), palette[1] + (255,))
            img.putpixel((8, i), palette[1] + (255,))
        return img
    return draw


def raw(palette, seed, block=False):
    """Raw metal: lumps, light on top and dark beneath; a block of them."""
    def draw():
        rng = random.Random(seed)
        img = image()
        if block:
            noise(img, palette, rng, 1, 3)
            spots = [(x, y) for x in range(0, 16, 4) for y in range(0, 16, 4)]
        else:
            spots = [(4, 5), (8, 4), (6, 8), (10, 8), (7, 11)]
        for x, y in spots:
            for dx in range(3):
                for dy in range(3):
                    if 0 <= x + dx < 16 and 0 <= y + dy < 16:
                        shade = [4, 3, 2][dy] if dx < 2 else 1
                        img.putpixel((x + dx, y + dy), palette[shade] + (255,))
            if not block:
                img.putpixel((x, y), palette[5] + (255,))
        if not block:
            # The outline round the lumps.
            outline = []
            for y in range(16):
                for x in range(16):
                    if img.getpixel((x, y))[3]:
                        continue
                    near = [(x + dx, y + dy) for dx, dy in [(-1, 0), (1, 0), (0, -1), (0, 1)]]
                    if any(0 <= a < 16 and 0 <= b < 16 and img.getpixel((a, b))[3] and
                           img.getpixel((a, b))[:3] != palette[0] for a, b in near):
                        outline.append((x, y))
            for xy in outline:
                img.putpixel(xy, palette[0] + (255,))
        return img
    return draw


def nugget(palette):
    def draw():
        img = image()
        rows = ["......", "..aa..", ".abba.", "abccda", ".adda.", "..aa.."]
        colors = {"a": palette[0], "b": palette[5], "c": palette[3], "d": palette[2]}
        for y, row in enumerate(rows):
            for x, ch in enumerate(row):
                if ch in colors:
                    img.putpixel((x + 5, y + 5), colors[ch] + (255,))
        return img
    return draw


def millstone_stone():
    """A millstone's side: rough grey stone, a groove round its middle."""
    rng = random.Random(41)
    img = image()
    noise(img, STONE, rng, 2, 4)
    for x in range(16):
        img.putpixel((x, 7), STONE[1] + (255,))
        img.putpixel((x, 8), STONE[0] + (255,))
    return img


def millstone_top():
    """A millstone's top: stone with eight grooves out from the eye."""
    rng = random.Random(42)
    img = image()
    noise(img, STONE, rng, 2, 4)
    for y in range(16):
        for x in range(16):
            dx, dy = x - 7.5, y - 7.5
            d = math.hypot(dx, dy)
            a = math.degrees(math.atan2(dy, dx)) % 45
            if d < 1.8:
                img.putpixel((x, y), STONE[0] + (255,))
            elif d < 2.6:
                img.putpixel((x, y), STONE[1] + (255,))
            elif d < 7.6 and (a < 10 or a > 41):
                img.putpixel((x, y), STONE[0] + (255,))
            elif 7.0 <= d < 8.0:
                img.putpixel((x, y), STONE[1] + (255,))
    return img


def flour():
    """A sack of flour, open at the top."""
    img = image()
    rows = [
        "................",
        "................",
        "................",
        "......aaaa......",
        ".....awwwwa.....",
        "....awwvwwwa....",
        ".....arrrra.....",
        "....abbbbbba....",
        "...abbcbbbbba...",
        "...abccbbbbda...",
        "..abbcbbbbbdda..",
        "..abbbbbbbbdda..",
        "..abbbbbbbddda..",
        "...addddddddda..",
        "....aaaaaaaaa...",
        "................",
    ]
    colors = {"a": WOOD[0], "w": (244, 240, 228), "v": (210, 202, 184), "r": WOOD[1],
              "b": (196, 170, 120), "c": (224, 204, 160), "d": (158, 132, 90)}
    for y, row in enumerate(rows):
        assert len(row) == 16, row
        for x, ch in enumerate(row):
            if ch in colors:
                img.putpixel((x, y), colors[ch] + (255,))
    return img


def outline(img, color):
    """A dark edge round what is drawn: the clear pixels beside it."""
    edge = []
    for y in range(16):
        for x in range(16):
            if img.getpixel((x, y))[3]:
                continue
            near = [(x + dx, y + dy) for dx, dy in [(-1, 0), (1, 0), (0, -1), (0, 1)]]
            if any(0 <= a < 16 and 0 <= b < 16 and img.getpixel((a, b))[3] for a, b in near):
                edge.append((x, y))
    for xy in edge:
        img.putpixel(xy, color + (255,))


def sheet(palette):
    """A metal sheet seen at a slant: its top, a sheen along the far edge, its
    thin near edge."""
    def draw():
        img = image()
        rows = [
            "................",
            "................",
            "................",
            "................",
            "......aaaaaaaaa.",
            ".....abbbbbbbca.",
            "....abbcccccbda.",
            "...abccccccbdda.",
            "..abcccccccddea.",
            ".abcccccccddeea.",
            ".abbbbbbbbbbea..",
            ".aeeeeeeeeeea...",
            "..aaaaaaaaaa....",
            "................",
            "................",
            "................",
        ]
        colors = {"a": palette[0], "b": palette[-1], "c": palette[3], "d": palette[2], "e": palette[1]}
        for y, row in enumerate(rows):
            assert len(row) == 16, row
            for x, ch in enumerate(row):
                if ch in colors:
                    img.putpixel((x, y), colors[ch] + (255,))
        return img
    return draw


def crushed(metal, seed):
    """Crushed ore: a heap of grit, rock grey and bits of its metal."""
    def draw():
        rng = random.Random(seed)
        img = image()
        for y in range(5, 14):
            half = 2 + (y - 5) * 0.75
            for x in range(16):
                if abs(x - 7.5) <= half and rng.random() < 0.93:
                    palette = metal if rng.random() < 0.45 else ROCK
                    shade = rng.randint(1, len(palette) - 2)
                    if y == 5 or rng.random() < 0.08:
                        shade = len(palette) - 1
                    img.putpixel((x, y), palette[shade] + (255,))
        outline(img, ROCK[0] if metal is not IRON else IRON[0])
        return img
    return draw


def depot_top():
    """A depot's top: a dark iron tray in a frame."""
    rng = random.Random(51)
    img = image()
    noise(img, IRON, rng, 0, 1)
    frame(img, IRON)
    for x in range(3, 13):
        for y in (3, 12):
            img.putpixel((x, y), IRON[2] + (255,))
    return img


def press_side():
    """A press's sides: a casing with a slot down the middle for the head."""
    img = casing()
    for y in range(2, 14):
        for x in (6, 9):
            img.putpixel((x, y), IRON[1] + (255,))
        for x in (7, 8):
            img.putpixel((x, y), IRON[0] + (255,))
    return img


def press_head():
    """The press head's iron: plates, a light rim."""
    rng = random.Random(52)
    img = image()
    noise(img, IRON, rng, 2, 4)
    frame(img, IRON)
    return img


def basin_inside():
    """A basin's inside: worn dark iron."""
    rng = random.Random(53)
    img = image()
    noise(img, IRON, rng, 0, 2)
    return img


def mixer_side():
    """A mixer's sides: a casing with a bronze band round it."""
    img = casing()
    for x in range(1, 15):
        img.putpixel((x, 7), BRONZE[3] + (255,))
        img.putpixel((x, 8), BRONZE[1] + (255,))
    return img


def fan_back():
    """A fan's grille, deep in its end: dark iron bars."""
    rng = random.Random(54)
    img = image()
    noise(img, IRON, rng, 0, 1)
    for i in range(1, 16, 3):
        for j in range(16):
            img.putpixel((i, j), IRON[2] + (255,))
            img.putpixel((j, i), IRON[2] + (255,))
    return img


def propeller():
    """A propeller: four iron blades round a hub."""
    img = image()
    blade = [(7, 1), (8, 1), (7, 2), (8, 2), (6, 3), (7, 3), (8, 3), (6, 4), (7, 4), (8, 4), (7, 5), (8, 5), (7, 6)]
    for turn in range(4):
        for x, y in blade:
            for _ in range(turn):
                x, y = 15 - y, x
            img.putpixel((x, y), IRON[4 if (x + y) % 3 else 3] + (255,))
    for x in range(6, 10):
        for y in range(6, 10):
            img.putpixel((x, y), BRONZE[2 if x in (6, 9) or y in (6, 9) else 3] + (255,))
    outline(img, IRON[0])
    return img


TEXTURES = {
    "block/ferrostone_casing": casing,
    "block/gearbox_side": casing_hole,
    "block/shaft": shaft_side,
    "block/shaft_end": shaft_end,
    "block/cogwheel": planks,
    "block/cogwheel_teeth": teeth,
    "block/creative_motor": motor_side,
    "block/creative_motor_back": motor_back,
    "item/ferrostone_alloy": alloy,
    "block/clutch": banded(REDSTONE),
    "block/gearshift": banded(BRONZE),
    "block/hand_crank": crank_wood,
    "block/sail": canvas,
    "item/engineers_goggles": goggles,
    "block/tin_ore": ore(ROCK, 31, TIN),
    "block/deepslate_tin_ore": ore(DEEP, 32, TIN, streaks=True),
    "block/tin_block": metal_block(TIN, 33),
    "block/raw_tin_block": raw(TIN, 34, block=True),
    "item/raw_tin": raw(TIN, 35),
    "item/tin_ingot": lambda: ingot({"a": TIN[0], "b": TIN[5], "c": TIN[3], "d": TIN[2], "e": TIN[1]}),
    "item/tin_nugget": nugget(TIN),
    "item/bronze_ingot": lambda: ingot({"a": BRONZE[0], "b": BRONZE[4], "c": BRONZE[3], "d": BRONZE[2],
                                        "e": BRONZE[1]}),
    "block/millstone_stone": millstone_stone,
    "block/depot_top": depot_top,
    "block/mechanical_press_side": press_side,
    "block/mechanical_press_head": press_head,
    "block/basin_inside": basin_inside,
    "block/mechanical_mixer_side": mixer_side,
    "block/encased_fan_back": fan_back,
    "item/propeller": propeller,
    **{f"item/{m}_sheet": sheet(pal) for m, pal in
       [("iron", IRON), ("copper", COPPER), ("gold", GOLD), ("bronze", BRONZE), ("tin", TIN)]},
    **{f"item/crushed_{m}": crushed(pal, 60 + i) for i, (m, pal) in
       enumerate([("iron", IRON), ("copper", COPPER), ("gold", GOLD), ("tin", TIN)])},
    "block/millstone_top": millstone_top,
    "item/wheat_flour": flour,
}


def t(name):
    return f"{NS}:{name}"


def face(texture, uv, cull=None):
    f = {"texture": texture, "uv": uv}
    if cull:
        f["cullface"] = cull
    return f


def box(frm, to, textures, rotation=None):
    """An element; `textures` maps each face to (texture, uv)."""
    e = {"from": frm, "to": to, "faces": {k: face(*v) for k, v in textures.items()}}
    if rotation:
        e["rotation"] = rotation
    return e


def shaft_part(lo=0, hi=16):
    side = ("#shaft", [6, lo, 10, hi])
    end = ("#shaft_end", [6, 6, 10, 10])
    return box([6, lo, 6], [10, hi, 10], {"north": side, "south": side, "east": side, "west": side,
                                          "up": end, "down": end})


def disc(r, rotation=None):
    lo, hi = 8 - r, 8 + r
    top = ("#wood", [max(lo, 0), max(lo, 0), min(hi, 16), min(hi, 16)])
    side = ("#wood", [0, 6, 16, 10])
    return box([lo, 6, lo], [hi, 10, hi], {"north": side, "south": side, "east": side, "west": side,
                                          "up": top, "down": top}, rotation)


def tooth(inner, outer, angle):
    """A tooth on the north edge between radii `inner` and `outer`, turned by `angle` about Y."""
    s = ("#teeth", [6, 6, 9, 10])
    e = ("#teeth", [6, 0, 9, 2])
    el = box([6.5, 6, 8 - outer], [9.5, 10, 8 - inner],
             {"north": s, "south": s, "east": s, "west": s, "up": e, "down": e})
    return el, angle


def turned(element_angle):
    el, angle = element_angle
    # Elements turn by at most 45 degrees: a tooth at 90 + a is the north
    # tooth's mirror, written with its own coordinates instead.
    q, a = divmod(angle + 45, 90)
    a -= 45
    frm, to = el["from"], el["to"]
    for _ in range(int(q) % 4):
        # (x, z) -> (16 - z, x): a quarter turn about the block's centre.
        corners = [(frm[0], frm[2]), (to[0], to[2])]
        turned_corners = [(16 - z, x) for x, z in corners]
        xs = sorted(c[0] for c in turned_corners)
        zs = sorted(c[1] for c in turned_corners)
        frm = [xs[0], frm[1], zs[0]]
        to = [xs[1], to[1], zs[1]]
    out = dict(el, **{"from": frm, "to": to})
    if a:
        out["rotation"] = {"origin": [8, 8, 8], "axis": "y", "angle": a}
    return out


def cog(radius, teeth_count):
    elements = [shaft_part(), disc(radius - 2), disc(radius - 2, {"origin": [8, 8, 8], "axis": "y", "angle": 45})]
    step = 360 / teeth_count
    for i in range(teeth_count):
        elements.append(turned(tooth(radius - 2, radius, i * step)))
    return elements


def model(parent="minecraft:block/block", textures=None, elements=None):
    m = {"parent": parent}
    if textures:
        m["textures"] = textures
    if elements is not None:
        m["elements"] = elements
    return m


SHAFT_TEX = {"shaft": t("block/shaft"), "shaft_end": t("block/shaft_end"), "particle": t("block/shaft")}
COG_TEX = dict(SHAFT_TEX, wood=t("block/cogwheel"), teeth=t("block/cogwheel_teeth"), particle=t("block/cogwheel"))

def still(particle):
    """A block's own model when its part turns: nothing but the particle
    (the game draws the turning part, the content engine's `pieces`)."""
    return {"parent": "minecraft:block/block", "textures": {"particle": t(particle)}}


# The plate it stands on covers its whole floor: its template's shape hides
# the face of the block under it.
MOTOR_PLATE = box([0, 0, 0], [16, 2, 16], {
    "north": ("#back", [0, 14, 16, 16]), "south": ("#back", [0, 14, 16, 16]),
    "east": ("#back", [0, 14, 16, 16]), "west": ("#back", [0, 14, 16, 16]),
    "up": ("#back", [0, 0, 16, 16]), "down": ("#back", [0, 0, 16, 16]),
})
MOTOR_BODY = box([2, 2, 4], [14, 14, 16], {
    "north": ("#back", [2, 2, 14, 14]), "south": ("#back", [2, 2, 14, 14], "south"),
    "east": ("#side", [0, 2, 12, 14]), "west": ("#side", [4, 2, 16, 14]),
    "up": ("#side", [2, 4, 14, 16]), "down": ("#side", [2, 0, 14, 12]),
})
MOTOR_TEX = {"side": t("block/creative_motor"), "back": t("block/creative_motor_back"),
             "shaft": t("block/shaft"), "shaft_end": t("block/shaft_end"), "particle": t("block/creative_motor")}

CRANK_TEX = {"wood": t("block/hand_crank"), "metal": t("block/shaft"), "particle": t("block/hand_crank")}


def all_faces(texture, frm, to):
    """Every face of a box, its UV from the box's own extent."""
    x0, y0, z0 = frm
    x1, y1, z1 = to
    uv = {
        "north": [x0, 16 - y1, x1, 16 - y0], "south": [x0, 16 - y1, x1, 16 - y0],
        "east": [z0, 16 - y1, z1, 16 - y0], "west": [z0, 16 - y1, z1, 16 - y0],
        "up": [x0, z0, x1, z1], "down": [x0, z0, x1, z1],
    }
    return box(frm, to, {k: (texture, [max(0, min(16, v)) for v in u]) for k, u in uv.items()})


WHEEL_TEX = dict(SHAFT_TEX, wood=t("block/cogwheel"), particle=t("block/cogwheel"))
SAIL_TEX = dict(SHAFT_TEX, wood=t("block/hand_crank"), cloth=t("block/sail"), particle=t("block/sail"))


def water_wheel():
    """A shaft, a hub, eight paddles out to 20 pixels, and a rim between them."""
    elements = [shaft_part(), all_faces("#wood", [4, 5, 4], [12, 11, 12])]
    for i in range(8):
        elements.append(turned((all_faces("#wood", [7, 3, -12], [9, 13, 4]), i * 45)))
        elements.append(turned((all_faces("#wood", [0.5, 4, -10], [15.5, 12, -8]), i * 45 + 22.5)))
    return elements


def windmill():
    """A shaft, a hub, and four spars out to 24 pixels, a sail on each."""
    elements = [shaft_part(), all_faces("#wood", [5, 6, 5], [11, 10, 11])]
    for i in range(4):
        elements.append(turned((all_faces("#wood", [7, 7, -16], [9, 9, 5]), i * 90)))
        elements.append(turned((all_faces("#cloth", [9, 7.5, -16], [16, 8.5, 1]), i * 90)))
    return elements


MILL_TEX = {"casing": t("block/ferrostone_casing"), "hole": t("block/gearbox_side"),
            "stone": t("block/millstone_stone"), "top": t("block/millstone_top"),
            "particle": t("block/millstone_stone")}
# The millstone: a casing base, still; the stone on it turns (the content
# engine's `pieces`), its shaft below.
MILL_BASE = box([0, 0, 0], [16, 5, 16], {
    "north": ("#casing", [0, 11, 16, 16]), "south": ("#casing", [0, 11, 16, 16]),
    "east": ("#casing", [0, 11, 16, 16]), "west": ("#casing", [0, 11, 16, 16]),
    "up": ("#casing", [0, 0, 16, 16]), "down": ("#hole", [0, 0, 16, 16]),
})
MILL_STONE = box([2, 5, 2], [14, 12, 14], {
    "north": ("#stone", [2, 4, 14, 11]), "south": ("#stone", [2, 4, 14, 11]),
    "east": ("#stone", [2, 4, 14, 11]), "west": ("#stone", [2, 4, 14, 11]),
    "up": ("#top", [2, 2, 14, 14]), "down": ("#stone", [2, 2, 14, 14]),
})


DEPOT_TEX = {"casing": t("block/ferrostone_casing"), "top": t("block/depot_top"),
             "particle": t("block/ferrostone_casing")}
# A depot: a tray as high as its template's shape (a stonecutter's, 9).
DEPOT = box([0, 0, 0], [16, 9, 16], {
    "north": ("#casing", [0, 7, 16, 16]), "south": ("#casing", [0, 7, 16, 16]),
    "east": ("#casing", [0, 7, 16, 16]), "west": ("#casing", [0, 7, 16, 16]),
    "up": ("#top", [0, 0, 16, 16]), "down": ("#casing", [0, 0, 16, 16]),
})
PRESS_TEX = {"head": t("block/mechanical_press_head"), "particle": t("block/mechanical_press_head")}

FAN_TEX = {"side": t("block/ferrostone_casing"), "back": t("block/encased_fan_back"),
           "blade": t("block/mechanical_press_head"), "particle": t("block/ferrostone_casing")}


def fan_ring(y0, y1, face):
    """A fan end's rim, two pixels wide and `y1 - y0` deep, round its grille."""
    v = [0, 0, 16, y1 - y0]
    return [
        box([0, y0, 0], [16, y1, 2], {"north": ("#side", v), "south": ("#side", v),
                                      "east": ("#side", [14, 0, 16, v[3]]), "west": ("#side", [0, 0, 2, v[3]]),
                                      face: ("#side", [0, 0, 16, 2])}),
        box([0, y0, 14], [16, y1, 16], {"north": ("#side", v), "south": ("#side", v),
                                        "east": ("#side", [0, 0, 2, v[3]]), "west": ("#side", [14, 0, 16, v[3]]),
                                        face: ("#side", [0, 14, 16, 16])}),
        box([0, y0, 2], [2, y1, 14], {"west": ("#side", [2, 0, 14, v[3]]), "east": ("#side", [2, 0, 14, v[3]]),
                                      face: ("#side", [0, 2, 2, 14])}),
        box([14, y0, 2], [16, y1, 14], {"east": ("#side", [2, 0, 14, v[3]]), "west": ("#side", [2, 0, 14, v[3]]),
                                        face: ("#side", [14, 2, 16, 14])}),
    ]


# An encased fan along Y: a casing, each end's grille three pixels deep
# behind a rim (its blades turn there: the content engine's `pieces`).
FAN = [
    box([0, 3, 0], [16, 13, 16], {
        "north": ("#side", [0, 3, 16, 13]), "south": ("#side", [0, 3, 16, 13]),
        "east": ("#side", [0, 3, 16, 13]), "west": ("#side", [0, 3, 16, 13]),
        "up": ("#back", [0, 0, 16, 16]), "down": ("#back", [0, 0, 16, 16])}),
    *fan_ring(13, 16, "up"),
    *fan_ring(0, 3, "down"),
]

BASIN_TEX = {"side": t("block/ferrostone_casing"), "inside": t("block/basin_inside"),
             "particle": t("block/ferrostone_casing")}
# A basin: a floor and four walls, two pixels thick (its template's shape, a
# composter's).
BASIN = [
    box([0, 0, 0], [16, 2, 16], {
        "north": ("#side", [0, 14, 16, 16]), "south": ("#side", [0, 14, 16, 16]),
        "east": ("#side", [0, 14, 16, 16]), "west": ("#side", [0, 14, 16, 16]),
        "up": ("#inside", [0, 0, 16, 16]), "down": ("#side", [0, 0, 16, 16])}),
    box([0, 2, 0], [16, 16, 2], {
        "north": ("#side", [0, 0, 16, 14]), "south": ("#inside", [0, 0, 16, 14]),
        "east": ("#side", [14, 0, 16, 14]), "west": ("#side", [0, 0, 2, 14]), "up": ("#side", [0, 0, 16, 2])}),
    box([0, 2, 14], [16, 16, 16], {
        "south": ("#side", [0, 0, 16, 14]), "north": ("#inside", [0, 0, 16, 14]),
        "east": ("#side", [0, 0, 2, 14]), "west": ("#side", [14, 0, 16, 14]), "up": ("#side", [0, 14, 16, 16])}),
    box([0, 2, 2], [2, 16, 14], {
        "west": ("#side", [2, 0, 14, 14]), "east": ("#inside", [2, 0, 14, 14]), "up": ("#side", [0, 2, 2, 14])}),
    box([14, 2, 2], [16, 16, 14], {
        "east": ("#side", [2, 0, 14, 14]), "west": ("#inside", [2, 0, 14, 14]), "up": ("#side", [14, 2, 16, 14])}),
]


def big_item(block):
    """An item for a part wider than its block: the block's model, smaller."""
    return {"parent": t(block), "display": {
        "gui": {"rotation": [30, 225, 0], "translation": [0, 0, 0], "scale": [0.3, 0.3, 0.3]},
        "ground": {"rotation": [0, 0, 0], "translation": [0, 3, 0], "scale": [0.15, 0.15, 0.15]},
        "fixed": {"rotation": [0, 0, 0], "translation": [0, 0, 0], "scale": [0.3, 0.3, 0.3]},
        "thirdperson_righthand": {"rotation": [75, 45, 0], "translation": [0, 2.5, 0], "scale": [0.2, 0.2, 0.2]},
        "thirdperson_lefthand": {"rotation": [75, 45, 0], "translation": [0, 2.5, 0], "scale": [0.2, 0.2, 0.2]},
        "firstperson_righthand": {"rotation": [0, 45, 0], "translation": [0, 0, 0], "scale": [0.25, 0.25, 0.25]},
        "firstperson_lefthand": {"rotation": [0, 225, 0], "translation": [0, 0, 0], "scale": [0.25, 0.25, 0.25]},
    }}


MODELS = {
    "block/ferrostone_casing": model("minecraft:block/cube_all", {"all": t("block/ferrostone_casing")}),
    # The turning parts, about +Y: the items show them whole too.
    "block/shaft": model(textures=SHAFT_TEX, elements=[shaft_part()]),
    "block/shaft_half": model(textures=SHAFT_TEX, elements=[shaft_part(8, 16)]),
    # A casing's turning shaft end: a quarter pixel out of its face, not in
    # it (two faces in one plane flicker).
    "block/shaft_half_cased": model(textures=SHAFT_TEX, elements=[box([6, 8, 6], [10, 16.25, 10], {
        "north": ("#shaft", [6, 8, 10, 16]), "south": ("#shaft", [6, 8, 10, 16]),
        "east": ("#shaft", [6, 8, 10, 16]), "west": ("#shaft", [6, 8, 10, 16]),
        "up": ("#shaft_end", [6, 6, 10, 10]), "down": ("#shaft_end", [6, 6, 10, 10]),
    })]),
    "block/shaft_still": still("block/shaft"),
    "block/cogwheel_still": still("block/cogwheel"),
    "block/large_cogwheel_still": still("block/cogwheel"),
    "block/creative_motor_body": model(textures=MOTOR_TEX, elements=[MOTOR_PLATE, MOTOR_BODY]),
    "block/cogwheel": model(textures=COG_TEX, elements=cog(8, 8)),
    "block/large_cogwheel": model(textures=COG_TEX, elements=cog(14, 16)),
    "block/gearbox": model("minecraft:block/cube_column", {"side": t("block/ferrostone_casing"),
                                                         "end": t("block/gearbox_side")}),
    "block/creative_motor": model(textures=MOTOR_TEX, elements=[
        MOTOR_PLATE,
        MOTOR_BODY,
        box([6, 6, 0], [10, 10, 4], {
            "north": ("#shaft_end", [6, 6, 10, 10]),
            "east": ("#shaft", [6, 0, 10, 4]), "west": ("#shaft", [6, 0, 10, 4]),
            "up": ("#shaft", [6, 0, 10, 4]), "down": ("#shaft", [6, 0, 10, 4]),
        }),
    ]),
    "item/ferrostone_alloy": model("minecraft:item/generated", {"layer0": t("item/ferrostone_alloy")}),
    "item/engineers_goggles": model("minecraft:item/generated", {"layer0": t("item/engineers_goggles")}),
    "block/clutch": model("minecraft:block/cube_column", {"side": t("block/clutch"), "end": t("block/gearbox_side")}),
    "block/gearshift": model("minecraft:block/cube_column",
                             {"side": t("block/gearshift"), "end": t("block/gearbox_side")}),
    # The hand crank, up from the block it is on: a still base, and a
    # handle that turns (the content engine's `pieces`).
    "block/hand_crank_base": model(textures=CRANK_TEX, elements=[
        all_faces("#metal", [4, 0, 4], [12, 2, 12]),
    ]),
    "block/hand_crank_handle": model(textures=CRANK_TEX, elements=[
        all_faces("#metal", [7, 0, 7], [9, 7, 9]),
        all_faces("#wood", [2, 7, 7], [14, 9, 9]),
        all_faces("#wood", [2, 9, 7], [4, 14, 9]),
    ]),
    "block/hand_crank": model(textures=CRANK_TEX, elements=[
        all_faces("#metal", [4, 0, 4], [12, 2, 12]),
        all_faces("#metal", [7, 0, 7], [9, 7, 9]),
        all_faces("#wood", [2, 7, 7], [14, 9, 9]),
        all_faces("#wood", [2, 9, 7], [4, 14, 9]),
    ]),
    **{f"block/{b}": model("minecraft:block/cube_all", {"all": t(f"block/{b}")})
       for b in ["tin_ore", "deepslate_tin_ore", "tin_block", "raw_tin_block"]},
    **{f"item/{i}": model("minecraft:item/generated", {"layer0": t(f"item/{i}")})
       for i in ["raw_tin", "tin_ingot", "tin_nugget", "bronze_ingot"]},
    "block/water_wheel": model(textures=WHEEL_TEX, elements=water_wheel()),
    "block/water_wheel_still": still("block/cogwheel"),
    "item/water_wheel": big_item("block/water_wheel"),
    "block/windmill": model(textures=SAIL_TEX, elements=windmill()),
    "block/windmill_still": still("block/sail"),
    "item/windmill": big_item("block/windmill"),
    "block/millstone": model(textures=MILL_TEX, elements=[MILL_BASE]),
    "block/millstone_top": model(textures=MILL_TEX, elements=[MILL_STONE]),
    "block/millstone_item": model(textures=MILL_TEX, elements=[MILL_BASE, MILL_STONE]),
    "item/wheat_flour": model("minecraft:item/generated", {"layer0": t("item/wheat_flour")}),
    "block/depot": model(textures=DEPOT_TEX, elements=[DEPOT]),
    "block/mechanical_press": model("minecraft:block/cube_column", {
        "side": t("block/mechanical_press_side"), "end": t("block/gearbox_side")}),
    # The head, drawn by the content engine under the press: a plate, inside
    # the press until it presses; its stem, stretched from the plate up
    # into the press.
    "block/mechanical_press_head": model(textures=PRESS_TEX, elements=[
        all_faces("#head", [3, 0, 3], [13, 2, 13]),
    ]),
    "block/mechanical_press_stem": model(textures=PRESS_TEX, elements=[
        all_faces("#head", [6, 0, 6], [10, 16, 10]),
    ]),
    "block/basin": model(textures=BASIN_TEX, elements=BASIN),
    "block/mechanical_mixer": model("minecraft:block/cube_bottom_top", {
        "side": t("block/mechanical_mixer_side"), "top": t("block/gearbox_side"),
        "bottom": t("block/gearbox_side")}),
    # The whisk, drawn by the content engine while the mixer mixes: two
    # crossed blades and a hub; its pole, stretched up into the mixer.
    "block/mechanical_mixer_whisk": model(textures=PRESS_TEX, elements=[
        all_faces("#head", [3, 0, 7], [13, 4, 9]),
        all_faces("#head", [7, 0.5, 3], [9, 3.5, 13]),
        all_faces("#head", [7, 3.5, 7], [9, 8, 9]),
    ]),
    "block/mechanical_mixer_pole": model(textures=PRESS_TEX, elements=[
        all_faces("#head", [7, 0, 7], [9, 16, 9]),
    ]),
    "block/encased_fan": model(textures=FAN_TEX, elements=FAN),
    # Its blades, drawn turning in the +Y end's grille.
    "block/fan_blades": model(textures=FAN_TEX, elements=[
        all_faces("#blade", [7, 13, 7], [9, 15.5, 9]),
        all_faces("#blade", [2, 14, 7], [14, 15, 9]),
        all_faces("#blade", [7, 14.2, 2], [9, 14.8, 14]),
    ]),
    "item/propeller": model("minecraft:item/generated", {"layer0": t("item/propeller")}),
    **{f"item/{m}_sheet": model("minecraft:item/generated", {"layer0": t(f"item/{m}_sheet")})
       for m in ["iron", "copper", "gold", "bronze", "tin"]},
    **{f"item/crushed_{m}": model("minecraft:item/generated", {"layer0": t(f"item/crushed_{m}")})
       for m in ["iron", "copper", "gold", "tin"]},
}

AXIS = {
    "variants": {
        "axis=x": {"x": 90, "y": 90},
        "axis=y": {},
        "axis=z": {"x": 90},
    }
}
# An end portal frame's (and a stonecutter's) facing points at the player
# who placed it; the motor's shaft (the model's north) points the other way,
# where the player looked.
FACING = {"variants": {f"facing={f}": {"y": y} for f, y in
                       [("north", 180), ("east", 270), ("south", 0), ("west", 90)]}}
# The frame's eye: the same.
FACING_EYE = {"variants": {f"eye={e},facing={f}": {"y": y} for f, y in
                           [("north", 180), ("east", 270), ("south", 0), ("west", 90)]
                           for e in ("false", "true")}}


SIX = {"variants": {f"facing={f}": r for f, r in [
    ("up", {}), ("down", {"x": 180}), ("north", {"x": 90}), ("south", {"x": 90, "y": 180}),
    ("east", {"x": 90, "y": 90}), ("west", {"x": 90, "y": 270}),
]}}


def states(base, model_name):
    out = {"variants": {}}
    for key, v in base["variants"].items():
        out["variants"][key] = dict({"model": t(model_name)}, **v)
    return out


def axis_and_water(model_name):
    """Iron chain's states: an axis and waterlogged."""
    out = {"variants": {}}
    for key, v in AXIS["variants"].items():
        for water in ("false", "true"):
            out["variants"][f"{key},waterlogged={water}"] = dict({"model": t(model_name)}, **v)
    return out


BLOCKS = {
    # id: (template, properties, blockstate, item model, names (en, id))
    "ferrostone_casing": ("minecraft:polished_andesite",
                          {"hardness": 2.0, "explosion_resistance": 6.0, "sounds": "minecraft:copper_block"},
                          {"variants": {"": {"model": t("block/ferrostone_casing")}}},
                          "block/ferrostone_casing", ("Ferrostone Casing", "Selubung Ferrostone")),
    "shaft": ("minecraft:iron_chain", {"hardness": 1.5, "sounds": "minecraft:iron_block"},
              axis_and_water("block/shaft_still"), "block/shaft", ("Shaft", "Poros")),
    "cogwheel": ("minecraft:iron_chain", {"hardness": 1.5, "sounds": "minecraft:oak_planks", "map_color": 13},
                 axis_and_water("block/cogwheel_still"), "block/cogwheel", ("Cogwheel", "Roda Gigi")),
    "large_cogwheel": ("minecraft:iron_chain", {"hardness": 1.5, "sounds": "minecraft:oak_planks", "map_color": 13},
                       axis_and_water("block/large_cogwheel_still"), "block/large_cogwheel",
                       ("Large Cogwheel", "Roda Gigi Besar")),
    "gearbox": ("minecraft:quartz_pillar", {"hardness": 2.0, "sounds": "minecraft:copper_block", "map_color": 11},
                states(AXIS, "block/gearbox"), "block/gearbox", ("Gearbox", "Kotak Gigi")),
    "creative_motor": ("minecraft:end_portal_frame",
                       {"hardness": -1.0, "explosion_resistance": 3600000.0, "sounds": "minecraft:copper_block",
                        "light": 6, "map_color": 23},
                       states(FACING_EYE, "block/creative_motor_body"), "block/creative_motor",
                       ("Creative Motor", "Motor Kreatif")),
    # A waxed lightning rod's states: six facings, powered, waterlogged;
    # placed against a block, it faces out of it.
    "hand_crank": ("minecraft:waxed_lightning_rod", {"hardness": 0.5, "sounds": "minecraft:oak_planks",
                                                     "map_color": 13},
                   states(SIX, "block/hand_crank_base"), "block/hand_crank", ("Hand Crank", "Engkol Tangan")),
    "clutch": ("minecraft:quartz_pillar", {"hardness": 2.0, "sounds": "minecraft:copper_block", "map_color": 11},
               states(AXIS, "block/clutch"), "block/clutch", ("Clutch", "Kopling")),
    "gearshift": ("minecraft:quartz_pillar", {"hardness": 2.0, "sounds": "minecraft:copper_block", "map_color": 11},
                  states(AXIS, "block/gearshift"), "block/gearshift", ("Gearshift", "Pemindah Gigi")),
    # Tin: in the ground where copper is (worldgen/placed_feature/ore_tin).
    "tin_ore": ("minecraft:iron_ore", {}, {"variants": {"": {"model": t("block/tin_ore")}}}, "block/tin_ore",
                ("Tin Ore", "Bijih Timah")),
    "deepslate_tin_ore": ("minecraft:deepslate_iron_ore", {},
                          {"variants": {"": {"model": t("block/deepslate_tin_ore")}}},
                          "block/deepslate_tin_ore", ("Deepslate Tin Ore", "Bijih Timah Batu Dalam")),
    "tin_block": ("minecraft:iron_block", {"map_color": 6}, {"variants": {"": {"model": t("block/tin_block")}}},
                  "block/tin_block", ("Block of Tin", "Balok Timah")),
    "raw_tin_block": ("minecraft:raw_iron_block", {"map_color": 6},
                      {"variants": {"": {"model": t("block/raw_tin_block")}}}, "block/raw_tin_block",
                      ("Block of Raw Tin", "Balok Timah Mentah")),
    # A wheel on a shaft: water flowing past its sides turns it.
    "water_wheel": ("minecraft:iron_chain", {"hardness": 1.5, "sounds": "minecraft:oak_planks", "map_color": 13},
                    axis_and_water("block/water_wheel_still"), "item/water_wheel", ("Water Wheel", "Kincir Air")),
    # Sails on a shaft: the wind turns them under open sky, faster higher up.
    "windmill": ("minecraft:iron_chain", {"hardness": 1.5, "sounds": "minecraft:white_wool", "map_color": 3},
                 axis_and_water("block/windmill_still"), "item/windmill", ("Windmill", "Kincir Angin")),
    # A stonecutter's shape (9 high) and states (four facings); a use goes to
    # the content engine first, which puts items in or takes them out.
    "millstone": ("minecraft:stonecutter", {"hardness": 2.0, "sounds": "minecraft:stone", "map_color": 11},
                  states(FACING, "block/millstone"), "block/millstone_item", ("Millstone", "Batu Gilingan")),
    # A tray for one stack: a press over it works what is on it.
    "depot": ("minecraft:stonecutter", {"hardness": 2.0, "sounds": "minecraft:copper_block", "map_color": 11},
              states(FACING, "block/depot"), "block/depot", ("Depot", "Depot")),
    # A quartz pillar's axis: the shaft through it; the head comes down on the
    # depot under it.
    "mechanical_press": ("minecraft:quartz_pillar", {"hardness": 2.5, "sounds": "minecraft:copper_block",
                                                     "map_color": 11},
                         states(AXIS, "block/mechanical_press"), "block/mechanical_press",
                         ("Mechanical Press", "Mesin Press")),
    # A composter's shape (a floor and walls, two pixels thick) and states;
    # a use goes to the content engine first, which puts items in or takes
    # them out.
    "basin": ("minecraft:composter", {"hardness": 2.0, "sounds": "minecraft:copper_block", "map_color": 11},
              {"variants": {f"level={i}": {"model": t("block/basin")} for i in range(9)}}, "block/basin",
              ("Basin", "Baskom")),
    # A shaft into its top turns it; its whisk mixes in the basin under it,
    # or one block lower.
    "mechanical_mixer": ("minecraft:polished_andesite", {"hardness": 2.5, "sounds": "minecraft:copper_block",
                                                         "map_color": 11},
                         {"variants": {"": {"model": t("block/mechanical_mixer")}}}, "block/mechanical_mixer",
                         ("Mechanical Mixer", "Mesin Pengaduk")),
    # A quartz pillar's axis: its shaft; it blows out of the end no shaft
    # drives it from.
    "encased_fan": ("minecraft:quartz_pillar", {"hardness": 2.0, "sounds": "minecraft:copper_block",
                                                "map_color": 11},
                    states(AXIS, "block/encased_fan"), "block/encased_fan", ("Encased Fan", "Kipas Terselubung")),
}

ITEMS = {
    "ferrostone_alloy": ("minecraft:brick", "item/ferrostone_alloy", ("Ferrostone Alloy", "Paduan Ferrostone")),
    "raw_tin": ("minecraft:raw_iron", "item/raw_tin", ("Raw Tin", "Timah Mentah")),
    "tin_ingot": ("minecraft:iron_ingot", "item/tin_ingot", ("Tin Ingot", "Batangan Timah")),
    "tin_nugget": ("minecraft:iron_nugget", "item/tin_nugget", ("Tin Nugget", "Butiran Timah")),
    "bronze_ingot": ("minecraft:copper_ingot", "item/bronze_ingot", ("Bronze Ingot", "Batangan Perunggu")),
    "wheat_flour": ("minecraft:brick", "item/wheat_flour", ("Wheat Flour", "Tepung Terigu")),
    **{f"{m}_sheet": ("minecraft:brick", f"item/{m}_sheet", (f"{en} Sheet", f"Lempeng {idn}"))
       for m, en, idn in [("iron", "Iron", "Besi"), ("copper", "Copper", "Tembaga"), ("gold", "Golden", "Emas"),
                          ("bronze", "Bronze", "Perunggu"), ("tin", "Tin", "Timah")]},
    **{f"crushed_{m}": ("minecraft:brick", f"item/crushed_{m}", (f"Crushed {en}", f"{idn} Hancur"))
       for m, en, idn in [("iron", "Iron", "Besi"), ("copper", "Copper", "Tembaga"), ("gold", "Gold", "Emas"),
                          ("tin", "Tin", "Timah")]},
    "propeller": ("minecraft:brick", "item/propeller", ("Propeller", "Baling-Baling")),
    # Worn on the head (a leather cap's slot), or held: a part's speed shows
    # when looked at.
    "engineers_goggles": ("minecraft:leather_helmet", "item/engineers_goggles",
                          ("Engineer's Goggles", "Kacamata Insinyur")),
}

# The goggles' corner line: "Cogwheel: 32 RPM".
HUD = {
    "kinetica.hud.still": ("still", "diam"),
    "kinetica.hud.jammed": ("jammed", "macet"),
    "kinetica.hud.rpm": ("%s RPM", "%s RPM"),
    "kinetica.hud.overstressed": ("overstressed", "kelebihan beban"),
    "kinetica.hud.stress": ("%s / %s SU", "%s / %s SU"),
    "kinetica.hud.pressing": ("Pressing", "Penekanan"),
    # What a fan's air does.
    "kinetica.hud.air": ("air", "udara"),
    "kinetica.hud.pull": ("pulling", "menyedot"),
    "kinetica.hud.washing": ("washing", "mencuci"),
    "kinetica.hud.smoking": ("smoking", "mengasapi"),
    "kinetica.hud.blasting": ("blasting", "melebur"),
}


def recipe_shaped(pattern, key, result, count=1):
    return {"type": "minecraft:crafting_shaped", "category": "misc", "pattern": pattern, "key": key,
            "result": {"id": t(result), "count": count}}


def recipe_shapeless(ingredients, result, count=1):
    return {"type": "minecraft:crafting_shapeless", "category": "misc", "ingredients": ingredients,
            "result": {"id": t(result), "count": count}}


def cooking(kind, ingredient, result, xp, ticks):
    return {"type": f"minecraft:{kind}", "category": "misc", "ingredient": ingredient,
            "result": {"id": t(result)}, "experience": xp, "cookingtime": ticks}


def metal_recipes(metal):
    """A metal's ingots, nuggets, blocks and raw blocks into each other, and smelted."""
    raw_item, ingot_item, nugget_item = f"raw_{metal}", f"{metal}_ingot", f"{metal}_nugget"
    out = {
        f"{metal}_block": recipe_shaped(["III", "III", "III"], {"I": t(ingot_item)}, f"{metal}_block"),
        f"{ingot_item}_from_{metal}_block": recipe_shapeless([t(f"{metal}_block")], ingot_item, 9),
        f"{ingot_item}_from_nuggets": recipe_shaped(["NNN", "NNN", "NNN"], {"N": t(nugget_item)}, ingot_item),
        nugget_item: recipe_shapeless([t(ingot_item)], nugget_item, 9),
        f"raw_{metal}_block": recipe_shaped(["RRR", "RRR", "RRR"], {"R": t(raw_item)}, f"raw_{metal}_block"),
        f"{raw_item}_from_raw_{metal}_block": recipe_shapeless([t(f"raw_{metal}_block")], raw_item, 9),
    }
    for kind, ticks in [("smelting", 200), ("blasting", 100)]:
        out[f"{ingot_item}_from_{kind}_raw"] = cooking(kind, t(raw_item), ingot_item, 0.7, ticks)
        out[f"{ingot_item}_from_{kind}_ore"] = cooking(
            kind, [t(f"{metal}_ore"), t(f"deepslate_{metal}_ore")], ingot_item, 0.7, ticks)
    return out


RECIPES = {
    "ferrostone_alloy": recipe_shaped(["SN", "NS"], {"S": "minecraft:andesite", "N": "minecraft:iron_nugget"},
                                      "ferrostone_alloy"),
    "ferrostone_alloy_from_tuff": recipe_shaped(["SN", "NS"], {"S": "minecraft:tuff", "N": "minecraft:iron_nugget"},
                                                "ferrostone_alloy"),
    "shaft": recipe_shaped(["A", "A"], {"A": t("ferrostone_alloy")}, "shaft", 6),
    "cogwheel": recipe_shapeless([t("shaft"), "#minecraft:planks"], "cogwheel"),
    "large_cogwheel": recipe_shapeless([t("shaft"), "#minecraft:planks", "#minecraft:planks"], "large_cogwheel"),
    "large_cogwheel_from_cogwheel": recipe_shapeless([t("cogwheel"), "#minecraft:planks"], "large_cogwheel"),
    "ferrostone_casing": recipe_shaped([" P ", "PAP", " P "], {"P": "#minecraft:planks", "A": t("ferrostone_alloy")},
                                       "ferrostone_casing", 2),
    "gearbox": recipe_shaped([" C ", "CXC", " C "], {"C": t("cogwheel"), "X": t("ferrostone_casing")}, "gearbox"),
    "hand_crank": recipe_shaped(["PPP", "  A"], {"P": "#minecraft:planks", "A": t("ferrostone_alloy")}, "hand_crank"),
    "clutch": recipe_shapeless([t("ferrostone_casing"), t("shaft"), "minecraft:redstone"], "clutch"),
    "gearshift": recipe_shapeless([t("ferrostone_casing"), t("cogwheel"), "minecraft:redstone"], "gearshift"),
    "water_wheel": recipe_shaped(["PPP", "PSP", "PPP"], {"P": "#minecraft:planks", "S": t("shaft")}, "water_wheel"),
    "windmill": recipe_shaped(["W W", " S ", "W W"], {"W": "#minecraft:wool", "S": t("shaft")}, "windmill"),
    **metal_recipes("tin"),
    "bronze_ingot": recipe_shapeless(["minecraft:copper_ingot"] * 3 + [t("tin_ingot")], "bronze_ingot", 4),
    "engineers_goggles": recipe_shaped([" L ", "GAG"], {"L": "minecraft:leather", "G": "minecraft:glass",
                                                         "A": t("ferrostone_alloy")}, "engineers_goggles"),
    "millstone": recipe_shaped([" P ", "ACA", " S "], {"P": "#minecraft:planks", "A": t("ferrostone_alloy"),
                                                       "C": t("cogwheel"), "S": "minecraft:stone"}, "millstone"),
    "depot": recipe_shapeless([t("ferrostone_casing"), t("ferrostone_alloy")], "depot"),
    "mechanical_press": recipe_shaped(["S", "X", "I"], {"S": t("shaft"), "X": t("ferrostone_casing"),
                                                       "I": "minecraft:iron_block"}, "mechanical_press"),
    "basin": recipe_shaped(["A A", "AAA"], {"A": t("ferrostone_alloy")}, "basin"),
    "propeller": recipe_shaped([" S ", "SAS", " S "], {"S": t("iron_sheet"), "A": t("ferrostone_alloy")},
                               "propeller"),
    "encased_fan": recipe_shaped(["S", "C", "P"], {"S": t("shaft"), "C": t("ferrostone_casing"),
                                                  "P": t("propeller")}, "encased_fan"),
    "mechanical_mixer": recipe_shaped(["C", "X", "W"], {"C": t("cogwheel"), "X": t("ferrostone_casing"),
                                                       "W": t("iron_sheet")}, "mechanical_mixer"),
    # Crushed ore smelts into an ingot.
    **{f"{ingot.split(':')[1]}_from_{kind}_crushed_{m}": {
        "type": f"minecraft:{kind}", "category": "misc", "ingredient": t(f"crushed_{m}"),
        "result": {"id": ingot}, "experience": 0.1, "cookingtime": ticks}
       for m, ingot in [("iron", "minecraft:iron_ingot"), ("copper", "minecraft:copper_ingot"),
                        ("gold", "minecraft:gold_ingot"), ("tin", t("tin_ingot"))]
       for kind, ticks in [("smelting", 200), ("blasting", 100)]},
    # Flour bakes into bread.
    **{f"bread_from_{kind}_flour": {"type": f"minecraft:{kind}", "category": "food",
                                    "ingredient": t("wheat_flour"), "result": {"id": "minecraft:bread"},
                                    "experience": 0.35, "cookingtime": ticks}
       for kind, ticks in [("smelting", 200), ("smoking", 100)]},
}


def mill(item, outputs, time=100):
    """A milling recipe: `outputs` (item, count, chance) for one `item`, in
    `time` work (1/16 for each RPM a tick)."""
    return {"input": item, "time": time,
            "outputs": [{"item": i, "count": c, "chance": p} for i, c, p in outputs]}


# The millstone's recipes (read by the content engine's millstone.rs).
DYES = [("dandelion", "yellow", 2), ("poppy", "red", 2), ("blue_orchid", "light_blue", 2),
        ("allium", "magenta", 2), ("azure_bluet", "light_gray", 2), ("red_tulip", "red", 2),
        ("orange_tulip", "orange", 2), ("white_tulip", "light_gray", 2), ("pink_tulip", "pink", 2),
        ("oxeye_daisy", "light_gray", 2), ("cornflower", "blue", 2), ("lily_of_the_valley", "white", 2),
        ("torchflower", "orange", 2), ("sunflower", "yellow", 4), ("lilac", "magenta", 4),
        ("rose_bush", "red", 4), ("peony", "pink", 4), ("pitcher_plant", "cyan", 4), ("cactus", "green", 2),
        ("cocoa_beans", "brown", 2), ("ink_sac", "black", 2), ("wither_rose", "black", 3)]
MILLING = [
    mill("minecraft:wheat", [(t("wheat_flour"), 1, 1.0), (t("wheat_flour"), 1, 0.25),
                             ("minecraft:wheat_seeds", 1, 0.25)], 150),
    mill("minecraft:cobblestone", [("minecraft:gravel", 1, 1.0)], 250),
    mill("minecraft:gravel", [("minecraft:sand", 1, 1.0), ("minecraft:flint", 1, 0.1)], 250),
    mill("minecraft:terracotta", [("minecraft:red_sand", 1, 1.0)], 250),
    mill("minecraft:bone", [("minecraft:bone_meal", 3, 1.0), ("minecraft:bone_meal", 1, 0.25)]),
    mill("minecraft:sugar_cane", [("minecraft:sugar", 2, 1.0)], 50),
    mill("minecraft:clay", [("minecraft:clay_ball", 3, 1.0), ("minecraft:clay_ball", 1, 0.5)]),
    mill("minecraft:white_wool", [("minecraft:string", 3, 1.0)], 150),
    *[mill(f"minecraft:{flower}", [(f"minecraft:{dye}_dye", n, 1.0)], 50) for flower, dye, n in DYES],
    # Raw metal crushed: one, and a quarter of the time a second.
    *[mill(raw, [(t(f"crushed_{m}"), 1, 1.0), (t(f"crushed_{m}"), 1, 0.25)], 250)
      for m, raw in [("iron", "minecraft:raw_iron"), ("copper", "minecraft:raw_copper"),
                     ("gold", "minecraft:raw_gold"), ("tin", t("raw_tin"))]],
]

# The press's recipes, on a depot under it (read by millstone.rs, as the
# millstone's are).
PRESSING = [
    *[mill(ingot, [(t(f"{m}_sheet"), 1, 1.0)])
      for m, ingot in [("iron", "minecraft:iron_ingot"), ("copper", "minecraft:copper_ingot"),
                       ("gold", "minecraft:gold_ingot"), ("bronze", t("bronze_ingot")), ("tin", t("tin_ingot"))]],
    mill("minecraft:sugar_cane", [("minecraft:paper", 1, 1.0)]),
]


def mix(inputs, outputs, time=100):
    """A mixing recipe: all `inputs` (item, count) in a basin mixed in `time`
    work into `outputs` (item, count, chance)."""
    return {"inputs": [{"item": i, "count": c} for i, c in inputs], "time": time,
            "outputs": [{"item": i, "count": c, "chance": p} for i, c, p in outputs]}


COLORS = ["white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray", "cyan",
          "purple", "blue", "brown", "green", "red", "black"]
# The mixer's recipes, in a basin under it (read by the content engine's
# basin.rs): the first one a basin holds everything for.
MIXING = [
    # Ferrostone alloy: one of each, not two (as crafting asks).
    mix([("minecraft:andesite", 1), ("minecraft:iron_nugget", 1)], [(t("ferrostone_alloy"), 1, 1.0)]),
    mix([("minecraft:tuff", 1), ("minecraft:iron_nugget", 1)], [(t("ferrostone_alloy"), 1, 1.0)]),
    mix([("minecraft:copper_ingot", 3), (t("tin_ingot"), 1)], [(t("bronze_ingot"), 4, 1.0)], 150),
    mix([(t("crushed_copper"), 3), (t("crushed_tin"), 1)], [(t("bronze_ingot"), 4, 1.0)], 200),
    mix([("minecraft:dirt", 2), ("minecraft:gravel", 2)], [("minecraft:coarse_dirt", 4, 1.0)]),
    mix([("minecraft:mud", 1), ("minecraft:wheat", 1)], [("minecraft:packed_mud", 1, 1.0)]),
    mix([("minecraft:brown_mushroom", 1), ("minecraft:red_mushroom", 1), ("minecraft:bowl", 1)],
        [("minecraft:mushroom_stew", 1, 1.0)], 50),
    mix([("minecraft:spider_eye", 1), ("minecraft:brown_mushroom", 1), ("minecraft:sugar", 1)],
        [("minecraft:fermented_spider_eye", 1, 1.0)]),
    mix([("minecraft:slime_ball", 1), ("minecraft:blaze_powder", 1)], [("minecraft:magma_cream", 1, 1.0)]),
    *[mix([("minecraft:sand", 4), ("minecraft:gravel", 4), (f"minecraft:{c}_dye", 1)],
          [(f"minecraft:{c}_concrete_powder", 8, 1.0)]) for c in COLORS],
]


# A fan's washing, its air through water (read by the content engine's
# fan.rs, in millstone.rs's form): crushed ore into ten nuggets and now
# and then something more, dye washed out, concrete powder set.
WASHING = [
    *[mill(t(f"crushed_{m}"), [(nugget, 10, 1.0), *extra])
      for m, nugget, extra in [("iron", "minecraft:iron_nugget", [("minecraft:redstone", 1, 0.25)]),
                               ("copper", "minecraft:copper_nugget", [("minecraft:clay_ball", 1, 0.25)]),
                               ("gold", "minecraft:gold_nugget", [("minecraft:quartz", 1, 0.25)]),
                               ("tin", t("tin_nugget"), [])]],
    mill("minecraft:gravel", [("minecraft:flint", 1, 0.25), ("minecraft:iron_nugget", 1, 0.125)]),
    mill("minecraft:sand", [("minecraft:clay_ball", 1, 0.25)]),
    mill("minecraft:red_sand", [("minecraft:gold_nugget", 3, 0.125), ("minecraft:dead_bush", 1, 0.05)]),
    mill("minecraft:soul_sand", [("minecraft:quartz", 1, 0.125), ("minecraft:gold_nugget", 1, 0.02)]),
    mill("minecraft:magma_block", [("minecraft:obsidian", 1, 1.0)]),
    *[mill(f"minecraft:{c}_concrete_powder", [(f"minecraft:{c}_concrete", 1, 1.0)]) for c in COLORS],
    *[mill(f"minecraft:{c}_{kind}", [(f"minecraft:white_{kind}", 1, 1.0)])
      for c in COLORS if c != "white" for kind in ("wool", "carpet")],
]


def ore_loot(block, drop):
    """An ore's drops: itself with silk touch, else its raw metal (more with fortune)."""
    return {"type": "minecraft:block", "pools": [{
        "rolls": 1, "bonus_rolls": 0,
        "entries": [{"type": "minecraft:alternatives", "children": [
            {"type": "minecraft:item", "name": t(block), "conditions": [{
                "condition": "minecraft:match_tool",
                "predicate": {"predicates": {"minecraft:enchantments": [{
                    "enchantments": "minecraft:silk_touch", "levels": {"min": 1}}]}}}]},
            {"type": "minecraft:item", "name": t(drop), "functions": [
                {"function": "minecraft:apply_bonus", "enchantment": "minecraft:fortune",
                 "formula": "minecraft:ore_drops"},
                {"function": "minecraft:explosion_decay"}]},
        ]}],
    }], "random_sequence": f"{NS}:blocks/{block}"}


# Tin in the ground: a vein of up to 9 in stone or deepslate, 10 tries a
# chunk between y -32 and 80 (most near 24), wherever vanilla puts copper.
# 26.3's formats: a configured feature's settings beside its type, a block
# state as {"id": ...}.
WORLDGEN = {
    "configured_feature/ore_tin": {
        "type": "minecraft:ore", "size": 9, "discard_chance_on_air_exposure": 0.0, "targets": [
            {"target": {"predicate_type": "minecraft:tag_match", "tag": "minecraft:stone_ore_replaceables"},
             "state": {"id": t("tin_ore")}},
            {"target": {"predicate_type": "minecraft:tag_match", "tag": "minecraft:deepslate_ore_replaceables"},
             "state": {"id": t("deepslate_tin_ore")}},
        ]},
    "placed_feature/ore_tin": {"feature": t("ore_tin"), "placement": [
        {"type": "minecraft:count", "count": 10},
        {"type": "minecraft:in_square"},
        {"type": "minecraft:height_range", "height": {
            "type": "minecraft:trapezoid", "min_inclusive": {"absolute": -32}, "max_inclusive": {"absolute": 80}}},
        {"type": "minecraft:biome"},
    ], "horizon:beside": "minecraft:ore_copper"},
}

# Convention tags (c:) other mods' recipes use.
C_TAGS = {
    "block/ores/tin": [t("tin_ore"), t("deepslate_tin_ore")],
    "item/ores/tin": [t("tin_ore"), t("deepslate_tin_ore")],
    "block/storage_blocks/tin": [t("tin_block")],
    "item/ingots/tin": [t("tin_ingot")],
    "item/ingots/bronze": [t("bronze_ingot")],
    "item/nuggets/tin": [t("tin_nugget")],
    "item/raw_materials/tin": [t("raw_tin")],
    **{f"item/plates/{m}": [t(f"{m}_sheet")] for m in ["iron", "copper", "gold", "bronze", "tin"]},
}


def loot(block):
    return {"type": "minecraft:block", "pools": [{
        "rolls": 1, "bonus_rolls": 0,
        "entries": [{"type": "minecraft:item", "name": t(block)}],
        "conditions": [{"condition": "minecraft:survives_explosion"}],
    }], "random_sequence": f"{NS}:blocks/{block}"}


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    if isinstance(value, Image.Image):
        value.save(path)
    else:
        path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def main():
    if ROOT.exists():
        shutil.rmtree(ROOT)
    assets, data = ROOT / "assets" / NS, ROOT / "data"
    (ROOT).mkdir(parents=True)
    (ROOT / "horizon-mod.toml").write_text(
        "# Tachyon: Aether & Gears. Kinetica, the machines: written by tachyon/art/kinetica.py.\n"
        'id = "kinetica"\nname = "Kinetica"\nversion = "0.6.0"\napi = "0.2"\nside = "both"\n'
        'capabilities = ["registries"]\n', encoding="utf-8")
    for name, draw in TEXTURES.items():
        write(assets / "textures" / f"{name}.png", draw())
    for name, m in MODELS.items():
        write(assets / "models" / f"{name}.json", m)
    en, idn = {}, {}
    for block, (template, props, blockstate, item_model, (name_en, name_id)) in BLOCKS.items():
        write(data / NS / "horizon" / "block" / f"{block}.json",
              {"template": template, "properties": props, "item": True})
        write(assets / "blockstates" / f"{block}.json", blockstate)
        write(assets / "items" / f"{block}.json", {"model": {"type": "minecraft:model", "model": t(item_model)}})
        drops = {"tin_ore": "raw_tin", "deepslate_tin_ore": "raw_tin"}
        write(data / NS / "loot_table" / "blocks" / f"{block}.json",
              ore_loot(block, drops[block]) if block in drops else loot(block))
        en[f"block.{NS}.{block}"], idn[f"block.{NS}.{block}"] = name_en, name_id
    for item, (template, item_model, (name_en, name_id)) in ITEMS.items():
        write(data / NS / "horizon" / "item" / f"{item}.json", {"template": template})
        write(assets / "items" / f"{item}.json", {"model": {"type": "minecraft:model", "model": t(item_model)}})
        en[f"item.{NS}.{item}"], idn[f"item.{NS}.{item}"] = name_en, name_id
    for key, (name_en, name_id) in HUD.items():
        en[key], idn[key] = name_en, name_id
    for name, r in RECIPES.items():
        write(data / NS / "recipe" / f"{name}.json", r)
    inputs = [m["input"] for m in MILLING]
    assert len(inputs) == len(set(inputs)), "one milling recipe an item"
    write(data / NS / "tachyon" / "milling.json", {"recipes": MILLING})
    inputs = [m["input"] for m in PRESSING]
    assert len(inputs) == len(set(inputs)), "one pressing recipe an item"
    write(data / NS / "tachyon" / "pressing.json", {"recipes": PRESSING})
    write(data / NS / "tachyon" / "mixing.json", {"recipes": MIXING})
    inputs = [m["input"] for m in WASHING]
    assert len(inputs) == len(set(inputs)), "one washing recipe an item"
    write(data / NS / "tachyon" / "washing.json", {"recipes": WASHING})
    wooden = ("cogwheel", "large_cogwheel", "hand_crank", "water_wheel", "windmill")
    pickaxe = [t(b) for b in BLOCKS if b not in wooden]
    axe = [t(b) for b in wooden]
    write(data / "minecraft" / "tags" / "block" / "mineable" / "pickaxe.json", {"values": pickaxe})
    write(data / "minecraft" / "tags" / "block" / "mineable" / "axe.json", {"values": axe})
    stone_tool = [t(b) for b in ["tin_ore", "deepslate_tin_ore", "tin_block", "raw_tin_block"]]
    write(data / "minecraft" / "tags" / "block" / "needs_stone_tool.json", {"values": stone_tool})
    for name, values in C_TAGS.items():
        write(data / "c" / "tags" / f"{name}.json", {"values": values})
    for name, value in WORLDGEN.items():
        write(data / NS / "worldgen" / f"{name}.json", value)
    write(assets / "lang" / "en_us.json", en)
    write(assets / "lang" / "id_id.json", idn)
    files = sum(1 for p in ROOT.rglob("*") if p.is_file())
    print(f"kinetica: {files} files in {ROOT}")


if __name__ == "__main__":
    main()
