"""Tachyon in a world, driven through its control socket, with pictures.

    python scene.py GAME HOME OUT

Starts GAME (Tachyon.exe, which runs the game with TACHYON_GAME set) in a
new creative world in HOME, waits for the world, then takes pictures into
OUT: the world with the HUD (the built-in Minimap and Coordinates HUD),
Options > Mods (the built-in mods' rows), and writes every reply to
OUT/scene.json. Exits nonzero when a step fails, so the job shows it.
"""

import json
import math
import os
import re
import socket
import subprocess
import sys
import time


class Control:
    def __init__(self, endpoint, deadline):
        while not os.path.exists(endpoint):
            if time.time() > deadline:
                raise SystemExit("no control endpoint: the game never listened")
            time.sleep(1)
        time.sleep(0.5)
        with open(endpoint, encoding="utf-8") as f:
            info = json.load(f)
        self.sock = socket.create_connection(("127.0.0.1", info["port"]), timeout=180)
        self.file = self.sock.makefile("r", encoding="utf-8")
        self.log = []
        self.send(f"auth {info['token']}")

    def send(self, line):
        self.sock.sendall((line + "\n").encode())
        reply = json.loads(self.file.readline() or "{}")
        self.log.append({"command": line if not line.startswith("auth") else "auth", "reply": reply})
        print(line.split()[0], json.dumps(reply)[:400], flush=True)
        return reply


