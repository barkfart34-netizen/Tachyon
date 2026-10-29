"""Tachyon in a world, driven through its control socket, with pictures.

    python scene.py GAME HOME OUT

Starts GAME (Tachyon.exe, which runs the game with TACHYON_GAME set) in a
new creative world in HOME, waits for the world, then takes pictures into
OUT: the world with the HUD (the built-in Minimap and Coordinates HUD),
Options > Mods (the built-in mods' rows), and writes every reply to
OUT/scene.json. Exits nonzero when a step fails, so the job shows it.
"""

import json
import os
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
        where = control.send("chat /where")
        if not any(" in minecraft:" in str(line) for line in where.get("chat", [])):
            failed.append("/where")
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
    if failed:
        raise SystemExit("failed: " + ", ".join(failed))
    print("the scene is done")


if __name__ == "__main__":
    main()
