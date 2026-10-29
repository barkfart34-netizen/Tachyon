"""Lost Realms' content: the structures found around the world (their
templates written as NBT here, block by block), how they are spread, and
their chests' loot, written into tachyon/content/lost_realms (a Horizon
content mod, API 0.2).

    python tachyon/art/lost_realms.py

Everything here is Tachyon's own. Run it again after a change; it rewrites
the whole mod.
"""

import gzip
import json
import random
import shutil
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "content" / "lost_realms"
NS = "lost_realms"
# The world data version the templates are written for (26.3).
DATA_VERSION = 5023


def t(name):
    return f"{NS}:{name}"


# NBT, big-endian, as a structure template is stored.
class Int(int):
    pass


class Byte(int):
    pass


def nbt_payload(value):
    """(tag id, payload bytes) of a value: dict is a compound, list a list,
    str a string, Int and Byte their numbers."""
    if isinstance(value, dict):
        out = b"".join(named(k, v) for k, v in value.items()) + b"\x00"
        return 10, out
    if isinstance(value, list):
        items = [nbt_payload(v) for v in value]
        kind = items[0][0] if items else 0
        assert all(k == kind for k, _ in items), "a list holds one kind of tag"
        return 9, struct.pack(">bi", kind, len(items)) + b"".join(p for _, p in items)
    if isinstance(value, str):
        data = value.encode("utf-8")
        return 8, struct.pack(">H", len(data)) + data
    if isinstance(value, Byte):
        return 1, struct.pack(">b", value)
    if isinstance(value, int):
        return 3, struct.pack(">i", value)
    raise TypeError(value)


def named(name, value):
    kind, payload = nbt_payload(value)
    key = name.encode("utf-8")
    return struct.pack(">bH", kind, len(key)) + key + payload


def template(size, blocks):
    """A structure template: `blocks` maps (x, y, z) to (name, properties,
    block entity or None); a place left out keeps what the world has there."""
    palette, index, entries = [], {}, []
    for pos in sorted(blocks, key=lambda p: (p[1], p[2], p[0])):
        name, props, entity = blocks[pos]
        key = (name, tuple(sorted(props.items())))
        if key not in index:
            index[key] = len(palette)
            state = {"Name": name}
            if props:
                state["Properties"] = dict(sorted(props.items()))
            palette.append(state)
        entry = {"pos": [Int(c) for c in pos], "state": Int(index[key])}
        if entity:
            entry["nbt"] = entity
        entries.append(entry)
    root = {"DataVersion": Int(DATA_VERSION), "size": [Int(c) for c in size], "palette": palette,
            "blocks": entries, "entities": []}
    return gzip.compress(named("", root), mtime=0)


def workshop_ruin():
    """A tinkerer's workshop gone to ruin: a stone brick floor, broken walls
    on spruce corner posts, half a roof, a machine left on its casing, and a
    chest. 7 wide, 5 high, 7 deep; the door on the south side."""
    rng = random.Random(7)
    b = {}

    def put(p, name, props=None, entity=None):
        b[p] = (name, props or {}, entity)

    w, d = 7, 7
    for x in range(w):
        for z in range(d):
            put((x, 0, z), rng.choices(
                ["minecraft:stone_bricks", "minecraft:cracked_stone_bricks", "minecraft:mossy_stone_bricks",
                 "minecraft:cobblestone"], [60, 20, 15, 5])[0])
            for y in range(1, 5):
                put((x, y, z), "minecraft:air")
    for x in range(w):
        for z in range(d):
            edge = x in (0, w - 1) or z in (0, d - 1)
            corner = x in (0, w - 1) and z in (0, d - 1)
            if corner:
                for y in range(1, 5):
                    put((x, y, z), "minecraft:spruce_log", {"axis": "y"})
            elif edge:
                # Broken walls: one to three courses high, lower toward the
                # collapsed (south) side.
                top = rng.choice([1, 2, 3, 3]) if z < 4 else rng.choice([1, 1, 2])
                for y in range(1, top + 1):
                    put((x, y, z), rng.choices(["minecraft:stone_bricks", "minecraft:cracked_stone_bricks",
                                                "minecraft:mossy_stone_bricks"], [60, 25, 15])[0])
    # The door and two windows.
    for y in (1, 2, 3):
        put((3, y, d - 1), "minecraft:air")
    put((0, 2, 3), "minecraft:air")
    put((w - 1, 2, 3), "minecraft:air")
    # Half a roof: a beam, slabs over the north half.
    for x in range(w):
        put((x, 4, 3), "minecraft:spruce_planks")
        for z in range(0, 3):
            if (x, z) not in [(0, 0), (w - 1, 0)] and rng.random() < 0.85:
                put((x, 4, z), "minecraft:spruce_slab", {"type": "bottom", "waterlogged": "false"})
    # The machine: a cogwheel on a shaft standing on a casing, a gearbox
    # with a shaft beside it.
    chain = {"waterlogged": "false"}
    put((1, 1, 1), "kinetica:ferrostone_casing")
    put((1, 2, 1), "kinetica:shaft", dict(chain, axis="y"))
    put((1, 3, 1), "kinetica:cogwheel", dict(chain, axis="y"))
    put((5, 1, 1), "kinetica:gearbox", {"axis": "y"})
    put((4, 1, 1), "kinetica:shaft", dict(chain, axis="x"))
    put((1, 1, 5), "minecraft:crafting_table")
    put((5, 1, 5), "minecraft:chest", {"facing": "west", "type": "single", "waterlogged": "false"},
        {"id": "minecraft:chest", "LootTable": t("chests/workshop_ruin")})
    # A lantern hanging from the roof (a slab over it, whatever fell).
    put((3, 4, 2), "minecraft:spruce_slab", {"type": "bottom", "waterlogged": "false"})
    put((3, 3, 2), "minecraft:lantern", {"hanging": "true", "waterlogged": "false"})
    for p in [(5, 3, 1), (1, 3, 4)]:
        put(p, "minecraft:cobweb")
    for p in [(2, 1, 3), (4, 1, 4), (3, 1, 4)]:
        put(p, "minecraft:moss_carpet")
    return (w, 5, d), b