# Kinetica on a stone floor south of the player (yaw 0 looks south), by
# offsets from the block the player stands in: a motor driving a shaft and
# cogwheels (small ones reverse, the large one turns at half speed), another
# driving a gearbox that sends the drive three ways, the other blocks, and
# its items in the hotbar.
SHOWCASE = [
    ("fill", (-8, -1, -2), (8, -1, 20), "minecraft:smooth_stone"),
    ("fill", (-8, 0, -2), (8, 30, 20), "minecraft:air"),
    ("setblock", (0, 0, 7), "kinetica:creative_motor[facing=south]"),
    ("setblock", (0, 0, 6), "kinetica:shaft[axis=z]"),
    ("setblock", (0, 0, 5), "kinetica:cogwheel[axis=z]"),
    ("setblock", (1, 0, 5), "kinetica:cogwheel[axis=z]"),
    ("setblock", (-1, 1, 5), "kinetica:large_cogwheel[axis=z]"),
    ("setblock", (4, 0, 7), "kinetica:creative_motor[facing=south]"),
    ("setblock", (4, 0, 6), "kinetica:gearbox[axis=y]"),
    ("setblock", (4, 0, 5), "kinetica:shaft[axis=z]"),
    ("fill", (5, 0, 6), (6, 0, 6), "kinetica:shaft[axis=x]"),
    ("setblock", (3, 0, 6), "kinetica:shaft[axis=x]"),
    ("setblock", (-4, 0, 6), "kinetica:ferrostone_casing"),
    ("fill", (-4, 1, 6), (-4, 3, 6), "kinetica:shaft[axis=y]"),
    # A line along x: a motor, a gearshift over a redstone block (the shaft
    # after it turns the other way), an unpowered clutch (passing it on).
    ("setblock", (-6, 0, 9), "kinetica:creative_motor[facing=west]"),
    ("setblock", (-5, 0, 9), "kinetica:shaft[axis=x]"),
    ("setblock", (-4, -1, 9), "minecraft:redstone_block"),
    ("setblock", (-4, 0, 9), "kinetica:gearshift[axis=x]"),
    ("setblock", (-3, 0, 9), "kinetica:shaft[axis=x]"),
    ("setblock", (-2, 0, 9), "kinetica:clutch[axis=x]"),
    ("setblock", (-1, 0, 9), "kinetica:shaft[axis=x]"),
    # A hand crank on a shaft.
    ("setblock", (2, 0, 9), "kinetica:shaft[axis=y]"),
    ("setblock", (2, 1, 9), "kinetica:hand_crank[facing=up]"),
    # A water wheel along x: water from a source runs over its top in a
    # glass channel, then falls past its south side into a hole.
    ("setblock", (6, 1, 10), "kinetica:water_wheel[axis=x]"),
    ("setblock", (6, 1, 9), "minecraft:stone"),
    ("fill", (5, 2, 8), (7, 2, 11), "minecraft:glass"),
    ("fill", (6, 2, 9), (6, 2, 11), "minecraft:air"),
    ("setblock", (6, -2, 11), "minecraft:glass"),
    ("setblock", (6, -1, 11), "minecraft:air"),
    ("setblock", (6, 2, 9), "minecraft:water"),
    # Tin: its ores and blocks in a row.
    ("setblock", (-8, 0, 12), "kinetica:tin_ore"),
    ("setblock", (-7, 0, 12), "kinetica:deepslate_tin_ore"),
    ("setblock", (-6, 0, 12), "kinetica:tin_block"),
    ("setblock", (-5, 0, 12), "kinetica:raw_tin_block"),
    # A windmill in the open, along z, on a shaft behind it.
    ("setblock", (-6, 2, 2), "kinetica:windmill[axis=z]"),
    ("setblock", (-6, 2, 1), "kinetica:shaft[axis=z]"),
    # A millstone on a gearbox a motor drives.
    ("setblock", (2, 0, 12), "kinetica:creative_motor[facing=west]"),
    ("setblock", (3, 0, 12), "kinetica:gearbox[axis=z]"),
    ("setblock", (3, 1, 12), "kinetica:millstone[facing=north]"),
    # A press over a depot, a motor beside it; a hopper beside the depot
    # fills it, one under it empties it.
    ("setblock", (-2, 0, 13), "kinetica:depot[facing=north]"),
    ("setblock", (-2, 1, 13), "kinetica:mechanical_press[axis=x]"),
    ("setblock", (-3, 1, 13), "kinetica:creative_motor[facing=west]"),
    ("setblock", (-1, 0, 13), "minecraft:hopper[facing=west]"),
    ("setblock", (-2, -1, 13), "minecraft:hopper[facing=down]"),
    # A depot alone.
    ("setblock", (-4, 0, 11), "kinetica:depot[facing=north]"),
    # A mixer two blocks over a basin (Create's way), turned from above
    # through a gearbox a motor drives.
    ("setblock", (5, 0, 14), "kinetica:basin"),
    ("setblock", (5, 2, 14), "kinetica:mechanical_mixer"),
    ("setblock", (5, 3, 14), "kinetica:gearbox[axis=x]"),
    ("setblock", (5, 3, 13), "kinetica:creative_motor[facing=north]"),
    # A fan blowing east (a motor drives it from the west) through water,
    # walled in glass, to a stone wall: it washes what it blows there.
    ("setblock", (-6, 0, -1), "kinetica:creative_motor[facing=west]"),
    ("setblock", (-5, 0, -1), "kinetica:encased_fan[axis=x]"),
    ("fill", (-4, 0, -2), (-2, 0, -2), "minecraft:glass"),
    ("fill", (-4, 0, 0), (-2, 0, 0), "minecraft:glass"),
    ("setblock", (-1, 0, -1), "minecraft:stone"),
    ("setblock", (-4, 0, -1), "minecraft:water"),
    # A belt along x by the player (the ruins take the far end), a motor
    # turning it from the north, carrying west: a funnel on a chest at its
    # east end takes the chest's iron out onto it (once used, below), one on
    # a chest at its west end puts it in.
    ("setblock", (8, 0, 2), "minecraft:chest[facing=west]"),
    ("setblock", (7, 0, 2), "kinetica:funnel[facing=west]"),
    ("fill", (3, 0, 2), (6, 0, 2), "kinetica:belt[facing=east]"),
    ("setblock", (4, 0, 1), "kinetica:creative_motor[facing=north]"),
    ("setblock", (2, 0, 2), "kinetica:funnel[facing=east]"),
    ("setblock", (1, 0, 2), "minecraft:chest[facing=east]"),
]
# Given to the player: with the goggles on and the motor at 128 RPM, enough
# for Chronicles' machine quests (the counts some ask to hold).
ITEMS = ["shaft 8", "cogwheel 2", "large_cogwheel", "gearbox", "hand_crank", "clutch", "gearshift",
         "water_wheel", "windmill", "creative_motor", "ferrostone_alloy 4", "raw_tin", "tin_ingot",
         "bronze_ingot", "millstone", "wheat_flour", "depot", "mechanical_press", "crushed_iron 4",
         "basin", "mechanical_mixer", "encased_fan", "propeller", "belt 8", "funnel 2"]


def look_at(control, eye, target):
    """Turns the player to look at a block's centre."""
    v = [t + 0.5 - e for t, e in zip(target, eye)]
    yaw = math.degrees(math.atan2(-v[0], v[2]))
    pitch = -math.degrees(math.atan2(v[1], math.hypot(v[0], v[2])))
    control.send(f"look {yaw:.1f} {pitch:.1f}")


