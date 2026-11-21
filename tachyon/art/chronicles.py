"""Chronicles' content: the modpack's quests (chapters, what each asks for
and gives) and their names in English and Indonesian, written into
tachyon/content/chronicles (a Horizon content mod, API 0.2) in Horizon Quests'
data-pack format (`data/chronicles/horizon_quests/`).

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


def quest(
    id,
    icon,
    names,
    texts,
    x,
    y,
    tasks=(),
    rewards=(),
    after=(),
    turning=None,
    find=None,
    shape=None,
    size=None,
    checkmark=None,
):
    """A quest: `tasks` the items to hold at once, `turning` a machine near
    the player at that many RPM, `find` a structure to stand in."""
    q = {
        "id": id,
        "icon": icon,
        "x": float(x),
        "y": float(y),
        "after": list(after),
        "tasks": list(tasks),
        "rewards": list(rewards),
        "names": names,
        "texts": texts,
    }
    if turning is not None:
        q["turning"] = turning
    if find is not None:
        q["find"] = find
    if shape is not None:
        q["shape"] = shape
    if size is not None:
        q["size"] = float(size)
    if checkmark is not None:
        q["checkmark"] = checkmark
    return q


# Each quest: what it is called and says (English, Indonesian), its position on
# the chapter canvas, the tasks to complete, what it gives, and its dependencies.
CHAPTERS = [
    {
        "id": "machine_age",
        "file": "01_machine_age.json",
        "order": 1,
        "icon": "kinetica:cogwheel",
        "names": ("The Machine Age", "Era Mesin"),
        "quests": [
            quest(
                "welcome",
                "minecraft:writable_book",
                ("Welcome", "Selamat Datang"),
                (
                    "A world of gears and old magic. Claim a little help to start with.",
                    "Dunia roda gigi dan sihir kuno. Ambil sedikit bekal untuk memulai.",
                ),
                0.0,
                0.0,
                shape="circle",
                size=1.25,
                checkmark=("Begin your journey", "Mulai perjalananmu"),
                rewards=[stack("minecraft:bread", 6), stack("minecraft:torch", 16)],
            ),
            quest(
                "first_alloy",
                "kinetica:ferrostone_alloy",
                ("Ferrostone", "Ferrostone"),
                (
                    "Andesite or tuff with iron nuggets makes Ferrostone Alloy, what every machine is built from.",
                    "Andesit atau tuff dengan butiran besi menjadi Paduan Ferrostone, bahan semua mesin.",
                ),
                1.5,
                0.0,
                tasks=[stack("kinetica:ferrostone_alloy", 4)],
                rewards=[stack("kinetica:shaft", 6), stack("minecraft:iron_nugget", 9)],
                after=["welcome"],
            ),
            quest(
                "shafts_and_cogs",
                "kinetica:shaft",
                ("Shafts and Cogs", "Poros dan Roda Gigi"),
                (
                    "Shafts carry turning along a line; cogwheels pass it sideways, and reverse it.",
                    "Poros meneruskan putaran lurus; roda gigi meneruskannya ke samping dan membaliknya.",
                ),
                3.0,
                0.0,
                tasks=[stack("kinetica:shaft", 8), stack("kinetica:cogwheel", 2)],
                rewards=[stack("kinetica:large_cogwheel", 2)],
                after=["first_alloy"],
            ),
            quest(
                "goggles",
                "kinetica:engineers_goggles",
                ("Seeing Speed", "Melihat Kecepatan"),
                (
                    "Engineer's Goggles show how fast a part turns when you look at it.",
                    "Kacamata Insinyur menampilkan kecepatan bagian yang kamu lihat.",
                ),
                3.0,
                -1.5,
                shape="diamond",
                tasks=[stack("kinetica:engineers_goggles")],
                rewards=[stack("kinetica:ferrostone_alloy", 8)],
                after=["shafts_and_cogs"],
            ),
            quest(
                "hand_power",
                "kinetica:hand_crank",
                ("By Hand", "Tenaga Tangan"),
                (
                    "A hand crank turns what it sits on while you use it. Sneak to turn it the other way.",
                    "Engkol tangan memutar poros di bawahnya selama dipakai. Jongkok untuk arah sebaliknya.",
                ),
                4.5,
                0.0,
                tasks=[stack("kinetica:hand_crank")],
                rewards=[stack("kinetica:cogwheel", 4)],
                after=["shafts_and_cogs"],
            ),
            quest(
                "in_motion",
                "kinetica:cogwheel",
                ("In Motion", "Bergerak"),
                (
                    "Build a machine and set it turning: any part near you at 16 RPM or more will do.",
                    "Bangun mesin dan buat ia berputar: bagian mana pun di dekatmu, 16 RPM atau lebih.",
                ),
                6.0,
                0.0,
                shape="hexagon",
                turning=16,
                rewards=[stack("kinetica:ferrostone_casing", 4)],
                after=["hand_power"],
            ),
            quest(
                "millstone",
                "kinetica:millstone",
                ("Daily Bread", "Roti Harian"),
                (
                    "A millstone grinds what is put in it while it turns; use it with an empty hand to take out what "
                    "it ground. Grind wheat into flour, and bake the flour into bread. Mind the stress: a hand crank "
                    "drives only two.",
                    "Batu gilingan menggiling isinya selama berputar; pakai dengan tangan kosong untuk mengambil "
                    "hasilnya. Giling gandum jadi tepung, lalu panggang tepungnya jadi roti. Perhatikan beban: satu "
                    "engkol tangan hanya kuat memutar dua.",
                ),
                7.5,
                -1.0,
                tasks=[stack("kinetica:wheat_flour", 4)],
                rewards=[stack("minecraft:wheat", 16), stack("kinetica:large_cogwheel", 2)],
                after=["in_motion"],
            ),
            quest(
                "crushed_ore",
                "kinetica:crushed_iron",
                ("Crushed Ore", "Bijih Hancur"),
                (
                    "A millstone grinds raw ore into crushed ore, and now and then gives one more. Smelted, each "
                    "crushed ore gives an ingot.",
                    "Batu gilingan menggiling bijih mentah jadi bijih hancur, sesekali lebih satu. Dilebur, tiap "
                    "bijih hancur menjadi satu batangan.",
                ),
                9.0,
                -1.0,
                tasks=[stack("kinetica:crushed_iron", 4)],
                rewards=[stack("minecraft:hopper", 2)],
                after=["millstone"],
            ),
            quest(
                "pressing",
                "kinetica:mechanical_press",
                ("Pressed Metal", "Logam Tempa"),
                (
                    "A mechanical press turning over a depot presses what is put on the depot: ingots into sheets. "
                    "A hopper beside the depot fills it, and one under it takes the sheets out.",
                    "Mesin press yang berputar di atas depot menempa isi depot: batangan jadi lembaran. Hopper di "
                    "samping depot mengisinya, dan hopper di bawahnya mengambil lembarannya.",
                ),
                7.5,
                1.0,
                tasks=[stack("kinetica:iron_sheet", 4)],
                rewards=[stack("kinetica:depot", 2), stack("minecraft:iron_ingot", 8)],
                after=["in_motion"],
            ),
            quest(
                "mixing",
                "kinetica:mechanical_mixer",
                ("Stirred Together", "Diaduk Rata"),
                (
                    "A mechanical mixer turning over a basin, or two blocks over it, mixes what is put in the basin "
                    "once it holds all a recipe asks for: andesite and an iron nugget into ferrostone alloy, copper "
                    "and tin into bronze, and more. A hopper under the basin takes out what was mixed.",
                    "Mesin pengaduk yang berputar di atas baskom (langsung, atau dua blok di atasnya) mengaduk isi "
                    "baskom begitu semua bahan resep lengkap: andesit dan butiran besi jadi paduan ferrostone, "
                    "tembaga dan timah jadi perunggu, dan lainnya. Hopper di bawah baskom mengambil hasilnya.",
                ),
                9.0,
                0.5,
                tasks=[stack("kinetica:mechanical_mixer"), stack("kinetica:basin")],
                rewards=[stack("minecraft:andesite", 16), stack("minecraft:iron_nugget", 16)],
                after=["pressing"],
            ),
            quest(
                "bulk_washing",
                "kinetica:encased_fan",
                ("Fresh Air", "Angin Segar"),
                (
                    "An encased fan blows out of the end no shaft drives it from, further the faster it turns; "
                    "turned the other way, it pulls. Its air pushes dropped items and works what stays in it: through "
                    "water it washes them (crushed ore into nuggets, concrete powder into concrete), through fire it "
                    "smokes them, through lava it smelts them. Items on a depot in the air are worked too.",
                    "Kipas terselubung meniup dari ujung yang tidak tersambung poros, makin jauh bila makin cepat; "
                    "putarannya dibalik, ia menyedot. Anginnya mendorong barang yang tergeletak dan mengolah yang "
                    "tertahan di dalamnya: lewat air dicuci (bijih hancur jadi butiran, bubuk beton jadi beton), lewat "
                    "api diasapi, lewat lava dilebur. Barang di atas depot dalam aliran angin ikut diolah.",
                ),
                9.0,
                1.5,
                tasks=[stack("kinetica:encased_fan")],
                rewards=[stack("kinetica:crushed_iron", 8), stack("minecraft:water_bucket")],
                after=["pressing"],
            ),
            quest(
                "conveyor",
                "kinetica:belt",
                ("On the Move", "Ban Berjalan"),
                (
                    "Belts set in a line turn together, from a shaft beside any of them, and carry what lies on them; "
                    "use one with a stack in hand to put it on. A funnel on a chest, a furnace or a depot puts in what "
                    "comes into it; use it and it takes out instead, a stack at a time, onto the belt before it.",
                    "Sabuk yang dipasang berjajar berputar bersama dari poros di samping salah satunya, dan membawa "
                    "barang di atasnya; klik sabuk sambil memegang tumpukan untuk menaruhnya. Corong pada peti, "
                    "tungku, atau depot memasukkan barang yang masuk ke dalamnya; klik corong agar ia mengeluarkan "
                    "isi wadah, setumpuk demi setumpuk, ke sabuk di depannya.",
                ),
                10.5,
                1.5,
                tasks=[stack("kinetica:belt", 4), stack("kinetica:funnel")],
                rewards=[stack("minecraft:chest", 2), stack("minecraft:dried_kelp_block", 4)],
                after=["bulk_washing"],
            ),
            quest(
                "water_power",
                "kinetica:water_wheel",
                ("Running Water", "Air Mengalir"),
                (
                    "A water wheel turns with the water flowing past it, faster with water on more sides.",
                    "Kincir air berputar oleh air yang mengalir melewatinya, lebih cepat bila lebih banyak sisi.",
                ),
                4.5,
                1.5,
                tasks=[stack("kinetica:water_wheel")],
                rewards=[stack("kinetica:gearbox")],
                after=["hand_power"],
            ),
            quest(
                "wind_power",
                "kinetica:windmill",
                ("Open Sky", "Langit Terbuka"),
                (
                    "A windmill needs open sky above it, and turns faster the higher it stands.",
                    "Kincir angin butuh langit terbuka, dan berputar lebih cepat di tempat tinggi.",
                ),
                4.5,
                -1.5,
                tasks=[stack("kinetica:windmill")],
                rewards=[stack("kinetica:clutch"), stack("kinetica:gearshift")],
                after=["hand_power"],
            ),
            quest(
                "control",
                "kinetica:gearshift",
                ("In Control", "Kendali"),
                (
                    "A powered clutch parts a line; a powered gearshift turns it the other way.",
                    "Kopling yang diberi sinyal memutus jalur; pemindah gigi yang diberi sinyal membalik arahnya.",
                ),
                6.0,
                2.5,
                tasks=[
                    stack("kinetica:gearbox"),
                    stack("kinetica:clutch"),
                    stack("kinetica:gearshift"),
                ],
                rewards=[stack("kinetica:ferrostone_casing", 8)],
                after=["water_power", "wind_power"],
            ),
            quest(
                "full_speed",
                "kinetica:large_cogwheel",
                ("Full Speed", "Kecepatan Penuh"),
                (
                    "A large cogwheel driving a small one doubles the speed. Step a wheel's turning up to 128 RPM.",
                    "Roda gigi besar yang memutar roda gigi kecil menggandakan kecepatan. Naikkan putaran kincir "
                    "sampai 128 RPM.",
                ),
                7.5,
                2.5,
                shape="hexagon",
                size=1.25,
                turning=128,
                rewards=[
                    stack("kinetica:bronze_ingot", 8),
                    stack("kinetica:ferrostone_alloy", 16),
                ],
                after=["control"],
            ),
        ],
    },
    {
        "id": "metals",
        "file": "02_metals.json",
        "order": 2,
        "icon": "kinetica:tin_ingot",
        "names": ("Metals", "Logam"),
        "quests": [
            quest(
                "tin",
                "kinetica:raw_tin",
                ("Tin", "Timah"),
                (
                    "Tin lies in the ground where copper does. Smelt the raw tin into ingots.",
                    "Timah ada di dalam tanah di tempat yang sama dengan tembaga. Lebur timah mentah jadi batangan.",
                ),
                0.0,
                0.0,
                tasks=[stack("kinetica:tin_ingot", 8)],
                rewards=[stack("minecraft:copper_ingot", 12)],
                after=["machine_age/welcome"],
            ),
            quest(
                "bronze",
                "kinetica:bronze_ingot",
                ("The Bronze Age", "Zaman Perunggu"),
                (
                    "Three copper and one tin make four bronze.",
                    "Tiga tembaga dan satu timah menjadi empat perunggu.",
                ),
                2.0,
                0.0,
                shape="hexagon",
                tasks=[stack("kinetica:bronze_ingot", 8)],
                rewards=[stack("kinetica:engineers_goggles")],
                after=["tin"],
            ),
        ],
    },
    {
        "id": "lost_realms",
        "file": "03_lost_realms.json",
        "order": 3,
        "icon": "minecraft:compass",
        "names": ("Lost Realms", "Dunia yang Hilang"),
        "quests": [
            quest(
                "lost_workshop",
                "kinetica:cogwheel",
                ("The Lost Workshop", "Bengkel yang Hilang"),
                (
                    "Tinkerers once worked in the forests and plains. Find one of their ruined workshops.",
                    "Dulu para perajin bekerja di hutan dan dataran. Temukan salah satu bengkel mereka yang runtuh.",
                ),
                0.0,
                -1.0,
                find="lost_realms:workshop_ruin",
                rewards=[stack("minecraft:spyglass"), stack("kinetica:ferrostone_alloy", 8)],
                after=["machine_age/welcome"],
            ),
            quest(
                "watchtower",
                "minecraft:spyglass",
                ("The Old Watch", "Penjaga Lama"),
                (
                    "The old realm watched its borders from stone towers. Climb into one of them.",
                    "Kerajaan lama mengawasi perbatasannya dari menara batu. Masuklah ke salah satunya.",
                ),
                0.0,
                1.0,
                find="lost_realms:watchtower",
                rewards=[stack("minecraft:arrow", 16), stack("kinetica:tin_ingot", 6)],
                after=["machine_age/welcome"],
            ),
        ],
    },
]


def _short_id(resource):
    return resource.split(":", 1)[-1].replace("/", "_")


def main():
    if ROOT.exists():
        shutil.rmtree(ROOT)
    ROOT.mkdir(parents=True)
    (ROOT / "horizon-mod.toml").write_text(
        "# Tachyon: Aether & Gears. Chronicles, the quests: written by tachyon/art/chronicles.py.\n"
        'id = "chronicles"\nname = "Chronicles"\nversion = "0.7.0"\napi = "0.2"\nside = "both"\n'
        'capabilities = ["registries"]\n',
        encoding="utf-8",
    )

    en = {
        "chronicles.title": "Chronicles",
        "chronicles.claim": "Claim",
        "chronicles.claimed": "Claimed",
        "chronicles.locked": "Finish the quests before it first",
        "chronicles.tasks": "Hold at once:",
        "chronicles.rewards": "Rewards:",
        "chronicles.done": "Quest complete: %s",
        "chronicles.turning": "A machine near you turning at %s RPM",
        "chronicles.find": "Find: %s",
        "chronicles.task.in_motion.rpm": "Machine turning at 16 RPM",
        "chronicles.task.full_speed.rpm": "Machine turning at 128 RPM",
        "chronicles.task.lost_workshop.find": "Find: Workshop Ruin",
        "chronicles.task.watchtower.find": "Find: Watchtower",
    }
    idn = {
        "chronicles.title": "Kronik",
        "chronicles.claim": "Klaim",
        "chronicles.claimed": "Sudah diklaim",
        "chronicles.locked": "Selesaikan quest sebelumnya dulu",
        "chronicles.tasks": "Bawa sekaligus:",
        "chronicles.rewards": "Hadiah:",
        "chronicles.done": "Quest selesai: %s",
        "chronicles.turning": "Mesin di dekatmu berputar %s RPM",
        "chronicles.find": "Temukan: %s",
        "chronicles.task.in_motion.rpm": "Mesin berputar 16 RPM",
        "chronicles.task.full_speed.rpm": "Mesin berputar 128 RPM",
        "chronicles.task.lost_workshop.find": "Temukan: Reruntuhan Bengkel",
        "chronicles.task.watchtower.find": "Temukan: Menara Pengawas",
    }

    pack_dir = ROOT / "data" / NS / "horizon_quests"
    chapters_dir = pack_dir / "chapters"
    chapters_dir.mkdir(parents=True, exist_ok=True)

    settings = {
        "version": 1,
        "progression": "flexible",
        "default_shape": "square",
        "default_size": 1.0,
        "auto_claim_rewards": False,
        "detection_delay": 20,
        "groups": [{"id": "chronicles", "title": "chronicles.title"}],
    }
    (pack_dir / "quests.json").write_text(
        json.dumps(settings, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )

    all_full_ids = set()
    for chapter in CHAPTERS:
        for q in chapter["quests"]:
            full_id = f"{chapter['id']}/{q['id']}"
            assert full_id not in all_full_ids, f"duplicate quest {full_id}"
            all_full_ids.add(full_id)

    total = 0
    for chapter in CHAPTERS:
        ch_key = f"chronicles.chapter.{chapter['id']}"
        en[ch_key], idn[ch_key] = chapter["names"]
        hq_quests = []
        for q in chapter["quests"]:
            total += 1
            q_key = f"chronicles.quest.{q['id']}"
            en[q_key], idn[q_key] = q["names"]
            en[f"{q_key}.text"], idn[f"{q_key}.text"] = q["texts"]

            hq_tasks = []
            if "checkmark" in q:
                ck_key = f"chronicles.task.{q['id']}.ready"
                en[ck_key], idn[ck_key] = q["checkmark"]
                hq_tasks.append({"id": "ready", "type": "checkmark", "title": ck_key})
            for idx, t in enumerate(q["tasks"], start=1):
                hq_tasks.append(
                    {
                        "id": f"item_{idx}_{_short_id(t['item'])}",
                        "type": "item",
                        "item": t["item"],
                        "count": t["count"],
                    }
                )
            if "turning" in q:
                hq_tasks.append(
                    {
                        "id": "rpm",
                        "type": "kinetic_speed",
                        "rpm": q["turning"],
                        "title": f"chronicles.task.{q['id']}.rpm",
                    }
                )
            if "find" in q:
                hq_tasks.append(
                    {
                        "id": "structure",
                        "type": "structure",
                        "structure": q["find"],
                        "title": f"chronicles.task.{q['id']}.find",
                    }
                )

            hq_rewards = []
            for idx, r in enumerate(q["rewards"], start=1):
                hq_rewards.append(
                    {
                        "id": f"reward_{idx}_{_short_id(r['item'])}",
                        "type": "item",
                        "item": r["item"],
                        "count": r["count"],
                    }
                )

            for dep in q["after"]:
                resolved = dep if "/" in dep else f"{chapter['id']}/{dep}"
                assert resolved in all_full_ids, f"{chapter['id']}/{q['id']}: bad dep {dep}"

            entry = {
                "id": q["id"],
                "title": q_key,
                "description": [f"{q_key}.text"],
                "icon": q["icon"],
                "x": q["x"],
                "y": q["y"],
                "dependencies": q["after"],
                "tasks": hq_tasks,
                "rewards": hq_rewards,
            }
            if "shape" in q:
                entry["shape"] = q["shape"]
            if "size" in q:
                entry["size"] = q["size"]
            hq_quests.append(entry)

        ch_doc = {
            "id": chapter["id"],
            "title": ch_key,
            "icon": chapter["icon"],
            "group": "chronicles",
            "order": chapter["order"],
            "quests": hq_quests,
        }
        (chapters_dir / chapter["file"]).write_text(
            json.dumps(ch_doc, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
        )

    for path, value in [
        (ROOT / "assets" / NS / "lang" / "en_us.json", en),
        (ROOT / "assets" / NS / "lang" / "id_id.json", idn),
    ]:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

    print(f"chronicles: {total} quests in {pack_dir}")


if __name__ == "__main__":
    main()
