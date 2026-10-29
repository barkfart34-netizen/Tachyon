# Rencana modpack Tachyon: fantasi + era mesin

Status: **Tahap 0 selesai, Tahap 1–2 berjalan** (kemajuan di bagian 10). Ditulis 29 Oktober 2026.

Nama modpack: **Tachyon: Aether & Gears**.

## 1. Tujuan

Modpack milik Tachyon sendiri, jalan di **PC (Windows) dan Android**, bisa dimainkan bersama
lewat LAN/server, berisi:

| Bagian | Mirip dengan | Nama kerja |
|---|---|---|
| Mesin kinetik: poros, gear, proses, logistik, contraption | Create | **Kinetica** |
| Kapal udara: balon, baling-baling, kemudi, dek yang bisa dinaiki | Create: Aeronautics | **Aerodyne** |
| Sihir: kristal aether, mana, mantra, altar, mesin bertenaga aether | mod magic (Ars, Botania) | **Arcana** |
| Quest: buku quest per era, tugas, hadiah | FTB Quests | **Chronicles** |
| Struktur, dungeon, pulau langit, loot | YUNG's, When Dungeons Arise | **Lost Realms** |
| Minimap, penampil resep, zoom | Xaero, JEI | mod resmi Horizon, ikut dibundel |

Nama-nama di atas sementara. Nama modpack ditentukan nanti (usulan di bagian 9).

**Benang merah cerita:** *Aether*, energi kristal dari peradaban langit yang runtuh. Pemain mulai
di dunia fantasi abad pertengahan, menemukan mesin jam kuno, membangun mesin uap-aether, lalu
terbang dengan kapal udara ke pulau-pulau langit tempat dungeon dan bos terakhir.

| Era | Isi utama | Mod |
|---|---|---|
| I. Abad Pertengahan | Desa, reruntuhan, sihir dasar, quest awal | Arcana, Lost Realms, Chronicles |
| II. Jam Mekanik | Hand crank, kincir air, poros, gear, millstone | Kinetica |
| III. Uap & Aether | Mesin aether, press, mixer, belt, otomasi, contraption | Kinetica + Arcana |
| IV. Zaman Langit | Kapal udara, pulau langit, dungeon langit, bos | Aerodyne, Lost Realms |

## 2. Batas yang harus dipahami dulu

- **Mod Java tidak bisa dijalankan.** Tachyon (Horizon) adalah Minecraft yang ditulis ulang dalam
  Rust. Create, Aeronautics, dan mod lain harus *dibuat ulang* dalam Rust, bukan disalin.
- **Lisensi Create:** kodenya **MIT**, boleh dipelajari dan diadaptasi asal mencantumkan kredit.
  **Asetnya (tekstur, model, suara) "All Rights Reserved"**, jadi semua gambar dan model harus
  buatan kita sendiri. Nama "Create" juga tidak dipakai.
- **Create: Aeronautics:** lisensinya belum saya periksa. Kita tidak menyalin kode atau asetnya,
  hanya meniru *ide gameplay* (ide tidak dilindungi hak cipta).
- **Sistem mod Horizon belum cukup.** API mod WebAssembly-nya (versi 0.2) baru bisa menambah data:
  blok, item, resep, worldgen. Block entity, render animasi, entity, dan menu baru ada di rencana
  Horizon (API 0.3 sampai 0.5) tanpa jadwal. Karena itu Kinetica, Aerodyne, dan lainnya ditulis
  **native di dalam Tachyon** (lihat bagian 3).
- **Tiruan, bukan salinan 1:1.** Mekaniknya akan terasa mirip Create, tapi angka, resep, dan
  detailnya milik kita.

## 3. Arsitektur