def kinetica(control, out):
    """Kinetica's blocks placed by commands, in pictures: two a second apart
    (the parts turn between them), and one looking at a cogwheel (its speed
    in the corner). Whether every command ran (an unknown block is a
    command error in chat)."""
    player = control.send("status").get("player") or {}
    px, py, pz = player.get("position", [0.5, 64.0, 0.5])
    base = [math.floor(px), math.floor(py), math.floor(pz)]
    at = lambda o: " ".join(str(b + d) for b, d in zip(base, o))
    good = True
    commands = [f"{c[0]} {at(c[1])} {at(c[2])} {c[3]}" if c[0] == "fill" else f"{c[0]} {at(c[1])} {c[2]}"
                for c in SHOWCASE]
    gives = [f"give @s kinetica:{item}" for item in ITEMS]
    # Goggles on: a part's speed shows when looked at.
    wear = ["item replace entity @s armor.head with kinetica:engineers_goggles",
            # What the clearing dropped (cocoa, saplings) out of the pictures.
            "kill @e[type=item]"]
    for command in commands + gives + wear:
        if "error" in control.send("chat /" + command):
            good = False
    time.sleep(2)
    chat = " ".join(str(line) for line in control.send("chat-log 30").get("chat", []))
    for word in ["Unknown", "Incorrect", "Invalid", "Could not", "Failed"]:
        if word in chat:
            print("chat says", word, ":", chat)
            good = False
    control.send("look 0 25")
    time.sleep(6)
    for name in ["kinetica", "kinetica-later"]:
        if not control.send("screenshot " + os.path.join(out, f"{name}.png")).get("ok"):
            good = False
        time.sleep(1)
    eye = [px, py + 1.62, pz]
    look_at(control, eye, [base[0], base[1], base[2] + 5])
    time.sleep(2)
    control.send("screenshot " + os.path.join(out, "kinetica-goggles.png"))
    control.send("look 35 30")
    time.sleep(2)
    control.send("screenshot " + os.path.join(out, "kinetica-side.png"))

    def visit(stand, target):
        """The player stood on the block at `stand`, looking at `target`'s
        middle (both offsets): within reach, to use it or read its speed."""
        x, y, z = [b + d for b, d in zip(base, stand)]
        control.send(f"chat /tp @s {x + 0.5} {y} {z + 0.5}")
        time.sleep(1.5)
        look_at(control, [x + 0.5, y + 1.62, z + 0.5], [b + d for b, d in zip(base, target)])
        time.sleep(1)

    # Two right clicks on the first motor, from behind it: 32 RPM, then 64,
    # then 128.
    visit((0, 0, 9), (0, 0, 7))
    for _ in range(2):
        control.send("mouse right click")
        time.sleep(0.5)
    time.sleep(1)
    control.send("screenshot " + os.path.join(out, "kinetica-faster.png"))
    # The water wheel and the windmill, each with its speed in the corner.
    for name, stand, target in [("water", (4, 0, 12), (6, 1, 10)), ("wind", (-5, 0, 4), (-6, 2, 2))]:
        visit(stand, target)
        time.sleep(1)
        control.send("screenshot " + os.path.join(out, f"kinetica-{name}.png"))
    # Tin in the ground under the showcase, counted by turning its ores to
    # stone: two 32-block layers, about y 15 to 46 and -17 to 14.
    found = 0
    for top in (-17, -49):
        corners = f"{base[0] - 16} {base[1] + top - 31} {base[2] - 16} {base[0] + 15} {base[1] + top} {base[2] + 15}"
        control.send(f"chat /fill {corners} minecraft:stone replace kinetica:tin_ore")
        control.send(f"chat /fill {corners} minecraft:deepslate replace kinetica:deepslate_tin_ore")
        time.sleep(2)
        for line in control.send("chat-log 2").get("chat", []):
            filled = re.search(r"filled (\d+)", str(line))
            found += int(filled.group(1)) if filled else 0
    print("tin ores under the showcase:", found)
    if not found:
        good = False
    # Lost Realms' workshop ruin and watchtower: their templates set down at
    # the far end, turned so their doors face the player.
    for name, corner in [("workshop_ruin", (3, -1, 20)), ("watchtower", (-5, -1, 20))]:
        corner = " ".join(str(b + d) for b, d in zip(base, corner))
        control.send(f"chat /place template lost_realms:{name} {corner} 180")
    visit((0, 0, 11), (0, 1, 17))
    time.sleep(1)
    control.send("screenshot " + os.path.join(out, "ruin.png"))
    visit((-3, 0, 10), (-7, 7, 18))
    time.sleep(1)
    control.send("screenshot " + os.path.join(out, "watchtower.png"))
    # The millstone: its motor up to 128 RPM, wheat put in with a use, the
    # flour taken out with an empty hand and counted.
    visit((1, 0, 13), (2, 0, 12))
    for _ in range(2):
        control.send("mouse right click")
        time.sleep(0.5)
    control.send("chat /item replace entity @s weapon.mainhand with minecraft:wheat 16")
    visit((3, 0, 10), (3, 0.7, 12))
    control.send("mouse right click")
    time.sleep(8)
    control.send("screenshot " + os.path.join(out, "kinetica-millstone.png"))
    control.send("chat /item replace entity @s weapon.mainhand with minecraft:air")
    time.sleep(0.5)
    control.send("mouse right click")
    time.sleep(1)
    control.send("chat /clear @s kinetica:wheat_flour 0")
    time.sleep(1)
    lines = " ".join(str(line) for line in control.send("chat-log 2").get("chat", []))
    print("flour:", lines)
    flour = re.search(r"Found (\d+)", lines)
    # The one given with the items, and what was ground.
    if not flour or int(flour.group(1)) < 5:
        good = False
    # The press: its motor up to 128 RPM (from its west side: the ruin's
    # walls stand south of it), iron put in the hopper beside the depot, the
    # sheets the hopper under it took counted (out of it into the player's
    # hand).
    visit((-4, 0, 13), (-3, 1, 13))
    for _ in range(2):
        control.send("mouse right click")
        time.sleep(0.5)
    # The press's head coming down on the iron, from two blocks north (the
    # whole tray shows under the press; the goggles tell what it holds):
    # 8 ingots take it about 5 seconds at 128 RPM.
    visit((-2, 0, 11), (-2, 0, 13))
    control.send(f"chat /item replace block {at((-1, 0, 13))} container.0 with minecraft:iron_ingot 8")
    time.sleep(1.2)
    for name in ["kinetica-press", "kinetica-press-2"]:
        control.send("screenshot " + os.path.join(out, f"{name}.png"))
        time.sleep(0.3)
    # The depot alone: five ingots put on it with a use, lying on its tray.
    control.send("chat /item replace entity @s weapon.mainhand with minecraft:iron_ingot 5")
    visit((-4, 0, 13), (-4, 0, 11))
    control.send("mouse right click")
    time.sleep(1.5)
    control.send("screenshot " + os.path.join(out, "kinetica-depot.png"))
    time.sleep(6)
    control.send(f"chat /item replace entity @s weapon.mainhand from block {at((-2, -1, 13))} container.0")
    time.sleep(1)
    control.send("chat /clear @s kinetica:iron_sheet 0")
    time.sleep(1)
    lines = " ".join(str(line) for line in control.send("chat-log 2").get("chat", []))
    print("sheets:", lines)
    sheets = re.search(r"Found (\d+)", lines)
    if not sheets or int(sheets.group(1)) < 6:
        good = False
    # The mixer: copper and tin put in its basin with uses, the bronze it
    # mixed taken out with an empty hand and counted (with the one given).
    visit((5, 0, 16), (5, 0, 14))
    for item in ["minecraft:copper_ingot 3", "kinetica:tin_ingot 1"]:
        control.send(f"chat /item replace entity @s weapon.mainhand with {item}")
        time.sleep(0.5)
        control.send("mouse right click")
        time.sleep(0.5)
    time.sleep(1.5)
    control.send("screenshot " + os.path.join(out, "kinetica-mixer.png"))
    time.sleep(4)
    control.send("chat /item replace entity @s weapon.mainhand with minecraft:air")
    time.sleep(0.5)
    control.send("mouse right click")
    time.sleep(1)
    control.send("chat /clear @s kinetica:bronze_ingot 0")
    time.sleep(1)
    lines = " ".join(str(line) for line in control.send("chat-log 2").get("chat", []))
    print("bronze:", lines)
    bronze = re.search(r"Found (\d+)", lines)
    if not bronze or int(bronze.group(1)) < 5:
        good = False
    # The fan: four crushed iron dropped in its air, washed into forty
    # nuggets at the wall, picked up and counted.
    x, y, z = base[0] - 4 + 0.5, base[1] + 0.5, base[2] - 1 + 0.5
    item = '{Item:{id:"kinetica:crushed_iron",count:4},PickupDelay:0s}'
    control.send(f"chat /summon minecraft:item {x} {y} {z} {item}")
    visit((-5, 0, 2), (-5, 0, -1))
    time.sleep(3)
    # A screenshot takes the runner half a minute: the items are worked by
    # then (7.5 s in the air).
    control.send("screenshot " + os.path.join(out, "kinetica-fan.png"))
    time.sleep(3)
    # At the wall, or by the fan should it pull.
    visit((-2, 0, -1), (-1, 0, -1))
    time.sleep(2)
    visit((-4, 0, -1), (-5, 0, -1))
    time.sleep(2)
    control.send("chat /clear @s minecraft:iron_nugget 0")
    time.sleep(1)
    lines = " ".join(str(line) for line in control.send("chat-log 2").get("chat", []))
    print("nuggets:", lines)
    nuggets = re.search(r"Found (\d+)", lines)
    if not nuggets or int(nuggets.group(1)) < 40:
        good = False
    # The belt: 32 iron in the east chest, its funnel used (taking out),
    # carried into the west one.
    x, y, z = base[0] + 8, base[1], base[2] + 2
    control.send(f"chat /item replace block {x} {y} {z} container.0 with minecraft:iron_ingot 32")
    visit((7, 0, 4), (7, 0, 2))
    control.send("mouse right click")
    time.sleep(1)
    control.send("screenshot " + os.path.join(out, "kinetica-funnel.png"))
    visit((5, 0, 4), (4, 0, 2))
    time.sleep(1)
    control.send("screenshot " + os.path.join(out, "kinetica-belt.png"))
    time.sleep(4)
    control.send(f"chat /data get block {x - 7} {y} {z} Items")
    time.sleep(1)
    lines = " ".join(str(line) for line in control.send("chat-log 1").get("chat", []))
    print("belt:", lines)
    if sum(int(n) for n in re.findall(r"count: ?(\d+)", lines)) < 32:
        good = False
    # The creative inventory: Tachyon's tab of the mods' items (the
    # cogwheel, top row) beside vanilla's.
    control.send("key e tap")
    time.sleep(1.5)
    control.send("screenshot " + os.path.join(out, "creative.png"))
    control.send("key escape tap")
    time.sleep(1)
    return good


