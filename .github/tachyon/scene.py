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
    ("fill", (-8, -1, -2), (8, -1, 12), "minecraft:smooth_stone"),
    ("fill", (-8, 0, -2), (8, 30, 12), "minecraft:air"),
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
]
ITEMS = ["shaft", "cogwheel", "large_cogwheel", "gearbox", "hand_crank", "clutch", "gearshift",
         "water_wheel", "windmill", "creative_motor", "ferrostone_alloy", "raw_tin", "tin_ingot",
         "bronze_ingot"]


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
    for word in ["Unknown", "Incorrect", "Invalid", "Could not"]:
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
    return good


def main():
    game, home, out = sys.argv[1:4]
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
    args = [game, "--world", "Scene", "--creative", "--seed", "5", "--render-distance", "4",
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
        for name in ["world", "world-again"]:
            reply = control.send("screenshot " + os.path.join(out, f"{name}.png"))
            if not reply.get("ok"):
                failed.append(name)
            time.sleep(5)
        if not kinetica(control, out):
            failed.append("kinetica")
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
    # The motor's speed is kept in the world's folder.
    kept = [os.path.join(d, f) for d, _, files in os.walk(home) for f in files if f.startswith("kinetica-")]
    for path in kept:
        print(path, open(path, encoding="utf-8").read())
    if not kept:
        failed.append("motor speed kept")
    if failed:
        raise SystemExit("failed: " + ", ".join(failed))
    print("the scene is done")


if __name__ == "__main__":
    main()
