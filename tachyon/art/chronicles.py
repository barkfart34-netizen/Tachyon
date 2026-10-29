"""Chronicles' content: the modpack's quests (chapters, what each asks for
and gives) and their names in English and Indonesian, written into
tachyon/content/chronicles (a Horizon content mod, API 0.2). Tachyon's
content engine reads the quests (game/tachyon_engine/chronicles.rs).

    python tachyon/art/chronicles.py

Run it again after a change; it rewrites the whole mod.
"""

import json
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "content" / "chronicles"
NS = "chronicles"


def stack(item, count=1):
    return {"item": item, "count": count}


def quest(id, icon, names, texts, tasks=(), rewards=(), after=()):
    return {"id": id, "icon": icon, "after": list(after), "tasks": list(tasks), "rewards": list(rewards),
            "names": names, "texts": texts}


# Each quest: what it is called and says (English, Indonesian), the items
# the player must hold at once, what it gives, and the quests before it.
CHAPTERS = [
    {"id": "machine_age", "icon": "kinetica:cogwheel", "names": ("The Machine Age", "Era Mesin"), "quests": [
        quest("welcome", "minecraft:writable_book",
              ("Welcome", "Selamat Datang"),
              ("A world of gears and old magic. Claim a little help to start with.",
               "Dunia roda gigi dan sihir kuno. Ambil sedikit bekal untuk memulai."),
              rewards=[stack("minecraft:bread", 6), stack("minecraft:torch", 16)]),
        quest("first_alloy", "kinetica:ferrostone_alloy",
              ("Ferrostone", "Ferrostone"),
              ("Andesite or tuff with iron nuggets makes Ferrostone Alloy, what every machine is built from.",
               "Andesit atau tuff dengan butiran besi menjadi Paduan Ferrostone, bahan semua mesin."),
              tasks=[stack("kinetica:ferrostone_alloy", 4)],
              rewards=[stack("kinetica:shaft", 6), stack("minecraft:iron_nugget", 9)], after=["welcome"]),
        quest("shafts_and_cogs", "kinetica:shaft",
              ("Shafts and Cogs", "Poros dan Roda Gigi"),
              ("Shafts carry turning along a line; cogwheels pass it sideways, and reverse it.",
               "Poros meneruskan putaran lurus; roda gigi meneruskannya ke samping dan membaliknya."),
              tasks=[stack("kinetica:shaft", 8), stack("kinetica:cogwheel", 2)],
              rewards=[stack("kinetica:large_cogwheel", 2)], after=["first_alloy"]),
        quest("hand_power", "kinetica:hand_crank",
              ("By Hand", "Tenaga Tangan"),
              ("A hand crank turns what it sits on while you use it. Sneak to turn it the other way.",
               "Engkol tangan memutar poros di bawahnya selama dipakai. Jongkok untuk arah sebaliknya."),
              tasks=[stack("kinetica:hand_crank")],
              rewards=[stack("kinetica:cogwheel", 4)], after=["shafts_and_cogs"]),
        quest("goggles", "kinetica:engineers_goggles",
              ("Seeing Speed", "Melihat Kecepatan"),
              ("Engineer's Goggles show how fast a part turns when you look at it.",
               "Kacamata Insinyur menampilkan kecepatan bagian yang kamu lihat."),
              tasks=[stack("kinetica:engineers_goggles")],
              rewards=[stack("kinetica:ferrostone_alloy", 8)], after=["shafts_and_cogs"]),
        quest("water_power", "kinetica:water_wheel",
              ("Running Water", "Air Mengalir"),
              ("A water wheel turns with the water flowing past it, faster with water on more sides.",
               "Kincir air berputar oleh air yang mengalir melewatinya, lebih cepat bila lebih banyak sisi."),
              tasks=[stack("kinetica:water_wheel")],
              rewards=[stack("kinetica:gearbox")], after=["hand_power"]),
        quest("wind_power", "kinetica:windmill",
              ("Open Sky", "Langit Terbuka"),
              ("A windmill needs open sky above it, and turns faster the higher it stands.",
               "Kincir angin butuh langit terbuka, dan berputar lebih cepat di tempat tinggi."),
              tasks=[stack("kinetica:windmill")],
              rewards=[stack("kinetica:clutch"), stack("kinetica:gearshift")], after=["hand_power"]),
        quest("control", "kinetica:gearshift",
              ("In Control", "Kendali"),
              ("A powered clutch parts a line; a powered gearshift turns it the other way.",
               "Kopling yang diberi sinyal memutus jalur; pemindah gigi yang diberi sinyal membalik arahnya."),
              tasks=[stack("kinetica:gearbox"), stack("kinetica:clutch"), stack("kinetica:gearshift")],
              rewards=[stack("kinetica:ferrostone_casing", 8)], after=["water_power", "wind_power"]),
    ]},
    {"id": "metals", "icon": "kinetica:tin_ingot", "names": ("Metals", "Logam"), "quests": [
        quest("tin", "kinetica:raw_tin",
              ("Tin", "Timah"),
              ("Tin lies in the ground where copper does. Smelt the raw tin into ingots.",
               "Timah ada di dalam tanah di tempat yang sama dengan tembaga. Lebur timah mentah jadi batangan."),
              tasks=[stack("kinetica:tin_ingot", 8)],
              rewards=[stack("minecraft:copper_ingot", 12)], after=["welcome"]),
        quest("bronze", "kinetica:bronze_ingot",
              ("The Bronze Age", "Zaman Perunggu"),
              ("Three copper and one tin make four bronze.",
               "Tiga tembaga dan satu timah menjadi empat perunggu."),
              tasks=[stack("kinetica:bronze_ingot", 8)],
              rewards=[stack("kinetica:engineers_goggles")], after=["tin"]),
    ]},
]


