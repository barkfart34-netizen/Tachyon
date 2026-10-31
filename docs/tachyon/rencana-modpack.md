# Rencana modpack Tachyon: fantasi + era mesin

Status: **Tahap 0–2 selesai; Tahap 3 dan port Create berjalan** (kemajuan di bagian 10, urutan
terbaru di bagian 9). Ditulis 29 Oktober 2026.

Nama modpack: **Tachyon: Aether & Gears**.

## 1. Tujuan

Modpack milik Tachyon sendiri, jalan di **PC (Windows) dan Android**, bisa dimainkan bersama
lewat LAN/server, berisi:

| Bagian | Mirip dengan | Nama kerja |
|---|---|---|
| Mesin kinetik: poros, gear, proses, logistik, contraption | Create | **Kinetica** |
| Kapal laut, kapal udara, pesawat, kendaraan | Create Aeronautics asli, diunduh dari Modrinth seperti Create | **Create Aeronautics** |
| Sihir: kristal aether, mana, mantra, altar | mod magic (Ars, Botania) | **Arcana** |
| Mesin + sihir: mesin bertenaga mana, mesin pengolah bahan sihir, tempa senjata dewa | addon Create bertema sihir | **Runeforge** (nama kerja) |
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
| IV. Zaman Langit | Kapal udara, pulau langit, dungeon langit, bos | Create Aeronautics, Lost Realms |

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
  Horizon (API 0.3 sampai 0.5) tanpa jadwal. Karena itu Kinetica, Arcana, dan lainnya ditulis
  **native di dalam Tachyon** (lihat bagian 3).
- **Tiruan, bukan salinan 1:1.** Mekaniknya akan terasa mirip Create, tapi angka, resep, dan
  detailnya milik kita.

## 3. Arsitektur

