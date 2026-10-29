# Tachyon Content Engine (TCE)

Lapisan yang memberi blok modpack perilaku di luar template vanilla-nya. Rencana besar ada di
[rencana-modpack.md](rencana-modpack.md); dokumen ini menjelaskan cara kerjanya sekarang.

## Konten bawaan

- Isi mod (blok, item, model, tekstur, resep, loot table, tag, nama) ada di `tachyon/content/<id>/`.
  Formatnya content mod Horizon API 0.2 (`horizon-mod.toml`, `assets/`, `data/`).
- Isi itu **ditulis oleh skrip**, misalnya `python tachyon/art/kinetica.py`. Jangan sunting hasilnya
  langsung; ubah skripnya lalu jalankan lagi.
- Windows: CI menyalinnya ke `resources/tachyon-mods/`. Android: CI mengemasnya sebagai
  `content.zip` dengan id sendiri, dibongkar terpisah dari resource Minecraft.
- Game tahu foldernya lewat `horizon_client::brand::set_content`. `content_root` memasang mod-mod
  itu lebih dulu dan selalu aktif, dan halaman Mods tidak menampilkannya.

## Blok yang berputar (Kinetica)

- Logika murni ada di `game/tachyon_engine/kinetic.rs`. `Part` adalah poros, roda gigi kecil/besar,
  gearbox, motor, engkol tangan, kopling, pemindah gigi, kincir air, atau kincir angin. `link` memuat aturan sambungan, dan `solve` menghitung kecepatan tiap jaringan
  dari motornya. Aturannya mengikuti Create (MIT), tetapi kodenya milik kita.
- **Pemilik dunia** menjalankan `Game::tachyon_tick_blocks` pada fase block entity, sebelum sculk.
  Pemilik dunia adalah thread server, host LAN, atau jendela game saat mode in-frame.
  - Pemilik menyimpan indeks bagian kinetik per dimensi. Chunk dipindai saat dimuat (palet section
    dicek dulu), lalu indeks diperbarui dari `world.watched_writes`. TCE menandai state-nya sebagai
    "watched".
  - Saat indeks berubah, pemilik menghitung ulang jaringan dan mengirim payload `tachyon:kinetic` ke
    pemain. Pengiriman penuh diulang tiap 100 tick untuk pemain yang baru masuk.
- **Klien** menyimpan indeks chunk-nya sendiri (`Game::tachyon_tick` dari hook `native_tick`) dan
  menerima kecepatan lewat `Game::tachyon_payload` di `remote.rs`.
  - `Game::tachyon_render` menggambar bagian yang berputar di pass entity. Model di-bake seperti
    item, lalu diputar `kinetic::angle`.
  - Model blok bawaan untuk bagian yang berputar sengaja kosong (`*_still`), hanya berisi tekstur
    partikel.
- **Sumber putaran:**
  - Motor kreatif: klik kanan menggandakan kecepatannya (sampai 256, lalu kembali ke 8); sambil
    jongkok, arahnya berbalik. Kecepatannya disimpan di `<dunia>/tachyon/kinetica-<dimensi>.json`.
  - Engkol tangan: setiap klik kanan memutarnya 10 tick (32 RPM, jongkok untuk arah sebaliknya).
  - Kincir air: setiap detik, aliran air di keempat sisinya dihitung, lalu diambil putarannya
    terhadap sumbu kincir, 8 RPM per sisi dan paling cepat 32 RPM.
  - Kincir angin: berputar bila langit di atasnya terbuka. Kecepatannya 8 RPM di ketinggian 64 dan
    naik 4 RPM tiap 16 blok, paling cepat 24 RPM.
- **Redstone:** kopling yang mendapat sinyal memutus putaran, dan pemindah gigi yang mendapat
  sinyal membalik arahnya.
- **HUD:** dengan Kacamata Insinyur dipakai di kepala atau dipegang, bagian kinetik yang dilihat
  menampilkan kecepatannya di pojok kiri atas, dalam bahasa pemain.