```
 ┌──────────────────────────────────────────────────────────────┐
 │ Modpack (resep, quest, keseimbangan, progresi antar era)     │
 ├────────────┬────────────┬──────────┬────────────┬────────────┤
 │ Kinetica   │ Aerodyne   │ Arcana   │ Chronicles │ Lost Realms│  ← crate Rust + aset kita
 ├────────────┴────────────┴──────────┴────────────┴────────────┤
 │ Tachyon Content Engine (TCE): registry blok/item, perilaku,  │  ← lapisan baru
 │ block entity, render animasi, menu, tipe resep, entity,      │
 │ sinkronisasi multiplayer, simpan/muat                        │
 ├──────────────────────────────────────────────────────────────┤
 │ Horizon (mesin game, di-merge rutin dari upstream)           │
 └──────────────────────────────────────────────────────────────┘
```

- **TCE** adalah crate baru (`crates/tachyon-engine`) dengan titik kait (hook) sesedikit mungkin
  di kode Horizon: dispatch perilaku blok (use, place, break, neighbour, tick), siklus block entity
  (tick, NBT, sync ke klien), render block entity beranimasi, menu/GUI, tipe resep, dan entity.
  Hook ditaruh di file terpisah supaya merge dari upstream (sekitar 700 commit per hari) tetap
  mudah.
- **Blok baru** memakai jalur "content mod" Horizon yang sudah ada untuk mendapat ID state, model,
  dan tabel. TCE menambahkan perilakunya.
- **Setiap mod adalah crate Rust** plus folder aset/data (JSON, model, tekstur, NBT struktur) yang
  dibundel ke resource game saat build. Semua pemain Tachyon punya isi yang sama, jadi tidak ada
  masalah versi mod saat main bersama.
- **Performa HP:** bagian yang berputar digambar dengan instancing (satu draw untuk banyak poros)
  dan jaringan kinetik dihitung per jaringan, bukan per blok per tick (cara Create dan Sodium).

## 4. Tahapan

Ukuran: **S** = 1–2 sesi, **M** = beberapa sesi, **L** = banyak sesi, **XL** = proyek besar
tersendiri. Setiap tahap punya kriteria "selesai" yang bisa diuji.

### Tahap 0: Persiapan (S)
- Memutuskan nama, gaya seni, dan urutan (bagian 9).
- Ruang disk laptop (bagian 6).
- Alur tes di laptop: Tachyon hasil CI + `horizon-ctl` (mode `--offscreen`, screenshot otomatis).
- CI: job Linux murah untuk `cargo check` + unit test di setiap push. Build Windows/APK hanya
  untuk playtest.
- Membundel mod resmi Horizon: **Minimap**, **Recipes** (penampil resep ala JEI), **Zoom**,
  **FPS**, **Keystrokes**.

**Selesai bila:** build Tachyon punya minimap dan penampil resep, dan saya bisa mengambil
screenshot game dari laptop secara otomatis.

### Tahap 1: Tachyon Content Engine (L–XL)
- Registry blok/item milik kita, callback perilaku, block entity (tick, simpan/muat NBT, sync).
- Render block entity beranimasi (bagian berputar), item dengan perilaku, menu/GUI sederhana.
- Tipe resep khusus yang tampil di penampil resep.
- Mod uji "Test Bench": satu blok berputar dan satu mesin ber-GUI.

**Selesai bila:** Test Bench jalan di singleplayer, tersimpan setelah keluar-masuk, dan terlihat
sama di PC dan HP saat LAN.

### Tahap 2: Kinetica I, sistem kinetik (L)
- Poros, cogwheel kecil/besar, gearbox, clutch, gearshift.
- Sumber tenaga: hand crank, kincir air, kincir angin sederhana, motor kreatif.
- Kecepatan (RPM), arah, *stress units* (kapasitas vs beban), kacamata insinyur untuk melihat angka.
- Material: logam baru (pengganti zinc/brass dengan nama sendiri), ore dan worldgen-nya, casing.

**Selesai bila:** jaringan 50+ poros/gear berputar mulus di HP, dan overstress menghentikan
jaringan.

### Tahap 3: Kinetica II, proses dan logistik (XL)
- Millstone, crushing wheel, press, mixer + basin, saw, kipas (cuci/bakar/asap), deployer.
- Belt, depot, funnel, chute, mechanical arm.
- (3b) Fluida: pipa, pompa, tangki.