# Create, got from Modrinth, on a stone floor south of the player: its
# blocks turned by Kinetica's engine (a Create motor's facing is its shaft's
# way). A gear train; a millstone on a motor, a hopper over it; a press two
# blocks over a depot (Create's way), a hopper beside the depot and one
# under it; a mixer two blocks over a basin, a motor over it, a hopper
# under the basin.
CREATE_SHOWCASE = [
    ("fill", (-8, -1, -2), (8, -1, 12), "minecraft:smooth_stone"),
    ("fill", (-8, 0, -2), (8, 12, 12), "minecraft:air"),
    ("setblock", (-4, 0, 4), "create:creative_motor[facing=east]"),
    ("setblock", (-3, 0, 4), "create:shaft[axis=x]"),
    ("setblock", (-2, 0, 4), "create:cogwheel[axis=x]"),
    ("setblock", (-2, 1, 5), "create:large_cogwheel[axis=x]"),
    ("setblock", (-1, 0, 4), "create:gearbox[axis=y]"),
    ("setblock", (0, 0, 4), "create:encased_fan[axis=x]"),
    ("setblock", (-1, 0, 3), "create:shaft[axis=z]"),
    ("setblock", (2, -1, 7), "create:creative_motor[facing=up]"),
    ("setblock", (2, 0, 7), "create:millstone"),
    ("setblock", (2, 1, 7), "minecraft:hopper[facing=down]"),
    ("setblock", (5, 0, 7), "create:depot[facing=north]"),
    ("setblock", (5, 2, 7), "create:mechanical_press[facing=north]"),
    ("setblock", (5, 2, 6), "create:creative_motor[facing=south]"),
    ("setblock", (6, 0, 7), "minecraft:hopper[facing=west]"),
    ("setblock", (5, -1, 7), "minecraft:hopper[facing=down]"),
    ("setblock", (-4, 0, 8), "create:basin"),
    ("setblock", (-4, 2, 8), "create:mechanical_mixer"),
    ("setblock", (-4, 3, 8), "create:creative_motor[facing=down]"),
    ("setblock", (-4, -1, 8), "minecraft:hopper[facing=down]"),
    # Two crushing wheels a block apart along x, turning against each other
    # (their motors on opposite sides), a hopper under the gap between.
    ("setblock", (-2, 1, 11), "create:crushing_wheel[axis=z]"),
    ("setblock", (-2, 1, 12), "create:creative_motor[facing=north]"),
    ("setblock", (0, 1, 11), "create:crushing_wheel[axis=z]"),
    ("setblock", (0, 1, 10), "create:creative_motor[facing=south]"),
    ("setblock", (-1, 0, 11), "minecraft:hopper[facing=down]"),
    # Create's water wheel (an OBJ model), along x.
    ("setblock", (3, 1, 4), "create:water_wheel[facing=east]"),
]


