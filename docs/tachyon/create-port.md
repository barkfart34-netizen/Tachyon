# Port Create: catatan dari source Create

Catatan untuk Tahap 2b dan seterusnya. Isinya membandingkan perilaku Create dengan mesin Tachyon,
diambil dari kode Create sendiri. Rencana besarnya ada di [rencana-modpack.md](rencana-modpack.md).

## Sumber dan lisensi

- Source yang dibaca: tag `mc1.21.1-6.0.10` di `github.com/Creators-of-Create/Create`. Tag ini sama
  dengan jar yang diunduh dari Modrinth (`create-1.21.1-6.0.10.jar`).
- Salinan lokal ada di `E:/Experiment/_work/create-src`, hanya `src/main/java` dan `LICENSE.md`.
  Salinan itu tidak masuk repo.
- Kode Create berlisensi MIT. Saat kode yang diporting dari Create masuk repo, tambahkan
  pemberitahuan MIT Create ke `THIRD_PARTY_NOTICES.md` dan sebut asalnya di kepala file Rust-nya.
- Aset Create (`src/main/resources/assets/`) berlisensi All Rights Reserved. Aset itu hanya diambil
  dari Modrinth di perangkat pemain dan tidak pernah masuk repo atau rilis.

## Yang sudah sama

- Aturan putaran (`RotationPropagator`):
  - poros ke poros;
  - roda gigi kecil bersebelahan berbalik arah (-1);
  - roda gigi besar ke kecil secara diagonal (-2 dan -0,5);
  - dua roda gigi besar bersudut siku;
  - roda gigi bercasing andesite dan brass memakai aturan persinggungan gigi yang sama;
  - arah sisi gearbox.
- Batas kecepatan 256 RPM.
- Beban per RPM:
  - millstone 4;
  - press 8;
  - mixer 4;
  - kipas 2.
- Engkol tangan: 32 RPM selama 10 tick setelah dipakai, kapasitas 8 per RPM.
- Kecepatan sabuk: RPM / 480 blok per tick.
- Kipas mengolah barang setelah 150 tick.

## Yang berbeda