def main():
    if ROOT.exists():
        shutil.rmtree(ROOT)
    ROOT.mkdir(parents=True)
    (ROOT / "horizon-mod.toml").write_text(
        "# Tachyon: Aether & Gears. Chronicles, the quests: written by tachyon/art/chronicles.py.\n"
        'id = "chronicles"\nname = "Chronicles"\nversion = "0.1.0"\napi = "0.2"\nside = "both"\n'
        'capabilities = ["registries"]\n', encoding="utf-8")
    en = {"chronicles.title": "Chronicles", "chronicles.claim": "Claim", "chronicles.claimed": "Claimed",
          "chronicles.locked": "Finish the quests before it first", "chronicles.tasks": "Hold at once:",
          "chronicles.rewards": "Rewards:", "chronicles.done": "Quest complete: %s"}
    idn = {"chronicles.title": "Kronik", "chronicles.claim": "Klaim", "chronicles.claimed": "Sudah diklaim",
           "chronicles.locked": "Selesaikan quest sebelumnya dulu", "chronicles.tasks": "Bawa sekaligus:",
           "chronicles.rewards": "Hadiah:", "chronicles.done": "Quest selesai: %s"}
    book = {"chapters": []}
    for chapter in CHAPTERS:
        en[f"chronicles.chapter.{chapter['id']}"], idn[f"chronicles.chapter.{chapter['id']}"] = chapter["names"]
        quests = []
        for q in chapter["quests"]:
            key = f"chronicles.quest.{q['id']}"
            en[key], idn[key] = q["names"]
            en[f"{key}.text"], idn[f"{key}.text"] = q["texts"]
            quests.append({k: v for k, v in q.items() if k not in ("names", "texts")})
        book["chapters"].append({"id": chapter["id"], "icon": chapter["icon"], "quests": quests})
    ids = [q["id"] for c in book["chapters"] for q in c["quests"]]
    assert len(ids) == len(set(ids)), "quest ids are unique"
    for c in book["chapters"]:
        for q in c["quests"]:
            assert all(a in ids for a in q["after"]), q["id"]
    for path, value in [(ROOT / "data" / NS / "tachyon" / "quests.json", book),
                        (ROOT / "assets" / NS / "lang" / "en_us.json", en),
                        (ROOT / "assets" / NS / "lang" / "id_id.json", idn)]:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"chronicles: {len(ids)} quests in {ROOT}")


if __name__ == "__main__":
    main()