def create(control, out):
    """Create's blocks placed by commands and turned, in pictures; its
    millstone, press and mixer working by Create's own recipes (what they
    make is Create's items). Whether every command ran and each made its
    item."""
    player = control.send("status").get("player") or {}
    px, py, pz = player.get("position", [0.5, 64.0, 0.5])
    base = [math.floor(px), math.floor(py), math.floor(pz)]
    at = lambda o: " ".join(str(b + d) for b, d in zip(base, o))
    good = True
    commands = [f"{c[0]} {at(c[1])} {at(c[2])} {c[3]}" if c[0] == "fill" else f"{c[0]} {at(c[1])} {c[2]}"
                for c in CREATE_SHOWCASE]
    wear = ["item replace entity @s armor.head with kinetica:engineers_goggles", "kill @e[type=item]"]
    for command in commands + wear:
        if "error" in control.send("chat /" + command):
            good = False
    time.sleep(2)
    chat = " ".join(str(line) for line in control.send("chat-log 30").get("chat", []))
    for word in ["Unknown", "Incorrect", "Invalid", "Could not", "Failed"]:
        if word in chat:
            print("chat says", word, ":", chat)
            good = False

    def visit(stand, target):
        x, y, z = [b + d for b, d in zip(base, stand)]
        control.send(f"chat /tp @s {x + 0.5} {y} {z + 0.5}")
        time.sleep(1.5)
        look_at(control, [x + 0.5, y + 1.62, z + 0.5], [b + d for b, d in zip(base, target)])
        time.sleep(1)

    def count(item):
        control.send(f"chat /clear @s {item} 0")
        time.sleep(1)
        lines = " ".join(str(line) for line in control.send("chat-log 2").get("chat", []))
        found = re.search(r"Found (\d+)", lines)
        print(item, ":", lines)
        return int(found.group(1)) if found else 0

    def until(made, least, seconds=30):
        """What `made` counts, asked again until it is `least` or `seconds`
        pass."""
        deadline = time.time() + seconds
        n = made()
        while n < least and time.time() < deadline:
            time.sleep(3)
            n = made()
        return n

    def taken(hopper, item):
        """A count of `item` in the first five slots of the hopper at
        `hopper`, copied into the hotbar."""
        def made():
            for slot in range(5):
                control.send(f"chat /item replace entity @s hotbar.{slot} from block {at(hopper)} container.{slot}")
            time.sleep(1)
            return count(item)
        return made

    def ground():
        """The flour the millstone ground so far, taken with an empty hand."""
        control.send("mouse right click")
        time.sleep(1)
        return count("create:wheat_flour")

    # The work: wheat in the millstone's hopper, iron in the depot's, and
    # andesite with iron nuggets put in the basin with uses.
    control.send(f"chat /item replace block {at((2, 1, 7))} container.0 with minecraft:wheat 4")
    control.send(f"chat /item replace block {at((6, 0, 7))} container.0 with minecraft:iron_ingot 4")
    visit((-4, 0, 10), (-4, 0, 8))
    for item in ["minecraft:andesite 2", "minecraft:iron_nugget 2"]:
        control.send(f"chat /item replace entity @s weapon.mainhand with {item}")
        time.sleep(0.5)
        control.send("mouse right click")
        time.sleep(0.5)
    # Four raw iron dropped into the crushing wheels' gap.
    x, y, z = base[0] - 1 + 0.5, base[1] + 2.3, base[2] + 11 + 0.5
    item = '{Item:{id:"minecraft:raw_iron",count:4},PickupDelay:40s}'
    control.send(f"chat /summon minecraft:item {x} {y} {z} {item}")
    # The whole showcase, twice a second apart (the parts turn between).
    visit((0, 0, -1), (0, 0, 5))
    control.send("look 0 25")
    time.sleep(2)
    for name in ["create", "create-later"]:
        if not control.send("screenshot " + os.path.join(out, f"{name}.png")).get("ok"):
            good = False
        time.sleep(1)
    # The cogwheels with the goggles' speed, the press's head, the mixer.
    visit((-2, 0, 2), (-2, 0, 4))
    control.send("screenshot " + os.path.join(out, "create-cogs.png"))
    visit((5, 0, 4), (5, 1, 7))
    control.send("screenshot " + os.path.join(out, "create-press.png"))
    visit((-4, 0, 10), (-4, 1, 8))
    control.send("screenshot " + os.path.join(out, "create-mixer.png"))
    visit((-1, 0, 8), (-1, 1, 11))
    control.send("screenshot " + os.path.join(out, "create-crusher.png"))
    # How the blocks look (dark faces, missing parts): a row behind the start, a crank on a shaft
    # and one on its side, the saw, the arm, the millstone, a gearbox and the speed controller.
    looks = [
        ("setblock", (-5, 0, -4), "create:shaft[axis=y]"), ("setblock", (-5, 1, -4), "create:hand_crank[facing=up]"),
        ("setblock", (-3, 0, -4), "create:shaft[axis=z]"), ("setblock", (-3, 0, -5), "create:hand_crank[facing=north]"),
        ("setblock", (-1, 0, -4), "create:mechanical_saw[facing=up]"), ("setblock", (1, 0, -4), "create:mechanical_arm"),
        ("setblock", (3, 0, -4), "create:millstone"), ("setblock", (5, 0, -4), "create:gearbox"),
        ("setblock", (7, 0, -4), "create:rotation_speed_controller"),
        # A drill turned from above, drilling the cobblestone under it into the hopper below.
        ("setblock", (-7, -1, -4), "minecraft:hopper[facing=down]"),
        ("setblock", (-7, 0, -4), "minecraft:cobblestone"),
        ("setblock", (-7, 1, -4), "create:mechanical_drill[facing=down]"),
        ("setblock", (-7, 2, -4), "create:creative_motor[facing=down]"),
    ]
    for c in looks:
        if "error" in control.send(f"chat /{c[0]} {at(c[1])} {c[2]}"):
            good = False
    time.sleep(2)
    visit((1, 0, -1), (1, 0, -4))
    control.send("screenshot " + os.path.join(out, "create-looks.png"))
    visit((-4, 0, -2), (-4, 1, -4))
    control.send("screenshot " + os.path.join(out, "create-cranks.png"))
    # What they made, by now: the millstone's taken with an empty hand, the
    # depot's and the basin's out of the hoppers under them.
    control.send("chat /item replace entity @s weapon.mainhand with minecraft:air")
    visit((2, 0, 9), (2, 0, 7))
    flour = until(ground, 4)
    sheets = until(taken((5, -1, 7), "create:iron_sheet"), 3)
    alloy = until(taken((-4, -1, 8), "create:andesite_alloy"), 1)
    crushed = until(taken((-1, 0, 11), "create:crushed_raw_iron"), 4)
    drilled = until(taken((-7, -1, -4), "minecraft:cobblestone"), 1)
    print("create made: flour", flour, "sheets", sheets, "alloy", alloy, "crushed", crushed, "drilled", drilled)
    return good and flour >= 4 and sheets >= 3 and alloy >= 1 and crushed >= 4 and drilled >= 1