**Selesai bila:** satu lini produksi otomatis (ore → hancur → cuci → lebur → pelat) berjalan tanpa
pemain.

### Tahap 4: Kinetica III, contraption (XL)
- Mechanical piston, bearing, gantry: kumpulan blok yang bergerak sebagai satu entity, plus
  tabrakan dan pemain yang menumpang di atasnya.
- Mesin pada contraption: bor, gergaji, harvester.

**Selesai bila:** kincir angin besar dari blok bebas berputar, dan lift piston mengangkut pemain.

### Tahap 5: Aerodyne, kapal udara (XL, risiko tertinggi)
- Rakit kapal dari blok bebas, lalu jadi satu badan fisik.
- Daya angkat (balon/envelope), dorongan (baling-baling dari jaringan kinetik), kemudi, roda kemudi.
- Pemain berjalan di dek yang bergerak, tabrakan dengan dunia, berlabuh, dan simpan/muat.
- Sinkronisasi multiplayer.

**Selesai bila:** kapal kecil bisa dirakit, terbang ke pulau langit, berlabuh, lalu disimpan dan
dimuat ulang. Pemain kedua lewat LAN bisa ikut menumpang.

### Tahap 6: Arcana, sihir (L)
- Kristal aether (ore, worldgen), mana pemain, tongkat dan mantra (cooldown, biaya mana).
- Altar infus atau enchant baru.
- **Jembatan ke mesin:** mesin aether yang memberi tenaga ke jaringan Kinetica, dan mesin yang
  memproses material sihir.

### Tahap 7: Lost Realms, dunia (M–L)
- Struktur: desa fantasi, reruntuhan, menara penyihir, bengkel mekanik kuno, dungeon.
- Pulau langit untuk Era IV, tabel loot, dan bioma tambahan bila perlu.
- Sebagian besar berupa data (JSON + NBT struktur) yang didukung Horizon.

### Tahap 8: Chronicles, quest (M–L)
- Buku quest dengan bab per era, tugas (dapatkan, craft, bunuh, kunjungi struktur, bangun mesin),
  hadiah, progres per pemain yang tersimpan, dan sinkron saat multiplayer.

### Tahap 9: Mob dan bos fantasi (XL)
- Sistem entity di TCE: mob baru dengan model, animasi, AI, serta bos untuk dungeon dan
  pulau langit.
- Sebelum tahap ini, dungeon memakai mob vanilla yang diberi perlengkapan dan nama khusus.

### Tahap 10: Integrasi dan rilis (M)
- Keseimbangan resep antar era, buku panduan (ala Ponder), performa di HP, rilis Alpha dan Beta.

### Urutan yang saya sarankan

```
0 → 1 → 2 → (8 + 7 versi kecil) = ALPHA 1: bisa dimainkan, ada quest, dunia, mesin dasar
  → 6 → 3 = ALPHA 2: sihir + otomasi
  → 4 → 5 = ALPHA 3: contraption + kapal udara
  → 9 → 10 = BETA
```

Alasan: TCE (Tahap 1) adalah fondasi semua mod. Kinetica dasar paling cepat terasa "seperti
Create". Quest dan struktur kecil membuat Alpha 1 sudah punya alur main. Kapal udara paling
berisiko, jadi dikerjakan setelah contraption terbukti jalan.

## 5. Aset (tekstur, model, suara)

- Semua harus asli milik kita, karena aset Create "All Rights Reserved".
- **Model blok/item:** saya tulis sebagai JSON model Minecraft (bisa dibuka di Blockbench).
- **Tekstur 16×16:** saya buat dengan skrip pixel-art berbasis palet (logam, kayu, kuningan,
  kristal) supaya gayanya konsisten. Hasilnya rapi tapi sederhana. Anda atau seniman bisa
  menggantinya kapan saja tanpa mengubah kode.