def watchtower():
    """A watchtower of the old realm, its east side fallen: a 5×5 stone
    tower, 12 high, a ladder up its north wall through two floors to a
    walled top with a chest. The door on the south side."""
    rng = random.Random(11)
    b = {}

    def put(p, name, props=None, entity=None):
        b[p] = (name, props or {}, entity)

    def stone():
        return rng.choices(["minecraft:stone_bricks", "minecraft:cracked_stone_bricks",
                            "minecraft:mossy_stone_bricks", "minecraft:cobblestone"], [55, 20, 20, 5])[0]

    w, h = 5, 12
    for x in range(w):
        for z in range(w):
            put((x, 0, z), stone())
            edge = x in (0, w - 1) or z in (0, w - 1)
            for y in range(1, h):
                if edge and y <= 10:
                    # The east side broke away high up.
                    fallen = x == w - 1 and y >= 7 + rng.randint(0, 2)
                    put((x, y, z), "minecraft:air" if fallen else stone())
                else:
                    put((x, y, z), "minecraft:air")
    # Floors at 5 and 10 (the top), with a hole for the ladder.
    for x in range(1, w - 1):
        for z in range(1, w - 1):
            for y in (5, 10):
                put((x, y, z), "minecraft:spruce_planks")
    # Battlements round the top, every other block.
    for x in range(w):
        for z in range(w):
            if (x in (0, w - 1) or z in (0, w - 1)) and (x + z) % 2 == 0 and x != w - 1:
                put((x, 11, z), stone())
    for y in range(1, 11):
        put((2, y, 1), "minecraft:ladder", {"facing": "south", "waterlogged": "false"})
    # The door, arrow slits on each floor.
    for y in (1, 2):
        put((2, y, w - 1), "minecraft:air")
    for y in (3, 7):
        put((0, y, 2), "minecraft:air")
        put((2, y, w - 1), "minecraft:air")
    put((1, 6, 3), "minecraft:barrel", {"facing": "up", "open": "false"})
    put((3, 11, 3), "minecraft:chest", {"facing": "west", "type": "single", "waterlogged": "false"},
        {"id": "minecraft:chest", "LootTable": t("chests/watchtower")})
    put((1, 11, 3), "minecraft:lantern", {"hanging": "false", "waterlogged": "false"})
    put((3, 6, 1), "minecraft:cobweb")
    return (w, h, w), b


# The structures: their templates, the biomes they are in, how far apart
# (in chunks), and their names.
STRUCTURES = {
    "workshop_ruin": {
        "template": workshop_ruin,
        "biomes": ["#minecraft:is_forest", "#minecraft:is_taiga", "#minecraft:is_jungle", "#minecraft:is_hill",
                   "#minecraft:is_savanna", "minecraft:plains", "minecraft:sunflower_plains", "minecraft:meadow"],
        "spacing": 28, "separation": 10, "salt": 20481029,
        "names": ("Workshop Ruin", "Bengkel Terbengkalai"),
    },
    "watchtower": {
        "template": watchtower,
        "biomes": ["minecraft:plains", "minecraft:meadow", "minecraft:savanna", "#minecraft:is_forest",
                   "#minecraft:is_taiga", "minecraft:windswept_hills", "minecraft:windswept_forest"],
        "spacing": 34, "separation": 12, "salt": 20481030,
        "names": ("Ruined Watchtower", "Menara Pengawas Runtuh"),
    },
}