def lost_realms(control, out):
    """Lost Realms' structures where the world made them: the nearest of
    each, the player set down at it (a picture), which does Chronicles'
    quests to find them. Whether each was found and the player landed."""
    good = True
    for name in ["workshop_ruin", "watchtower"]:
        control.send(f"chat /locate structure lost_realms:{name}")
        time.sleep(8)
        lines = " ".join(str(line) for line in control.send("chat-log 3").get("chat", []))
        print("locate:", lines)
        at = re.search(r"lost_realms:" + name + r" is at \[(-?\d+), ~, (-?\d+)\]", lines)
        if not at:
            good = False
            continue
        x, z = int(at.group(1)), int(at.group(2))
        control.send(f"chat /tp @s {x + 0.5} 160 {z + 0.5}")
        time.sleep(3)
        deadline = time.time() + 90
        while not (control.send("status").get("player") or {}).get("on_ground"):
            if time.time() > deadline:
                print(name, ": the player never landed")
                good = False
                break
            time.sleep(2)
        # The owner's look (once a second) marks the quest done.
        time.sleep(4)
        control.send("look 30 35")
        time.sleep(1)
        control.send("screenshot " + os.path.join(out, f"found-{name}.png"))
        control.send("look 0 90")
        time.sleep(1)
        control.send("screenshot " + os.path.join(out, f"found-{name}-below.png"))
        # Its blocks round where the player landed: stone bricks, which the
        # ground there doesn't have, counted by turning them to air.
        y = math.floor((control.send("status").get("player") or {}).get("position", [0, 64, 0])[1])
        bricks = 0
        for brick in ["stone_bricks", "mossy_stone_bricks", "cracked_stone_bricks"]:
            control.send(f"chat /fill {x - 10} {y - 20} {z - 10} {x + 10} {y + 5} {z + 10} "
                         f"minecraft:air replace minecraft:{brick}")
            time.sleep(1)
            for line in control.send("chat-log 1").get("chat", []):
                filled = re.search(r"filled (\d+)", str(line))
                bricks += int(filled.group(1)) if filled else 0
        print(name, "stone bricks where it was found:", bricks)
        if bricks < 10:
            good = False
    return good


