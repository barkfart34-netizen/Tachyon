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
  gearbox, motor, engkol tangan, kopling, pemindah gigi, kincir air, kincir angin, atau batu gilingan. `link` memuat aturan sambungan, dan `solve` menghitung kecepatan tiap jaringan
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
- **Stress (beban):**
  - Mesin membebani jaringannya sebesar `Part::impact` per RPM. Batu gilingan bebannya 4 SU per
    RPM, mesin press 8, dan mesin pengaduk 4; poros, roda gigi, gearbox, kopling, dan pemindah gigi
    tidak membebani.
  - Kapasitas sumber (`kinetic::capacity`): engkol tangan 8 SU per RPM (256 SU pada 32 RPM), kincir
    air 16, kincir angin 32, dan motor kreatif selalu 16384 SU.
  - Jaringan yang bebannya melebihi kapasitas *kelebihan beban*: seluruh jaringan berhenti sampai
    bebannya dikurangi atau sumbernya ditambah. Menaikkan kecepatan lewat roda gigi juga menaikkan
    beban mesin, sedangkan kapasitas sumber tetap.
- **Batu gilingan** (`game/tachyon_engine/millstone.rs`):
  - Dipakai sambil memegang barang yang bisa digiling: tumpukan itu masuk (paling banyak 64).
    Dipakai dengan barang lain atau tangan kosong: hasil gilingan keluar ke inventaris.
  - Selama berputar, satu barang selesai setiap `time` kerja; kerja bertambah 1/16 per RPM tiap
    tick (gandum 150 kerja: hampir 5 detik pada 32 RPM). Hasil disimpan sampai 9 tumpukan; kalau
    penuh, batu berhenti menggiling.
  - Resepnya data: `kinetica/data/kinetica/tachyon/milling.json` (ditulis `tachyon/art/kinetica.py`),
    dengan peluang untuk hasil tambahan. Penampil resep (mod Recipes) menampilkannya di tab batu
    gilingan; hasil tambahan dan peluangnya ada di catatan resep.
  - Isinya disimpan bersama kecepatan motor di `kinetica-<dimensi>.json`, dan jatuh ke tanah saat
    batunya dihancurkan. Bentuknya meminjam template pemotong batu (setinggi 9 piksel); klik ke
    blok ini selalu ditangani TCE lebih dulu, jadi layar pemotong batu tidak pernah terbuka.
  - Selain gandum dan lainnya, batu gilingan menggiling bijih mentah (besi, tembaga, emas, timah)
    jadi bijih hancur: satu, ditambah satu lagi dengan peluang 25%. Bijih hancur dilebur (tungku
    atau blast furnace) jadi satu batangan.
- **Depot dan mesin press** (`millstone.rs` juga):
  - Depot menampung satu tumpukan (paling banyak 64). Dipakai saat kosong: tumpukan yang dipegang
    masuk. Dipakai saat berisi: seluruh isinya keluar ke inventaris. Isinya tergambar di atas
    nampannya dan terlihat lewat kacamata ("Depot: Iron Ingot x5").
  - Mesin press (template pilar kuarsa, berporos x/y/z) menerima putaran lewat poros di sepanjang
    porosnya, kecuali dari bawah karena kepalanya turun ke sana. Press yang berputar mengerjakan
    depot tepat di bawahnya, atau satu blok lebih rendah dengan ruang kosong di antaranya (cara
    Create; `worker` di `tachyon_engine.rs`). Resepnya ada di `pressing.json`: batangan besi,
    tembaga, emas, perunggu, dan timah jadi lembaran; tebu jadi kertas. Kerjanya 100 per barang,
    sama seperti batu gilingan (5 detik pada 16 RPM). Kepalanya naik turun selama bekerja, sekali
    per 100 kerja, dengan batang yang memanjang sampai ke dalam press.
  - Penampil resep punya tab mesin press.