def count(lo, hi):
    return {"function": "minecraft:set_count", "count": {"type": "minecraft:uniform", "min": lo, "max": hi}}


def entry(name, weight, lo=1, hi=1):
    e = {"type": "minecraft:item", "name": name, "weight": weight}
    if (lo, hi) != (1, 1):
        e["functions"] = [count(lo, hi)]
    return e


LOOT = {
    "chests/workshop_ruin": {"type": "minecraft:chest", "pools": [
        {"rolls": {"type": "minecraft:uniform", "min": 3, "max": 6}, "bonus_rolls": 0, "entries": [
            entry("kinetica:ferrostone_alloy", 10, 2, 5),
            entry("kinetica:shaft", 10, 2, 6),
            entry("kinetica:cogwheel", 8, 1, 3),
            entry("kinetica:tin_ingot", 8, 1, 4),
            entry("kinetica:raw_tin", 6, 2, 5),
            entry("kinetica:hand_crank", 4),
            entry("kinetica:large_cogwheel", 3),
            entry("minecraft:iron_nugget", 6, 3, 9),
            entry("minecraft:copper_ingot", 5, 1, 4),
            entry("minecraft:bread", 5, 1, 3),
        ]},
        {"rolls": 1, "bonus_rolls": 0, "entries": [
            {"type": "minecraft:item", "name": "kinetica:engineers_goggles", "weight": 1},
            {"type": "minecraft:empty", "weight": 5},
        ]},
    ], "random_sequence": t("chests/workshop_ruin")},
    "chests/watchtower": {"type": "minecraft:chest", "pools": [
        {"rolls": {"type": "minecraft:uniform", "min": 3, "max": 6}, "bonus_rolls": 0, "entries": [
            entry("minecraft:arrow", 10, 4, 16),
            entry("minecraft:iron_ingot", 8, 1, 4),
            entry("kinetica:tin_ingot", 8, 1, 4),
            entry("minecraft:bread", 6, 1, 4),
            entry("minecraft:gold_nugget", 6, 3, 9),
            entry("minecraft:map", 4),
            entry("minecraft:spyglass", 2),
            entry("minecraft:iron_sword", 3),
            entry("kinetica:ferrostone_alloy", 6, 2, 5),
        ]},
    ], "random_sequence": t("chests/watchtower")},
}


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    if isinstance(value, bytes):
        path.write_bytes(value)
    else:
        path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def main():
    if ROOT.exists():
        shutil.rmtree(ROOT)
    data = ROOT / "data" / NS
    ROOT.mkdir(parents=True)
    (ROOT / "horizon-mod.toml").write_text(
        "# Tachyon: Aether & Gears. Lost Realms, the structures: written by tachyon/art/lost_realms.py.\n"
        'id = "lost_realms"\nname = "Lost Realms"\nversion = "0.2.0"\napi = "0.2"\nside = "both"\n'
        'capabilities = ["registries"]\n', encoding="utf-8")
    for name, s in STRUCTURES.items():
        size, blocks = s["template"]()
        write(data / "structure" / f"{name}.nbt", template(size, blocks))
        write(data / "worldgen" / "template_pool" / name / "start.json", {
            "fallback": "minecraft:empty", "elements": [{"weight": 1, "element": {
                "element_type": "minecraft:single_pool_element", "location": t(name),
                "projection": "rigid", "processors": "minecraft:empty"}}]})
        write(data / "worldgen" / "structure" / f"{name}.json", {
            "type": "minecraft:jigsaw", "biomes": f"#{t('has_structure/' + name)}",
            "step": "surface_structures", "spawn_overrides": {}, "terrain_adaptation": "beard_thin",
            "start_pool": t(f"{name}/start"), "size": 1, "start_height": {"absolute": 0},
            "project_start_to_heightmap": "WORLD_SURFACE_WG", "max_distance_from_center": 80})
        write(data / "worldgen" / "structure_set" / f"{name}.json", {
            "structures": [{"structure": t(name), "weight": 1}],
            "placement": {"type": "minecraft:random_spread", "salt": s["salt"], "spacing": s["spacing"],
                          "separation": s["separation"]}})
        write(data / "tags" / "worldgen" / "biome" / "has_structure" / f"{name}.json", {"values": s["biomes"]})
    for name, table in LOOT.items():
        write(data / "loot_table" / f"{name}.json", table)
    # The structures' names (the quest book's "Find: ...").
    lang = ROOT / "assets" / NS / "lang"
    write(lang / "en_us.json", {f"structure.{NS}.{n}": s["names"][0] for n, s in STRUCTURES.items()})
    write(lang / "id_id.json", {f"structure.{NS}.{n}": s["names"][1] for n, s in STRUCTURES.items()})
    files = sum(1 for p in ROOT.rglob("*") if p.is_file())
    print(f"lost_realms: {files} files in {ROOT}")


if __name__ == "__main__":
    main()