```
 ┌──────────────────────────────────────────────────────────────┐
 │ Modpack (resep, quest, keseimbangan, progresi antar era)     │
 ├────────────┬────────────┬──────────┬────────────┬────────────┤
 │ Kinetica   │ Runeforge  │ Arcana   │ Chronicles │ Lost Realms│  ← crate Rust + aset kita
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

### Tahap 4b: laut dengan Create (L)
- Perlengkapan selam Create (helm dan tangki udara), mesin yang bekerja di bawah air.
- Pompa dan *hose pulley* untuk menguras atau mengisi air dalam jumlah besar (dipakai di arena
  Kraken, lihat [pembunuh-dewa.md](pembunuh-dewa.md)).

**Selesai bila:** pemain bisa menyelam lama dengan perlengkapan Create, dan hose pulley menguras
satu kolam.

### Tahap 5: Create Aeronautics, kapal laut dan udara (XL, risiko tertinggi)
- Create Aeronautics asli diunduh dari Modrinth seperti Create. Lisensinya sama: kode MIT, aset All
  Rights Reserved, jadi asetnya hanya diambil di perangkat pemain. Kodenya (MIT) diporting ke Rust.
- Fisika: Aeronautics memakai library **Sable**, yang intinya sudah ditulis dalam Rust di atas
  **Rapier** (`sable_rapier/src/main/rust`: daya apung, tabrakan blok, sambungan, tali).
  - Tachyon memakai Rapier langsung (Apache-2.0).
  - Sable berlisensi PolyForm Shield (boleh dipakai dan dimodifikasi, kecuali untuk produk pesaing),
    jadi kodenya hanya dijadikan acuan, tidak disalin.
- Rakitan blok menjadi satu badan fisik yang mengapung di air (kapal, kapal selam), terbang
  (balon, baling-baling, levitite), atau melaju di darat.
- Pemain berjalan di dek yang bergerak, tabrakan dengan dunia, berlabuh, simpan/muat, dan
  sinkronisasi multiplayer.
- Urutan: kapal laut dulu (daya apung), lalu kapal udara, lalu kendaraan darat.

**Selesai bila:** kapal kecil dirakit di pantai lalu berlayar, kapal udara terbang ke pulau langit
dan berlabuh, keduanya tersimpan dan termuat ulang, dan pemain kedua lewat LAN bisa ikut menumpang.

### Tahap 6: Arcana, sihir (L)
- Kristal aether (ore, worldgen), mana pemain, tongkat dan mantra (cooldown, biaya mana).
- Altar infus atau enchant baru.

### Tahap 6b: Runeforge, mesin + sihir (L)
- Mod tersendiri yang menggabungkan Kinetica/Create dengan Arcana.
- Mesin yang mengubah putaran jadi mana, dan motor bertenaga mana yang memutar jaringan kinetik.
- Mesin yang mengolah bahan sihir: press, mixer, dan kipas dengan proses sihir.
- Tempa senjata dewa: relik + otoritas ditempa jadi senjata (lihat
  [pembunuh-dewa.md](pembunuh-dewa.md)), dengan aether di tingkat tertinggi.

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

### Keputusan 31 Oktober 2026

1. **Create asli dari Modrinth**, digerakkan mesin Tachyon; Kinetica tetap jadi cadangan offline.
   Karena Tachyon pribadi (hobi, dimainkan bersama teman), kode Create (MIT) dibaca dari source
   resminya (tag `mc1.21.1-6.0.10`) dan diporting ke Rust dengan pemberitahuan lisensinya. Aset Create
   tetap tidak pernah masuk repo atau rilis.
2. **Create didahulukan, termasuk bagian laut dan udara**, karena paling sulit dan paling
   berpengaruh. Urutan baru:

   ```
   Create 2b (perilaku mesin dari kode Create, resep Create) → fluida (3b) → contraption (4)
     → laut dengan Create (4b) → Create Aeronautics (5) → Arcana (6) → Runeforge (6b)
     → cerita Pembunuh Dewa: entity, boss, kartu skill (9)
   ```

3. **Mod baru untuk mesin + sihir** (nama kerja *Runeforge*, Tahap 6b).
4. **Rancangan cerita "Pembunuh Dewa"** ([pembunuh-dewa.md](pembunuh-dewa.md)) memperluas Aether &
   Gears, bukan menggantinya.
5. **Create Aeronautics asli dipakai untuk laut dan udara, dan Aerodyne dihapus.** Aerodyne belum
   dibuat, jadi tidak ada yang hilang. Eureka tidak dipakai, karena butuh Valkyrien Skies (mesin
   fisika lain), resminya hanya sampai Minecraft 1.20.1, dan fiturnya sudah ada di Aeronautics.

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
- **30 Oktober 2026, perbaikan dari pemain.**
  - Motor kreatif tidak lagi berlubang di dasarnya. Templatenya kini end portal frame (setinggi 13
    piksel), dan modelnya diberi pelat penutup lantai.
  - Ujung poros pada casing (gearbox, kopling, pemindah gigi) dibuat sedikit menonjol dari permukaan
    casing, sehingga tidak lagi berkedip (z-fighting).
  - Android:
    - Bawaan 60 FPS di dalam dunia dan 30 di menu (mode baterai), bukan 120. Main lama di 120 FPS
      membuat HP panas lalu melambat.
    - Batas FPS turun ke 45 saat HP berstatus panas *moderate*, dan ke 30 saat *severe* ke atas.
    - Baris performa kini mencatat memori, suhu, FPS awal, dan lama bermain.
- **30 Oktober 2026, Kinetica 0.4: Kinetica II bagian 1 (press, depot, bijih hancur, hopper).**
  - Blok baru:
    - *Depot*: nampan untuk satu tumpukan.
    - *Mesin Press*: menempa isi depot di bawahnya. Batangan jadi lembaran besi, tembaga, emas,
      perunggu, atau timah, dan tebu jadi kertas.
  - Batu gilingan kini menggiling bijih mentah jadi bijih hancur (kadang lebih satu), yang dilebur
    jadi batangan.
  - Hopper vanilla bisa mengisi dan mengosongkan batu gilingan serta depot, sehingga lini
    "hopper → depot + press → hopper" berjalan tanpa pemain.
  - Quest baru *Bijih Hancur* dan *Logam Tempa*, sehingga total ada 17 quest. Penampil resep
    mendapat tab mesin press.
- **30 Oktober 2026, Kinetica 0.5: mesin pengaduk dan baskom.**
  - *Baskom* menampung bahan.
  - *Mesin Pengaduk* di atasnya (langsung, atau dua blok di atasnya seperti Create) mengaduk bahan
    jadi paduan ferrostone, perunggu, bubuk beton, dan lainnya (25 resep).
  - Press kini juga bekerja dua blok di atas depot.
  - Quest baru *Diaduk Rata*, sehingga total ada 18 quest.
  - Semua blok dan barang mod ada di tab kreatif "Tachyon", jadi tidak perlu lagi memakai perintah
    `/give`.
  - Android: blur latar menu kini mati secara bawaan, karena membuka menu jeda membuat HP lag.
- **30 Oktober 2026, Kinetica 0.6: kipas terselubung.**
  - Angin kipas mendorong barang. Lewat air, angin mencuci barang (4 bijih hancur jadi 40 butiran
    besi); lewat api atau campfire mengasapi; lewat lava melebur. Barang di depot dalam aliran angin
    ikut diolah.
  - Item baru *Baling-Baling* untuk resep kipas.
  - Quest baru *Angin Segar*, sehingga total ada 19 quest.
  - Lini bijih → hancur → cuci → lebur → lembaran kini bisa dibuat. Yang masih kurang tinggal belt
    supaya lini itu sepenuhnya tanpa pemain.
- **30 Oktober 2026, Kinetica 0.7: sabuk mekanis dan corong.**
  - Sabuk membawa barang di atasnya. Segmen yang berjajar berputar bersama dari poros di sampingnya.
  - Corong memasukkan barang ke peti, tungku, atau mesin. Bila diklik, corong berganti mengeluarkan
    isi wadah ke sabuk.
  - Quest baru *Ban Berjalan*, sehingga total ada 20 quest.
- **31 Oktober 2026, Create dari Modrinth (tahap 1 dan 2a, belum dicek CI).**
  - Keputusan pengguna: Create asli diunduh dari Modrinth di perangkat pemain, lalu mesin Kinetica
    menggerakkan blok `create:*`. Kinetica tetap ada sebagai cadangan offline. Hanya untuk Tachyon,
    tidak diusulkan ke Horizon.
  - Lisensi Create: hanya `assets/` (tekstur, model, suara) yang All Rights Reserved; kode dan
    `data/` MIT. Repo dan rilis Tachyon tidak pernah memuat aset Create.
  - Tahap 1 (`tachyon/fetch.rs`): tombol "Get Create" di launcher desktop (halaman Installations)
    dan Android (baris 05). Pemain ditanya dulu ("about 19 MB, Download?").
  - File diambil dari CDN Modrinth berdasarkan SHA-512 yang dikunci, ditolak bila hash berbeda, dan
    disimpan di `cache` untuk dipakai offline.
  - Hanya `assets/create/`, `data/create/`, `data/c/`, `data/minecraft/`, dan metadata yang diambil
    dari jar (kodenya tidak), lalu diporting dengan importer yang sudah ada. CI memakai jalur yang
    sama (`TACHYON_FETCH=create`).
  - Tahap 2a (`tachyon/create_port.rs` dan `create_part` di TCE):
    - template yang sesuai bentuk untuk blok kinetik Create;
    - poros dan roda gigi keluar dari mesh chunk, lalu digambar berputar oleh mesin dengan model
      parsial Create sendiri (shaft_half, cogwheel, millstone/inner, press head, mixer head/pole,
      propeller);
    - kacamata memakai nama blok Create.
  - Berikutnya (2b): resep Create (`create:milling`, `pressing`, `mixing`, `splashing`) untuk
    mesinnya, sabuk Create (belt connector), dan kincir air (model OBJ).
