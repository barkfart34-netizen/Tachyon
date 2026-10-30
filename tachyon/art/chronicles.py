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


def quest(id, icon, names, texts, tasks=(), rewards=(), after=(), turning=None, find=None):
    """A quest: `tasks` the items to hold at once, `turning` a machine near
    the player at that many RPM, `find` a structure to stand in."""
    q = {"id": id, "icon": icon, "after": list(after), "tasks": list(tasks), "rewards": list(rewards),
         "names": names, "texts": texts}
    if turning is not None:
        q["turning"] = turning
    if find is not None:
        q["find"] = find
    return q


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
        quest("in_motion", "kinetica:cogwheel",
              ("In Motion", "Bergerak"),
              ("Build a machine and set it turning: any part near you at 16 RPM or more will do.",
               "Bangun mesin dan buat ia berputar: bagian mana pun di dekatmu, 16 RPM atau lebih."),
              turning=16, rewards=[stack("kinetica:ferrostone_casing", 4)], after=["hand_power"]),
        quest("millstone", "kinetica:millstone",
              ("Daily Bread", "Roti Harian"),
              ("A millstone grinds what is put in it while it turns; use it with an empty hand to take out what "
               "it ground. Grind wheat into flour, and bake the flour into bread. Mind the stress: a hand crank "
               "drives only two.",
               "Batu gilingan menggiling isinya selama berputar; pakai dengan tangan kosong untuk mengambil "
               "hasilnya. Giling gandum jadi tepung, lalu panggang tepungnya jadi roti. Perhatikan beban: satu "
               "engkol tangan hanya kuat memutar dua."),
              tasks=[stack("kinetica:wheat_flour", 4)],
              rewards=[stack("minecraft:wheat", 16), stack("kinetica:large_cogwheel", 2)], after=["in_motion"]),
        quest("crushed_ore", "kinetica:crushed_iron",
              ("Crushed Ore", "Bijih Hancur"),
              ("A millstone grinds raw ore into crushed ore, and now and then gives one more. Smelted, each "
               "crushed ore gives an ingot.",
               "Batu gilingan menggiling bijih mentah jadi bijih hancur, sesekali lebih satu. Dilebur, tiap "
               "bijih hancur menjadi satu batangan."),
              tasks=[stack("kinetica:crushed_iron", 4)],
              rewards=[stack("minecraft:hopper", 2)], after=["millstone"]),
        quest("pressing", "kinetica:mechanical_press",
              ("Pressed Metal", "Logam Tempa"),
              ("A mechanical press turning over a depot presses what is put on the depot: ingots into sheets. "
               "A hopper beside the depot fills it, and one under it takes the sheets out.",
               "Mesin press yang berputar di atas depot menempa isi depot: batangan jadi lembaran. Hopper di "
               "samping depot mengisinya, dan hopper di bawahnya mengambil lembarannya."),
              tasks=[stack("kinetica:iron_sheet", 4)],
              rewards=[stack("kinetica:depot", 2), stack("minecraft:iron_ingot", 8)], after=["in_motion"]),
        quest("mixing", "kinetica:mechanical_mixer",
              ("Stirred Together", "Diaduk Rata"),
              ("A mechanical mixer turning over a basin, or two blocks over it, mixes what is put in the basin "
               "once it holds all a recipe asks for: andesite and an iron nugget into ferrostone alloy, copper "
               "and tin into bronze, and more. A hopper under the basin takes out what was mixed.",
               "Mesin pengaduk yang berputar di atas baskom (langsung, atau dua blok di atasnya) mengaduk isi "
               "baskom begitu semua bahan resep lengkap: andesit dan butiran besi jadi paduan ferrostone, "
               "tembaga dan timah jadi perunggu, dan lainnya. Hopper di bawah baskom mengambil hasilnya."),
              tasks=[stack("kinetica:mechanical_mixer"), stack("kinetica:basin")],
              rewards=[stack("minecraft:andesite", 16), stack("minecraft:iron_nugget", 16)], after=["pressing"]),
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
        quest("full_speed", "kinetica:large_cogwheel",
              ("Full Speed", "Kecepatan Penuh"),
              ("A large cogwheel driving a small one doubles the speed. Step a wheel's turning up to 128 RPM.",
               "Roda gigi besar yang memutar roda gigi kecil menggandakan kecepatan. Naikkan putaran kincir "
               "sampai 128 RPM."),
              turning=128, rewards=[stack("kinetica:bronze_ingot", 8), stack("kinetica:ferrostone_alloy", 16)],
              after=["control"]),
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
    {"id": "lost_realms", "icon": "minecraft:compass", "names": ("Lost Realms", "Dunia yang Hilang"), "quests": [
        quest("lost_workshop", "kinetica:cogwheel",
              ("The Lost Workshop", "Bengkel yang Hilang"),
              ("Tinkerers once worked in the forests and plains. Find one of their ruined workshops.",
               "Dulu para perajin bekerja di hutan dan dataran. Temukan salah satu bengkel mereka yang runtuh."),
              find="lost_realms:workshop_ruin",
              rewards=[stack("minecraft:spyglass"), stack("kinetica:ferrostone_alloy", 8)], after=["welcome"]),
        quest("watchtower", "minecraft:spyglass",
              ("The Old Watch", "Penjaga Lama"),
              ("The old realm watched its borders from stone towers. Climb into one of them.",
               "Kerajaan lama mengawasi perbatasannya dari menara batu. Masuklah ke salah satunya."),
              find="lost_realms:watchtower",
              rewards=[stack("minecraft:arrow", 16), stack("kinetica:tin_ingot", 6)], after=["welcome"]),
    ]},
]


def main():
    if ROOT.exists():
        shutil.rmtree(ROOT)
    ROOT.mkdir(parents=True)
    (ROOT / "horizon-mod.toml").write_text(
        "# Tachyon: Aether & Gears. Chronicles, the quests: written by tachyon/art/chronicles.py.\n"
        'id = "chronicles"\nname = "Chronicles"\nversion = "0.5.0"\napi = "0.2"\nside = "both"\n'
        'capabilities = ["registries"]\n', encoding="utf-8")
    en = {"chronicles.title": "Chronicles", "chronicles.claim": "Claim", "chronicles.claimed": "Claimed",
          "chronicles.locked": "Finish the quests before it first", "chronicles.tasks": "Hold at once:",
          "chronicles.rewards": "Rewards:", "chronicles.done": "Quest complete: %s",
          "chronicles.turning": "A machine near you turning at %s RPM", "chronicles.find": "Find: %s"}
    idn = {"chronicles.title": "Kronik", "chronicles.claim": "Klaim", "chronicles.claimed": "Sudah diklaim",
           "chronicles.locked": "Selesaikan quest sebelumnya dulu", "chronicles.tasks": "Bawa sekaligus:",
           "chronicles.rewards": "Hadiah:", "chronicles.done": "Quest selesai: %s",
           "chronicles.turning": "Mesin di dekatmu berputar %s RPM", "chronicles.find": "Temukan: %s"}
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
