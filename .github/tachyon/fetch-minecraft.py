#!/usr/bin/env python3
"""Downloads Minecraft: Java Edition at the version research-lock.json pins
from Mojang's own servers into a launcher-style folder (versions/, libraries/,
assets/), the layout horizon-prepare reads, plus the server jar. Every file is
checked against the SHA-1 Mojang publishes, and the version file and jars
against the pins.

    fetch-minecraft.py DIR

The files are Mojang's: they stay in the build and are never committed."""
import concurrent.futures
import hashlib
import json
import os
import sys
import time
import urllib.request

MANIFEST = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
OBJECTS = "https://resources.download.minecraft.net"


def get(url):
    for attempt in range(5):
        try:
            with urllib.request.urlopen(url, timeout=60) as r:
                return r.read()
        except Exception as e:  # noqa: BLE001 - retried, then raised
            if attempt == 4:
                raise RuntimeError(f"{url}: {e}") from e
            time.sleep(2 + attempt * 3)


def sha1(data):
    return hashlib.sha1(data).hexdigest()


def fetch(url, path, want):
    if os.path.isfile(path):
        with open(path, "rb") as f:
            if sha1(f.read()) == want:
                return
    data = get(url)
    if sha1(data) != want:
        raise RuntimeError(f"{url}: sha1 {sha1(data)}, expected {want}")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path + ".part", "wb") as f:
        f.write(data)
    os.replace(path + ".part", path)


# The libraries of the system this runs on, as Mojang's launcher picks them (a Windows horizon-prepare wants the
# Windows natives).
OS_NAME = {"win32": "windows", "darwin": "osx"}.get(sys.platform, "linux")


def allowed(library):
    rules = library.get("rules")
    if not rules:
        return True
    allow = False
    for rule in rules:
        os_rule = rule.get("os")
        if os_rule and os_rule.get("name") not in (None, OS_NAME):
            continue
        allow = rule["action"] == "allow"
    return allow


def main():
    out = sys.argv[1]
    here = os.path.dirname(os.path.abspath(__file__))
    with open(os.path.join(here, "..", "..", "research-lock.json"), encoding="utf-8") as f:
        pins = json.load(f)["minecraft"]
    version = pins["version"]
    accepted = {pins["manifest_sha1"], *pins.get("accepted_manifest_sha1", [])}

    listing = json.loads(get(MANIFEST))
    entry = next(v for v in listing["versions"] if v["id"] == version)
    if entry["sha1"] not in accepted:
        print(f"note: Mojang's {version}.json is {entry['sha1']}, not one research-lock.json lists")
    version_dir = os.path.join(out, "versions", version)
    manifest_path = os.path.join(version_dir, f"{version}.json")
    fetch(entry["url"], manifest_path, entry["sha1"])
    with open(manifest_path, encoding="utf-8") as f:
        manifest = json.load(f)

    files = []
    client = manifest["downloads"]["client"]
    assert client["sha1"] == pins["client_sha1"], "the client jar is not the pinned one"
    files.append((client["url"], os.path.join(version_dir, f"{version}.jar"), client["sha1"]))
    server = manifest["downloads"]["server"]
    assert server["sha1"] == pins["server_sha1"], "the server jar is not the pinned one"
    files.append((server["url"], os.path.join(out, "server.jar"), server["sha1"]))
    for library in filter(allowed, manifest["libraries"]):
        artifact = library.get("downloads", {}).get("artifact")
        if artifact:
            path = os.path.join(out, "libraries", *artifact["path"].split("/"))
            files.append((artifact["url"], path, artifact["sha1"]))
    logging = manifest.get("logging", {}).get("client", {}).get("file")
    if logging:
        files.append((logging["url"], os.path.join(out, "assets", "log_configs", logging["id"]), logging["sha1"]))

    index = manifest["assetIndex"]
    index_path = os.path.join(out, "assets", "indexes", f"{index['id']}.json")
    fetch(index["url"], index_path, index["sha1"])
    with open(index_path, encoding="utf-8") as f:
        objects = json.load(f)["objects"]
    seen = set()
    for obj in objects.values():
        h = obj["hash"]
        if h in seen:
            continue
        seen.add(h)
        files.append((f"{OBJECTS}/{h[:2]}/{h}", os.path.join(out, "assets", "objects", h[:2], h), h))

    print(f"Minecraft {version}: {len(files)} files")
    done = 0
    with concurrent.futures.ThreadPoolExecutor(16) as pool:
        for future in concurrent.futures.as_completed(pool.submit(fetch, *f) for f in files):
            future.result()
            done += 1
            if done % 500 == 0:
                print(f"  {done}/{len(files)}")
    print("done")


if __name__ == "__main__":
    main()