| Bagian | Create | Tachyon sekarang |
|---|---|---|
| Roda gigi bercasing | Andesite/brass, kecil/besar: casing diam, gigi tetap bertaut. Koneksi poros mengikuti `top_shaft` dan `bottom_shaft`, keduanya `false` secara default. | Keempat blok memakai aturan roda gigi yang sama; casing impor diam, model shaftless berputar. Kedua ujung poros selalu tertutup karena template hanya punya `axis`. |
| Motor kreatif | Kapasitas 16384 per RPM | 16384 rata, tidak tergantung RPM |
| Kincir air | 8 RPM dikali arah aliran di sekitarnya: skor aliran dibatasi -1..1, jadi bisa berbalik. Kapasitas 32 per RPM. Lava juga bisa. | 8 RPM, kapasitas 16 per RPM |
| Kincir air besar | Skor aliran dari 12 titik tepi, dibatasi -1..1; 4 RPM, kapasitas 128 per RPM. Bidang 3×3 diisi `water_wheel_structure`. | Pemungutan suara dan angka sama; model Create berputar sepanjang `axis`. Blok penyangga 3×3 belum dipasang. |
| Kincir angin | Bearing + layar: RPM = jumlah layar / 8, dibatasi 1..16. Kapasitas 512 per RPM. Butuh contraption. | Blok tunggal, kapasitas 32 per RPM |
| Speedometer dan stressometer | Meneruskan putaran lewat kedua ujung poros, tanpa beban. Kacamata membaca RPM mutlak atau persentase beban dan sisa kapasitas. Orientasi memakai `facing` dan `axis_along_first`, dengan bentuk `GaugeShaper`. | Putaran dan pembacaan memakai jaringan yang sama, tanpa beban atau kapasitas sumber. Satu orientasi bawaan per sumbu X/Y/Z, casing Create tetap, tumbukan blok penuh; kepala dan jarum belum digambar. |
| Millstone | Kerja per tick = `clamp(int(abs(RPM)/16), 1, 512)`, bilangan bulat dan minimal 1. Lama per resep (`processing_time`). Berhenti bila satu dari 9 slot hasil penuh. | `RPM/16` pecahan; berhenti bila hasil lebih dari 9 tumpukan |
| Press | Satu siklus 240; tiap tick maju `lerp(clamp(abs(RPM)/512, 0, 1), 1, 60)`; menekan di tengah siklus. Satu barang per tekanan (`bulkPressing` mati). Mengerjakan barang yang tergeletak di blok tepat di bawahnya, sabuk/depot dua blok di bawah, dan baskom dua blok di bawah (*compacting*). | Depot satu atau dua blok di bawah; belum ada barang tergeletak dan compacting |
| Mixer | Kepala turun 20 tick (kurva kosinus). Mengaduk `max(log2(int(512/abs(RPM))) * ceil(t/100*15) + 1, 1)` tick, dengan `t` = `processing_time` resep. Naik 20 tick. Juga mengerjakan resep crafting shapeless vanilla. Ada syarat panas (blaze burner) dan fluida. | Waktu sendiri, tanpa panas dan fluida |
| Kipas | Jangkauan `lerp(min(abs(RPM)/256, 1), 3, 20)`. Waktu olah `150 * ((jumlah-1)/16 + 1) + 1`, jadi tumpukan besar lebih lama. RPM negatif menghisap. | Jangkauan `clamp(RPM*20/256, 3, 20)`; waktu tetap 150 |
| Corong | Mengeluarkan tiap 8 tick. Andesite: sampai satu tumpukan (64). Brass: sejumlah filternya. | 16 barang tiap 8 tick |
| Sabuk | Dipasang dengan item *belt connector* di antara dua poros, panjang maksimal 20 | Satu blok per segmen |
| Chute biasa | Satu tumpukan; gravitasi -0,2 blok per tick tanpa kipas. Mengambil sampai 16 dari inventaris; keluaran buntu menahan barang di tengah. Bisa mengalir turun atau naik melalui cabang chute. | Gerak, transfer ujung, hopper/corong, ejeksi, penyimpanan, dan model statis sudah dipasang. Bentuk tabrakan dan beberapa aturan aliran serta interaksi masih berbeda; lihat catatan chute di bawah. |

## Resep Create

Jumlah resep per jenis di jar 6.0.10:

| Jenis | Jumlah |
|---|---|
| `create:milling` | 246 |
| `create:crushing` | 211 |
| `create:deploying` | 167 |
| `create:splashing` | 52 |
| `create:pressing` | 43 |
| `create:cutting` | 32 |
| `create:haunting` | 22 |
| `create:filling` | 19 |
| `create:mixing` | 14 |
| `create:compacting` | 7 |

Ada juga beberapa resep lain: emptying, item_application, sequenced_assembly, dan mechanical_crafting.

**Format:**
- `ingredients`: `{"item": ...}` atau `{"tag": ...}`, plus fluida `{"type": "neoforge:single", "fluid", "amount"}`.
- `results`: `{"id", "count", "chance"}`, atau fluida `{"id", "amount"}`.
- `processing_time` (opsional).
- `heat_requirement`: `heated` atau `superheated`.
- `neoforge:conditions`: biasanya `not tag_empty`, artinya resep dilewati bila tag-nya kosong.

**Rencana pemakaian:**
- Porter Horizon menyimpan resep berjenis khusus apa adanya di `horizon-mods/create/data/create/recipe/<jenis>/`.
  Resep yang hasilnya bukan item terdaftar dipindah ke `outside/`.
- Mesin `create:*` memakai resep Create. Mesin Kinetica tetap memakai resep Kinetica.
- Tag dibaca dari tag item game.
- Resep yang memakai fluida dilewati sampai fluida ada (Tahap 3b).

## Urutan kerja 2b

**Kemajuan (1 November 2026):**
- **Langkah 1 selesai:** `tachyon_engine/create_recipes.rs` membaca milling, crushing, pressing, splashing,
  haunting, mixing, dan compacting.
  - Mesin `create:*` mendahulukan resep Create, mesin Kinetica mendahulukan resep Kinetica.
  - Press di atas baskom mengerjakan compacting.