- **State** disimpan per thread (`thread_local`). Jendela game dan thread server masing-masing
  berjalan di thread sendiri.

## Quest (Chronicles)

- **Data:** daftar quest ada di `chronicles/data/chronicles/tachyon/quests.json`, ditulis oleh
  `tachyon/art/chronicles.py`. Isinya bab, quest, tugas, hadiah, dan quest yang harus selesai dulu.
  Nama dan teksnya ada di file bahasa mod itu.
- **Tugas** (boleh digabung dalam satu quest):
  - `tasks`: item yang harus dibawa sekaligus (inventaris, armor, tangan kiri);
  - `turning`: bagian Kinetica yang berputar minimal sekian RPM dalam jarak 16 blok dari pemain;
  - `find`: berdiri di dalam struktur itu (kotak struktur, termasuk tepi 12 blok untuk struktur yang
    meratakan tanah). Namanya diambil dari `structure.<ns>.<id>` di file bahasa.
- **Pemilik dunia** (`game/tachyon_engine/chronicles.rs`):
  - Menyimpan progres tiap pemain di `<dunia>/tachyon/chronicles/<uuid>.json`.
  - Setiap detik memeriksa item, mesin di sekitar, dan struktur tempat para pemain berada, lalu
    menandai quest yang selesai.
  - Memberi hadiah saat diklaim, lalu mengirim progres lewat payload `tachyon:quests`.
- **Jendela pemain:**
  - Tombol **J** (bisa diganti di menu Mods) membuka buku quest dan meminta progres
    (`tachyon:quests_hello`).
  - Tombol Klaim mengirim `tachyon:quest_claim`.
  - Quest yang baru selesai diberitahukan di chat.
- **Layar:** build Tachyon tidak memakai mod WebAssembly, jadi `Screen::Mod` dipakai untuk buku quest.
  Fungsi kosongnya di `game/mods.rs` diarahkan ke `tachyon_screen_*`.
- **Belum ada:**
  - tugas membuat item dan mengalahkan mob;
  - quest untuk pemain lokal dalam mode in-frame.

## Bijih di dunia

- Content mod boleh menambah `configured_feature` dan `placed_feature`, tetapi tidak boleh mengubah
  bioma. Karena itu Tachyon menambah kunci `"horizon:beside": "<fitur vanilla>"` pada placed feature
  (`mod_content.rs`).
- Fitur yang memakai kunci itu ditaruh tepat setelah fitur vanilla yang disebut, pada langkah yang
  sama, di setiap bioma yang memuat fitur vanilla tersebut. Cara ini mirip *biome modifier* di
  NeoForge.
- Contoh: bijih timah Kinetica (`kinetica:ore_tin`) ditaruh di samping `minecraft:ore_copper`.

## Titik kait di kode Horizon

Satu baris per titik, supaya merge dari upstream tetap mudah:

| File | Kait |
|---|---|
| `game.rs` | `mod tachyon_engine; mod tachyon_mods;`, `tachyon_tick_blocks` sebelum `tick_sculk_blocks`, `tachyon_render` setelah block entity |
| `game/native_hooks.rs` | Isi setiap hook memanggil `tachyon_*` (mod resmi dan TCE) |
| `game/remote.rs` | Payload `tachyon:*` menuju `tachyon_payload` |
| `game/mods.rs` | `content_root` memasang konten bawaan |
| `mod_content.rs` | `horizon:beside`: placed feature dari mod ikut masuk ke bioma |
| `game/lan_actions.rs` | Payload `tachyon:*` dari pemain menuju `tachyon_guest_payload` |
| `game/mods.rs` | Tanpa fitur `mods`, layar mod adalah buku quest (`tachyon_screen_*`) |

## Belum ada

- Kacamata yang terlihat di kepala (sekarang tampilannya masih topi kulit dari template).
- Stress (kapasitas dan beban).
- Mesin yang memproses item.
- Instancing GPU untuk jaringan yang sangat besar.
