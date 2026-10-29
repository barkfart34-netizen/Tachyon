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


def alloy():
    """An ingot: ferrostone grey, an iron sheen."""
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
    colors = {"a": STONE[0], "b": IRON[5], "c": IRON[3], "d": STONE[3], "e": STONE[1]}
    for y, row in enumerate(rows):
        for x, ch in enumerate(row):
            if ch in colors:
                img.putpixel((x, y), colors[ch] + (255,))
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

MODELS = {
    "block/ferrostone_casing": model("minecraft:block/cube_all", {"all": t("block/ferrostone_casing")}),
    "block/shaft": model(textures=SHAFT_TEX, elements=[shaft_part()]),
    "block/cogwheel": model(textures=COG_TEX, elements=cog(8, 8)),
    "block/large_cogwheel": model(textures=COG_TEX, elements=cog(14, 16)),
    "block/gearbox": model("minecraft:block/cube_column", {"side": t("block/ferrostone_casing"),
                                                         "end": t("block/gearbox_side")}),
    "block/creative_motor": model(textures={
        "side": t("block/creative_motor"), "back": t("block/creative_motor_back"),
        "shaft": t("block/shaft"), "shaft_end": t("block/shaft_end"), "particle": t("block/creative_motor"),
    }, elements=[
        box([2, 2, 4], [14, 14, 16], {
            "north": ("#back", [2, 2, 14, 14]), "south": ("#back", [2, 2, 14, 14], "south"),
            "east": ("#side", [0, 2, 12, 14]), "west": ("#side", [4, 2, 16, 14]),
            "up": ("#side", [2, 4, 14, 16]), "down": ("#side", [2, 0, 14, 12]),
        }),
        box([6, 6, 0], [10, 10, 4], {
            "north": ("#shaft_end", [6, 6, 10, 10]),
            "east": ("#shaft", [6, 0, 10, 4]), "west": ("#shaft", [6, 0, 10, 4]),
            "up": ("#shaft", [6, 0, 10, 4]), "down": ("#shaft", [6, 0, 10, 4]),
        }),
    ]),
    "item/ferrostone_alloy": model("minecraft:item/generated", {"layer0": t("item/ferrostone_alloy")}),
}

AXIS = {
    "variants": {
        "axis=x": {"x": 90, "y": 90},
        "axis=y": {},
        "axis=z": {"x": 90},
    }
}
# Glazed terracotta's facing points at the player who placed it; the motor's
# shaft (the model's north) points the other way, where the player looked.
FACING = {"variants": {f"facing={f}": {"y": y} for f, y in
                       [("north", 180), ("east", 270), ("south", 0), ("west", 90)]}}


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
              axis_and_water("block/shaft"), "block/shaft", ("Shaft", "Poros")),
    "cogwheel": ("minecraft:iron_chain", {"hardness": 1.5, "sounds": "minecraft:oak_planks", "map_color": 13},
                 axis_and_water("block/cogwheel"), "block/cogwheel", ("Cogwheel", "Roda Gigi")),
    "large_cogwheel": ("minecraft:iron_chain", {"hardness": 1.5, "sounds": "minecraft:oak_planks", "map_color": 13},
                       axis_and_water("block/large_cogwheel"), "block/large_cogwheel",
                       ("Large Cogwheel", "Roda Gigi Besar")),
    "gearbox": ("minecraft:quartz_pillar", {"hardness": 2.0, "sounds": "minecraft:copper_block", "map_color": 11},
                states(AXIS, "block/gearbox"), "block/gearbox", ("Gearbox", "Kotak Gigi")),
    "creative_motor": ("minecraft:white_glazed_terracotta",
                       {"hardness": -1.0, "explosion_resistance": 3600000.0, "sounds": "minecraft:copper_block",
                        "light": 6, "map_color": 23},
                       states(FACING, "block/creative_motor"), "block/creative_motor",
                       ("Creative Motor", "Motor Kreatif")),
}

ITEMS = {
    "ferrostone_alloy": ("minecraft:brick", "item/ferrostone_alloy", ("Ferrostone Alloy", "Paduan Ferrostone")),
}


def recipe_shaped(pattern, key, result, count=1):
    return {"type": "minecraft:crafting_shaped", "category": "misc", "pattern": pattern, "key": key,
            "result": {"id": t(result), "count": count}}


def recipe_shapeless(ingredients, result, count=1):
    return {"type": "minecraft:crafting_shapeless", "category": "misc", "ingredients": ingredients,
            "result": {"id": t(result), "count": count}}


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
        'id = "kinetica"\nname = "Kinetica"\nversion = "0.1.0"\napi = "0.2"\nside = "both"\n'
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
        write(data / NS / "loot_table" / "blocks" / f"{block}.json", loot(block))
        en[f"block.{NS}.{block}"], idn[f"block.{NS}.{block}"] = name_en, name_id
    for item, (template, item_model, (name_en, name_id)) in ITEMS.items():
        write(data / NS / "horizon" / "item" / f"{item}.json", {"template": template})
        write(assets / "items" / f"{item}.json", {"model": {"type": "minecraft:model", "model": t(item_model)}})
        en[f"item.{NS}.{item}"], idn[f"item.{NS}.{item}"] = name_en, name_id
    for name, r in RECIPES.items():
        write(data / NS / "recipe" / f"{name}.json", r)
    pickaxe = [t(b) for b in BLOCKS if b not in ("cogwheel", "large_cogwheel")]
    axe = [t("cogwheel"), t("large_cogwheel")]
    write(data / "minecraft" / "tags" / "block" / "mineable" / "pickaxe.json", {"values": pickaxe})
    write(data / "minecraft" / "tags" / "block" / "mineable" / "axe.json", {"values": axe})
    write(assets / "lang" / "en_us.json", en)
    write(assets / "lang" / "id_id.json", idn)
    files = sum(1 for p in ROOT.rglob("*") if p.is_file())
    print(f"kinetica: {files} files in {ROOT}")


if __name__ == "__main__":
    main()