def chronicles(control, out):
    """Chronicles' book: opened with its key, the first quest's rewards
    claimed. Whether Claim was there to click."""
    control.send("key j tap")
    time.sleep(3)
    control.send("screenshot " + os.path.join(out, "quests.png"))
    reply = control.send("click-button Claim")
    print("claim:", reply)
    time.sleep(3)
    control.send("screenshot " + os.path.join(out, "quests-claimed.png"))
    control.send("key escape tap")
    time.sleep(1)
    return bool(reply.get("ok"))


def orbital(control, out):
    """Orbital Laser (built in): the Designator given, its wheel open, then one
    Plasma strike with terrain damage off (light and sound only) aimed at the
    ground ahead, pictured as it comes down. Whether the mod answered and the
    strike began (its status panel's or the action bar's countdown)."""
    control.send("chat /orbital give")
    time.sleep(3)
    control.send("chat /orbital terrain off")
    time.sleep(1)
    control.send("chat /orbital plasma")
    time.sleep(2)
    chat = " ".join(str(line) for line in control.send("chat-log 8").get("chat", []))
    print("orbital chat:", chat)
    control.send("chat /orbital wheel")
    time.sleep(3)
    control.send("screenshot " + os.path.join(out, "orbital-wheel.png"))
    control.send("key escape tap")
    time.sleep(2)
    control.send("resume")
    # The scene's inventory is full: the Designator goes into the hand, and it's day for the pictures.
    control.send("chat /item replace entity @s weapon.mainhand with orbital:laser_designator")
    control.send("chat /time set noon")
    control.send("look 0 35")
    time.sleep(2)
    control.send("mouse right down")
    time.sleep(1.5)
    control.send("mouse right up")
    shots = 0
    for i, wait in enumerate([4, 6, 8]):
        time.sleep(wait)
        if control.send("screenshot " + os.path.join(out, f"orbital-strike-{i}.png")).get("ok"):
            shots += 1
    after = " ".join(str(line) for line in control.send("chat-log 8").get("chat", []))
    print("orbital chat after:", after)
    stop = " ".join(str(line) for line in control.send("chat /orbital stop").get("chat", []))
    time.sleep(2)
    stop += " ".join(str(line) for line in control.send("chat-log 2").get("chat", []))
    print("orbital stop:", stop)
    broken = "isn't installed" in chat or "Unknown" in chat or "unknown" in chat
    fired = "No strike under way" not in stop
    return shots == 3 and not broken and fired