- **Baskom dan mesin pengaduk** (`game/tachyon_engine/basin.rs`):
  - Baskom (template komposter: lantai dan dinding setebal 2 piksel) menampung sampai 9 tumpukan
    barang apa saja. Dipakai sambil memegang barang: tumpukannya masuk. Dipakai dengan tangan
    kosong: hasil adukan keluar, atau isinya bila belum ada hasil. Barang yang punya komponen (nama,
    enchantment) tidak diterima. Ketentuan yang sama berlaku untuk depot dan hopper.
  - Mesin pengaduk (blok penuh) menerima putaran dari poros di atasnya. Yang berputar mengaduk
    baskom tepat di bawahnya atau satu blok lebih rendah, begitu baskom memuat semua bahan sebuah
    resep `mixing.json` (resep pertama yang lengkap). Ada 25 resep:
    - andesit atau tuff + butiran besi jadi paduan ferrostone (dua kali hasil resep crafting);
    - 3 tembaga + 1 timah jadi 4 perunggu (dari batangan atau bijih hancur);
    - tanah kasar, lumpur padat, sup jamur, mata laba-laba fermentasi, dan magma cream;
    - 16 warna bubuk beton.
  - Kocokannya berputar di dalam baskom selama bekerja, pada tiang yang memanjang ke atas.
    Isi baskom (sampai 4 tumpukan) tergambar di dasarnya. Kacamata menampilkan isinya.
  - Penampil resep punya tab mesin pengaduk; bahan tampil dalam grid beserta jumlahnya.
- **Inventaris kreatif:** semua barang mod konten (urutan registri) ada di tab sendiri. Mod bawaan
  Tachyon ada di tab "Tachyon" pada tempat hotbar tersimpan vanilla (baris atas). Mod lain, misalnya
  hasil impor, ada di tab "Mods" pada tempat blok operator (baris bawah). Pencarian juga
  menemukannya (`creative.rs`, `add_mod_tabs`).
- **Hopper vanilla** (kait di `game/machines.rs`):
  - Hopper yang menghadap batu gilingan, depot, atau baskom memasukkan satu barang tiap kali. Batu
    gilingan hanya menerima barang yang bisa digiling. Depot hanya menerima barang selama belum ada
    hasil press di dalamnya. Baskom menerima apa saja selama masih ada tempat.
  - Hopper di bawah batu gilingan atau depot mengambil hasilnya satu per satu. Dari depot tanpa
    press di atasnya, atau berisi barang yang tidak bisa dipress, hopper mengambil barangnya juga,
    jadi depot bisa dipakai sebagai jalur lewat. Dari baskom, hopper hanya mengambil hasil adukan.
  - Barang yang diambil hopper tetapi tidak muat dikembalikan ke tempat asalnya.
  - Komponen barang (nama, enchantment) tidak ikut tersimpan di mesin; tidak masalah untuk bahan
    mentah.
- **HUD:** dengan Kacamata Insinyur dipakai di kepala atau dipegang, bagian kinetik yang dilihat
  menampilkan kecepatannya di pojok kiri atas, dalam bahasa pemain. Ditambah beban/kapasitas
  jaringannya ("128 / 256 SU"), status kelebihan beban, dan barang yang sedang digiling atau dipress.
- **Payload** `tachyon:kinetic` versi 5 berisi kecepatan, bagian yang macet, stress tiap bagian,
  mesin yang sedang bekerja (dan barangnya), serta isi tiap depot dan baskom (sampai 4 tumpukan).
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
| `game/remote.rs` | Payload `tachyon:*` menuju `tachyon_payload`; `remote_block_reacts` menanyakan `tachyon_takes_use` (klik ke motor, engkol, batu gilingan, atau depot tidak memprediksi penempatan blok) |
| `game/machines.rs` | `hopper_eject` dan `hopper_suck` lebih dulu menanyakan `tachyon_hopper_in` / `tachyon_hopper_out` (mesin TCE) |
| `game/mods.rs` | `content_root` memasang konten bawaan |
| `mod_content.rs` | `horizon:beside`: placed feature dari mod ikut masuk ke bioma |
| `game/lan_actions.rs` | Payload `tachyon:*` dari pemain menuju `tachyon_guest_payload` |
| `game/mods.rs` | Tanpa fitur `mods`, layar mod adalah buku quest (`tachyon_screen_*`) |

## Belum ada

- Kacamata yang terlihat di kepala (sekarang tampilannya masih topi kulit dari template).
- Mesin pemroses lain (kipas, gergaji), panas (blaze burner), fluida, dan logistik (belt, funnel):
  sisa Kinetica II.
- Instancing GPU untuk jaringan yang sangat besar.
