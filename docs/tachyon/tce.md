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
  gearbox, atau motor. `link` memuat aturan sambungan, dan `solve` menghitung kecepatan tiap jaringan
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
- **HUD:** saat pemain melihat bagian kinetik, kecepatannya tampil di pojok kiri atas, seperti
  kacamata insinyur di Create.
- **State** disimpan per thread (`thread_local`). Jendela game dan thread server masing-masing
  berjalan di thread sendiri.

## Titik kait di kode Horizon

Satu baris per titik, supaya merge dari upstream tetap mudah:

| File | Kait |
|---|---|
| `game.rs` | `mod tachyon_engine; mod tachyon_mods;`, `tachyon_tick_blocks` sebelum `tick_sculk_blocks`, `tachyon_render` setelah block entity |
| `game/native_hooks.rs` | Isi setiap hook memanggil `tachyon_*` (mod resmi dan TCE) |
| `game/remote.rs` | Payload `tachyon:*` menuju `tachyon_payload` |
| `game/mods.rs` | `content_root` memasang konten bawaan |

## Belum ada

- Mengatur kecepatan motor (klik kanan) dan menyimpan data per blok.
- Stress (kapasitas dan beban).
- Mesin yang memproses item.
- Instancing GPU untuk jaringan yang sangat besar.