- **Langkah 2 sebagian:** angka yang sudah disamakan dengan Create:
  - kapasitas motor kreatif (16384 per RPM);
  - kincir air (pemungutan suara aliran, 8 RPM, 32 per RPM);
  - kincir air besar (12 titik tepi, 4 RPM, 128 per RPM);
  - kerja millstone (bilangan bulat, 1..512);
  - jangkauan kipas;
  - corong (satu tumpukan per 8 tick);
  - waktu mixer.
- **Langkah 3 dimulai:**
  - **Crushing wheel** (`tachyon_engine/crusher.rs`) bekerja seperti controller Create:
    - celah di antara dua roda yang berputar berlawanan menelan satu tumpukan barang;
    - hitung mundur `speed / log2(jumlah)` per tick, selesai di bawah 20;
    - barang tanpa resep hilang setelah 100, sama seperti Create;
    - hasilnya keluar searah putaran pelek dan tidak ditelan lagi oleh celah yang sama;
    - isi celah disimpan di file mesin begitu masuk atau keluar;
    - roda yang berhenti menyimpan isinya, roda yang dihancurkan menjatuhkannya;
    - hopper di bawah celah tidak menyedot barang yang sedang menunggu di celah;
    - jendela host dan tamu LAN menampilkan empat partikel per tick dari barang yang sedang dihancurkan;
      barang blok memakai partikel blok, barang lain memakai partikel item;
    - celah yang berputar melukai satu mob atau pemain dengan damage dasar 4 (`create:crush`,
      pesan kematian `create.crush`), melewati armor dan mengikuti cooldown (serta tingkat kesulitan
      untuk pemain);
      pemain kreatif, spectator, dan semua entitas pada peaceful tidak dilukai;
    - damage hanya dihitung pemilik dunia, termasuk untuk pemain tamu LAN; celah berisi tumpukan
      barang tidak menerima entitas hidup lain.
  - **Model OBJ Create** (`obj_model.rs`, loader `neoforge:obj`) dipakai untuk crushing wheel dan
    kincir air.
  - **Tag konvensi** `c:` yang tidak ada di game ditebak dari namanya (`c:ingots/iron` →
    `minecraft:iron_ingot`), jadi resep press, mixer, dan crushing yang memakai tag ikut terbaca.
- **Waktu kipas** sekarang menurut besar tumpukan, seperti Create: `150 * ((jumlah - 1) / 16 + 1) + 1` tick.
- **Siklus press** seperti Create (`tachyon_engine/press.rs`):
  - satu siklus 240, maju `lerp(|RPM|/512, 1, 60)` per tick;
  - kepala turun di tengah siklus dan menekan satu barang di depot;
  - compacting di baskom sekali per siklus;
  - press tanpa depot atau baskom menekan barang yang tergeletak di blok bawahnya;
  - peluang hasil dikocok per barang (`rollOutput`).
- **Haunting kipas selesai:** soul fire dan soul campfire yang menyala mengolah tumpukan barang
  tergeletak maupun di depot lewat resep `create:haunting`. Barang tanpa resep tetap utuh.
  - Air atau blok waterlogged tetap menang atas haunting, lalu smoking dan blasting, sesuai prioritas Create.
  - Partikelnya memakai soul fire flame dan sesekali asap; nama proses tampil di kacamata dalam bahasa
    Inggris dan Indonesia.
  - Hasil cuci dan haunting dikocok per barang, seperti `RecipeApplier` Create.
- **Kincir air besar bisa berputar dan digambar** (`tachyon_engine/water_wheel.rs`):
  - memeriksa 12 titik di tepi, dua blok dari tengah, bukan empat tetangga kincir kecil;
  - suara berlawanan saling membatalkan; hasilnya -4, 0, atau 4 RPM, kapasitas 128 per RPM (512 SU saat berputar);
  - memakai model `create:block/large_water_wheel/block`, atau `block_extension` bila ada kincir besar
    di sisi negatif poros, seperti `LargeWaterWheelBlock` dan `WaterWheelRenderer`;
  - belum memasang `water_wheel_structure` di bidang 3×3: tumbukan dan pemilihan masih hanya di blok tengah.