- **Suara:** sementara pakai suara vanilla. Suara khusus bisa menyusul.
- Ini beban kerja yang besar. Kalau kualitas seni penting, sebaiknya ada seniman pixel-art.

## 6. Pengujian

| Di mana | Apa | Kapan |
|---|---|---|
| CI Linux | `cargo check` + unit test (logika jaringan kinetik, resep, quest) | Setiap push, murah |
| CI Windows | Build + snapshot launcher + game masuk dunia + skenario terskrip | Setiap milestone |
| Laptop Anda | Tachyon hasil CI + `horizon-ctl --offscreen`: saya pasang mesin, jalankan N tick, ambil screenshot, lalu periksa | Setiap fitur |
| HP Anda | APK: rasa main, performa (baris "Last session's performance") | Setiap Alpha |

**Laptop:** i3-1115G4 (2 core/4 thread), Intel UHD, RAM 7,8 GB.
- Cukup untuk *menjalankan* dan menguji Tachyon.
- Terlalu lambat untuk sering mengompilasi seluruh game. Kompilasi tetap di CI; di laptop hanya
  `cargo check` crate kita.
- **Ruang disk perlu dikosongkan:** E: sekarang 3,2 GB dan C: 2 GB. Butuh **minimal 15 GB kosong
  di E:** untuk resource game (~1 GB), build, dunia uji, dan screenshot.
- Saat tes berjalan, game memakai CPU dan GPU di latar belakang (tanpa mengambil alih layar).
  Sebaiknya Anda tidak sedang main game berat saat itu.

## 7. Biaya CI

- Repo privat mendapat sekitar 2000 menit GitHub Actions per bulan, dan menit Windows dihitung 2×.
  Satu build Windows sekitar 56 menit kuota, satu APK 12–18 menit.
- Strategi: job Linux untuk iterasi harian, build Windows dan APK hanya per milestone.
- Kalau kurang: GitHub Pro (3000 menit/bulan), atau build sebagian di laptop setelah disk lega.

## 8. Risiko

| Risiko | Dampak | Penanganan |
|---|---|---|
| Upstream Horizon sangat aktif | Konflik merge di titik hook | Hook minimal di file terpisah, merge rutin mingguan |
| Fisika kapal udara dan contraption | Bug tabrakan, desync multiplayer | Dikerjakan paling akhir, dengan prototipe dulu |
| Performa HP | FPS turun dengan banyak mesin | Instancing, simulasi per jaringan, batas jumlah |
| Beban seni | Tampilan sederhana | Skrip pixel-art dulu, seniman menyusul |
| Horizon nanti merilis API mod 0.3–0.5 | Duplikasi kerja | TCE dirancang mirip rencana API Horizon, bisa pindah nanti |
| Kuota CI | Iterasi lambat | Job Linux murah, Windows per milestone |

## 9. Keputusan (29 Oktober 2026)

1. **Nama modpack:** *Tachyon: Aether & Gears*. Nama mod: Kinetica (mesin), Aerodyne (kapal
   udara), Arcana (sihir), Chronicles (quest), Lost Realms (struktur).
2. **Seni:** skrip pixel-art dan model JSON dari saya. Bisa diganti seniman kapan saja.
3. **Urutan:** mesin dulu, sesuai saran di bagian 4.
4. **Ruang disk:** belum bisa dikosongkan. Untuk sementara semua tes lewat CI; tes di laptop
   menyusul setelah E: punya minimal 15 GB kosong.

## 10. Kemajuan

- **29 Oktober 2026, Tahap 0 selesai.**
  - Mod resmi Horizon sudah tertanam di Tachyon: Minimap, Recipes, Zoom, Coordinates HUD,
    FPS Counter, dan Keystrokes. FPS dan Keystrokes mati secara bawaan.
  - CI Linux menjalankan tes. CI Windows mengendalikan game lewat control socket dan mengambil
    screenshot (`.github/tachyon/scene.py`).