def main():
    game, home, out = sys.argv[1:4]
    # "create": Create's scene alone, in a game folder Create was got into.
    mode = sys.argv[4] if len(sys.argv) > 4 else "tachyon"
    os.makedirs(home, exist_ok=True)
    os.makedirs(out, exist_ok=True)
    endpoint = os.path.join(home, "control.json")
    env = dict(
        os.environ,
        TACHYON_GAME="1",
        HORIZON_HOME=home,
        HORIZON_CONTROL=endpoint,
        HORIZON_NO_HEARTBEAT="1",
        HORIZON_NO_DISCORD="1",
    )
    world = "Create" if mode == "create" else "Scene"
    args = [game, "--world", world, "--creative", "--seed", "5", "--render-distance", "4",
            "--width", "1280", "--height", "720", "--control", "--no-pause"]
    proc = subprocess.Popen(args, cwd=home, env=env)
    failed = []
    control = None
    try:
        control = Control(endpoint, time.time() + 600)
        # The world: a player standing, and time for the chunks and the minimap.
        deadline = time.time() + 600
        while True:
            status = control.send("status")
            player = status.get("player")
            if player and player.get("on_ground"):
                break
            if time.time() > deadline or proc.poll() is not None:
                raise SystemExit("the world never loaded")
            time.sleep(3)
        control.send("resume")
        time.sleep(20)
        control.send("resume")
        if mode == "create":
            if not create(control, out):
                failed.append("create")
            control.send("quit")
            return finish(failed, "the create scene is done")
        for name in ["world", "world-again"]:
            reply = control.send("screenshot " + os.path.join(out, f"{name}.png"))
            if not reply.get("ok"):
                failed.append(name)
            time.sleep(5)
        if not kinetica(control, out):
            failed.append("kinetica")
        if not lost_realms(control, out):
            failed.append("lost realms")
        if not chronicles(control, out):
            failed.append("chronicles")
        if not orbital(control, out):
            failed.append("orbital laser")
        control.send("options mods")
        time.sleep(2)
        if not control.send("screenshot " + os.path.join(out, "mods.png")).get("ok"):
            failed.append("mods")
        control.send("quit")
    finally:
        if control is not None:
            with open(os.path.join(out, "scene.json"), "w", encoding="utf-8") as f:
                json.dump(control.log, f, indent=1)
        try:
            proc.wait(timeout=60)
        except subprocess.TimeoutExpired:
            proc.kill()
    finish_tachyon(home, failed)


def finish(failed, done):
    if failed:
        raise SystemExit("failed: " + ", ".join(failed))
    print(done)


def finish_tachyon(home, failed):
    """The Tachyon scene's checks of the world's folder, then its end."""
    # The motor's speed is kept in the world's folder.
    kept = [os.path.join(d, f) for d, _, files in os.walk(home) for f in files if f.startswith("kinetica-")]
    for path in kept:
        print(path, open(path, encoding="utf-8").read())
    if not kept:
        failed.append("motor speed kept")
    # The first quest's rewards were taken, in the world's folder.
    quests = [os.path.join(d, f) for d, _, files in os.walk(home) for f in files
              if os.path.basename(d) == "chronicles" and f.endswith(".json")]
    for path in quests:
        print(path, open(path, encoding="utf-8").read())
    progress = [json.load(open(p, encoding="utf-8")) for p in quests]
    if not any("welcome" in p.get("claimed", []) for p in progress):
        failed.append("quest claimed")
    # Done by the motor turning near the player, and by standing in the
    # structures the world made.
    for quest in ["in_motion", "full_speed", "millstone", "crushed_ore", "pressing", "mixing", "bulk_washing",
                  "conveyor", "lost_workshop", "watchtower"]:
        if not any(quest in p.get("done", []) for p in progress):
            failed.append("quest " + quest)
    finish(failed, "the scene is done")


if __name__ == "__main__":
    main()