- **Chute biasa ditambahkan** (`tachyon_engine/chute.rs`), mengikuti `ChuteBlock`, `AbstractChuteBlock`,
  `ChuteBlockEntity`, dan `ChuteItemHandler`:
  - `create:chute` tidak berputar dan hanya menampung satu tumpukan; tumpukan kedua tidak digabungkan.
  - Gerak per tick = `(clamp((push + pull) / 64, -20, 20) + (push + pull <= 0 ? -4 : 0)) / 20`.
    Tanpa kipas barang turun 0,2 blok per tick; aliran positif mengangkatnya.
  - Mengambil sampai 16 barang dari inventaris atas, atau dari bawah saat aliran naik. Chute lain
    mengirim melalui sambungannya; chute miring mengarah ke blok bawah yang berlawanan dengan facing-nya.
  - Memeriksa ruang keluaran setelah melewati tengah, lalu memindahkan barang hanya setelah melewati
    ujung. Inventaris penuh menahannya di tengah; sisa transfer sebagian tetap disimpan.
  - Ujung bawah yang terbuka menjatuhkan tumpukan dengan kecepatan vertikal -0,25; ujung atas
    mengeluarkannya dengan kecepatan dua kali gerak chute. Hopper dan corong dapat mengisi atau mengambil isinya.
  - Kipas vertikal memberi aliran pada chute pertama yang dicapai jalur udaranya. Pull dijumlahkan
    dari cabang atas; push dibagi ke cabang atas. Kipas horizontal tidak mengangkat isi chute.
  - Isi, damage, komponen item, posisi, push/pull, dan jangkauan hisap disimpan dalam file mesin per
    dimensi. Chute yang dihancurkan menjatuhkan isinya; chunk yang tidak dimuat tetap menyimpannya.
    Entri simpanan dibaca satu per satu, sehingga satu entri rusak tidak menghapus isi chute lain.
    Interpolasi dimulai dari tinggi yang disimpan, seperti `ChuteBlockEntity.read`.
  - Template tangga menyimpan lima facing dan empat shape lewat alias `half` dan `shape`, tanpa
    perilaku tangga vanilla. Model statis Create tetap di mesh chunk; asetnya hanya berasal dari impor di perangkat pemain.
  - Renderer item depot dipakai kembali, berskala 0,5 dan berputar mengikuti tinggi yang diinterpolasi,
    sesuai aturan visibilitas `ChuteRenderer`. Tinggi dikirim lewat payload kinetik versi 7, bersama barang di celah crushing wheel.
  - Unit test ditulis untuk gerak, transfer, ejeksi, cabang udara, alias/model, penyimpanan komponen,
    dan pemulihan tinggi serta isi chute lain ketika ada entri simpanan rusak.
    Build, test, dan permainan tidak dijalankan pada pengerjaan ini; verifikasi diserahkan ke CI.
- **Roda gigi bercasing bisa bertaut dan digambar**:
  - `create:andesite_encased_cogwheel`, `create:brass_encased_cogwheel`,
    `create:andesite_encased_large_cogwheel`, dan `create:brass_encased_large_cogwheel`;
  - aturan putaran dan offset gigi sama dengan roda tanpa casing; casing tetap diam;
  - bagian berputar memakai `create:block/cogwheel_shaftless` dan
    `create:block/large_cogwheel_shaftless`;
  - kedua ujung poros tetap tertutup, sesuai default Create. Template `minecraft:quartz_pillar`
    hanya memberi `axis=x/y/z`, belum mendukung `top_shaft` dan `bottom_shaft`.
- **Speedometer dan stressometer: putaran dan pembacaan kacamata selesai** (`kinetic.rs`, `tachyon_engine.rs`,
  `tachyon/create_port.rs`):
  - meneruskan RPM bertanda lewat kedua ujung poros X/Y/Z, tanpa membangkitkan putaran atau menambah beban;
    dampak stress dan kapasitas sumber keduanya nol;
  - kacamata Create (`create:goggles`) maupun Kinetica bekerja di kepala atau di tangan mana pun;
    speedometer membaca RPM mutlak, termasuk 0 RPM saat tidak digerakkan;
  - stressometer membaca persentase beban (dipotong ke bilangan bulat) dan sisa/total kapasitas:
    128 dari 512 menjadi `25%`, `384 / 512 SU`; kelebihan beban 384 dari 256 tetap terbaca
    `150%`, `-128 / 256 SU` saat poros berhenti. Status macet didahulukan;
  - hanya tiga orientasi bawaan dipertahankan, satu per sumbu, dengan model `gauge/base` dan `gauge/base_wall`
    serta rotasinya tetap. Tumbukan memakai blok penuh pilar kuarsa, bukan bentuk `GaugeShaper`;
  - kepala dan jarum, partikel, comparator, serta sambungan display belum dikerjakan.