- **29 Oktober 2026, Tahap 1–2 dimulai.**
  - Konten bawaan (`tachyon/content`) selalu aktif dan ikut terpasang di Windows maupun APK.
  - Kinetica 0.1 berisi Shaft, Cogwheel, Large Cogwheel, Gearbox, Ferrostone Casing,
    Creative Motor, dan Ferrostone Alloy, lengkap dengan resep dan nama Inggris/Indonesia.
  - Bagian-bagian itu berputar sesuai aturan Create, dan kecepatannya dikirim server ke pemain
    (juga lewat LAN). Melihat sebuah bagian menampilkan RPM-nya.
  - Kecepatan motor diatur dengan klik kanan dan disimpan di dunia.
  - Cara kerjanya dijelaskan di [tce.md](tce.md).
- **29 Oktober 2026, Kinetica 0.2.**
  - Diverifikasi di CI Windows: blok Kinetica termuat, roda gigi berputar, dan HUD menampilkan RPM.
  - Ditambahkan:
    - engkol tangan;
    - kopling dan pemindah gigi (dikendalikan redstone);
    - kincir air (digerakkan aliran air);
    - kincir angin (butuh langit terbuka, lebih cepat di tempat tinggi);
    - Kacamata Insinyur (RPM hanya tampil saat kacamata dipakai atau dipegang);
    - timah (bijih, mentah, batangan, butiran, balok) yang muncul di dunia di tempat yang sama dengan
      tembaga, dan perunggu dari 3 tembaga + 1 timah.
- **29 Oktober 2026, Alpha 1 dimulai: Lost Realms 0.1.**
  - Struktur pertama, *Bengkel Terbengkalai* (`lost_realms:workshop_ruin`), berupa reruntuhan 7×5×7
    dengan mesin Kinetica, meja kerja, dan peti loot (alloy, poros, roda gigi, timah, kadang Kacamata
    Insinyur).
  - Muncul di hutan, taiga, rimba, bukit, sabana, dan dataran, rata-rata satu tiap 28 chunk.
  - Template NBT-nya ditulis oleh `tachyon/art/lost_realms.py`.
- **29 Oktober 2026, Chronicles 0.1 (buku quest).**
  - Ada 10 quest dalam dua bab, *Era Mesin* dan *Logam*. Buku dibuka dengan tombol J.
  - Progres disimpan oleh server per pemain, dan hadiahnya diberikan oleh server (aman untuk LAN).
- **30 Oktober 2026, Lost Realms 0.2 dan Chronicles 0.2.**
  - Struktur kedua, *Menara Pengawas Runtuh* (`lost_realms:watchtower`): menara batu 5×5 setinggi 12
    blok, sisi timurnya runtuh, tangga sampai atap, peti loot di atas (panah, besi, timah, peta,
    teropong). Muncul di dataran, padang, sabana, hutan, taiga, dan bukit berangin.
  - Quest punya dua jenis tugas baru: mesin yang berputar di dekat pemain (`turning`) dan menemukan
    struktur (`find`).
  - Ada 14 quest dalam tiga bab. Bab baru *Dunia yang Hilang* berisi quest menemukan bengkel dan
    menara; *Era Mesin* mendapat *Bergerak* (16 RPM) dan *Kecepatan Penuh* (128 RPM).
- **30 Oktober 2026, Kinetica 0.3: stress dan batu gilingan.**
  - Tahap 2 lengkap: setiap jaringan punya beban dan kapasitas (stress units), dan jaringan yang
    kelebihan beban berhenti. Kacamata menampilkan "beban / kapasitas SU".
  - Mesin pertama yang memakai tenaga: *Batu Gilingan*. Ia menggiling gandum jadi tepung (dipanggang
    jadi roti), batu bulat jadi kerikil, kerikil jadi pasir, tulang jadi bubuk tulang, tebu jadi gula,
    dan bunga jadi dua kali lipat pewarna. Ada 30 resep.
  - Quest baru *Roti Harian* (pegang 4 tepung), sehingga total ada 15 quest.