- **Belum:**
  - tarikan mob dan pemain melalui celah crushing wheel, serta posisi keluarnya sebelum mati.
- Pemberitahuan MIT Create sudah ada di `THIRD_PARTY_NOTICES.md`.

1. **Pembaca resep Create.** Mesin `create:*` memakai:
   - milling di millstone;
   - pressing di press;
   - mixing dan compacting di mixer dan press (tanpa fluida dan panas dulu);
   - splashing dan haunting di kipas.
2. **Angka disamakan** dengan tabel di atas:
   - kerja millstone;
   - siklus press: satu barang per tekanan, plus barang yang tergeletak;
   - waktu mixer;
   - jangkauan dan waktu kipas;
   - jumlah corong;
   - kincir air: kapasitas 32, kecepatan mengikuti arah aliran;
   - kincir air besar: kapasitas 128, 4 RPM menurut suara aliran di 12 titik tepi;
   - kapasitas motor kreatif.

   Kinetica ikut berubah supaya keduanya terasa sama.
3. **Mesin Create berikutnya:**
   - crushing wheel (211 resep crushing);
   - mechanical saw (cutting);
   - deployer;
   - blaze burner (panas untuk mixing);
   - belt connector;
   - smart chute (chute biasa sudah ada; interaksinya masih perlu dilengkapi);
   - mechanical arm.
4. **Kincir angin dan bearing** menunggu contraption (Tahap 4).

## Chute: batas implementasi

- Semua state memakai delapan kotak dinding berongga yang sama. Bentuk tabrakan dan seleksi belum
  mengikuti geometri miring, intersection, dan casing `ChuteShapes`; format deklarasi shape hanya memilih satu properti.
- Barang jatuh diterima lewat pemeriksaan bounding box dan tinggi tick sebelumnya, bukan callback
  tabrakan `updateEntityAfterFallOn` yang persis sama dengan Create.
- Jalur kipas dan jangkauan hisap bawah berhenti pada blok dengan tabrakan penuh. Pemeriksaan kedalaman
  bentuk parsial `AirCurrent.getFlowLimit` belum ada. Create menempelkan kipas langsung ke chute;
  Tachyon memakai chute pertama dalam jalur udara kipas. Aliran dihitung tiap tick dari kecepatan putaran
  yang terakhir diselesaikan, sehingga perubahan kipas dapat terlambat satu tick.
- Pengambilan inventaris memakai slot pertama yang bisa diambil; belum mengumpulkan barang yang cocok
  dari beberapa slot seperti `ItemHelper.extract`. Chute ditick dari bawah ke atas, sehingga transfer
  naik bisa bergerak lagi di chute penerima pada tick yang sama.
- Penyimpanan mempertahankan komponen item, tetapi payload gambar masih hanya membawa nama dan jumlah.
  Tampilan yang bergantung pada komponen belum tersinkron. Skala khusus package Create juga belum ada.
- Impor Create lama perlu diulang agar mendapat template dan alias baru; tidak ada migrasi state lama.
  Klien dan server harus sama-sama memakai payload versi 7. Model belum diperiksa dengan jar/aset Create nyata.
- Smart chute, mengambil isi dengan tangan kosong, wrench/window/casing, partikel dan advancement,
  serta penarikan barang sabuk/depot melalui `TransportedItemStackHandlerBehaviour` belum diporting.
  Larangan menaruh chute tepat di bawah chute miring diperiksa saat pemasangan; penghapusan chute yang
  menjadi tidak valid akibat perubahan tetangga belum diterapkan.
