# Ide lanjutan: sistem tempur dan peradaban modern menuju luar angkasa

Catatan ide dari pemilik proyek (5 November 2026), ditambah usulan Claude. Isinya belum
dijadwalkan. Rencana besarnya ada di [rencana-modpack.md](rencana-modpack.md).

Semua desain di sini milik Tachyon sendiri. "Ala Hytale" berarti rasa permainannya saja: tanpa
menyalin kode, aset, atau nama dari Hytale.

## 1. Sistem tempur

### Dasar: rasa ala Hytale

Tempur bergaya Minecraft 1.9 diganti dengan tempur berbasis **gerakan senjata**.

**Serangan:**
- **Serangan ringan dan berat.**
  - Klik atau ketuk untuk serangan ringan.
  - Tahan untuk serangan berat yang diisi.
  - Rangkaian ringan → ringan → berat membentuk **kombo** yang berbeda untuk tiap jenis senjata.
- **Gaya per jenis senjata:**
  - pedang: kombo cepat dan tebasan menyapu;
  - kapak: berat, menembus tangkisan;
  - tombak: jangkauan panjang dan tusukan maju;
  - palu: menghantam tanah, area kecil;
  - belati: sangat cepat, bonus dari belakang;
  - busur: membidik dengan tarikan penuh.
- **Skill senjata.** Bar energi terisi dari serangan yang kena. Saat penuh, senjata bisa memakai
  satu jurus khusus, misalnya:
  - pedang: tebasan berputar;
  - tombak: lompatan menghunjam.

**Bertahan:**
- **Menghindar.** Gulingan atau langkah cepat ke empat arah. Ada jendela kebal yang singkat,
  dan memakai stamina.
- **Menangkis.** Menahan untuk mengurangi damage dan memakai stamina.

**Stamina:** satu bar untuk berlari, menghindar, menangkis, dan serangan berat. Bar ini pulih saat
tidak dipakai.

### Modifikasi Tachyon agar lebih seru

- **Parry tepat waktu.**
  - Menangkis tepat sebelum kena (sekitar 0,2 detik) membuat musuh **goyah**: terbuka sebentar
    dan menerima damage lebih.
  - Menangkis terlambat hanya mengurangi damage.
- **Postur.**
  - Musuh, termasuk bos, punya bar postur yang terkikis oleh serangan dan parry.
  - Postur yang habis membuka **serangan penghabisan**.
  - Bos jadi bisa dikalahkan dengan keterampilan, bukan hanya dengan armor tebal.
- **Elemen dari sihir Runeforge.**
  - Senjata yang diberi rune mendapat efek: api membakar, es memperlambat, petir merambat ke musuh
    dekat, aether membuat ringan dan melontarkan.
  - Kombo yang diakhiri serangan berat **memicu** elemennya.
- **Senjata mekanis dari Create.** Senjata yang perlu diisi tekanan atau putaran di bengkel:
  - palu pneumatik dengan hantaman pegas;
  - gergaji tangan yang berputar;
  - crossbow beroda gigi untuk tembakan beruntun.

  Senjata ini kuat, tetapi bahan bakarnya harus dikelola. Ini menghubungkan tempur dengan
  pabrik pemain.
- **Kerja sama** (cocok dengan [server bersama](server-bersama.md)):
  - musuh yang goyah menerima bonus dari serangan pemain lain;
  - pemain yang jatuh bisa diselamatkan teman dalam beberapa detik sebelum mati.

### Kontrol

- **PC:**
  - klik kiri: ringan;
  - tahan klik kiri: berat;
  - klik kanan: tangkis atau parry;
  - Shift + arah: menghindar;
  - R: skill senjata.
- **HP**, memakai HUD sentuh ala Bedrock (Tahap A):
  - ketuk layar: ringan;
  - tahan: berat;
  - tombol perisai: tangkis;
  - usap tombol menghindar ke suatu arah: menghindar;
  - tombol skill menyala saat energi penuh.

  Parry di HP diberi jendela waktu sedikit lebih longgar.
- Tetap ada **bidik lunak** (*soft lock*) ke musuh terdekat di depan, bisa dimatikan.

### Yang perlu dibangun

- Animasi tangan dan senjata orang pertama, plus pose orang ketiga. Bisa dimulai dengan animasi
  prosedural, tanpa file animasi.
- Hitbox serangan berbentuk busur atau garis per gerakan, bukan sekadar "entitas yang ditunjuk".
- AI mob yang membaca serangan: menghindar, menangkis, punya pola serangan dengan **aba-aba**
  yang bisa dibaca pemain.
- Sinkronisasi LAN: host tetap yang menentukan kena atau tidak, seperti damage crushing wheel
  sekarang.

## 2. Peradaban modern di Create: kunci menuju luar angkasa

**Gagasan pemilik:** teknologi Create tumbuh dari zaman roda gigi ke zaman modern, dan
peradaban modern itulah yang membuka eksplorasi luar angkasa.

### Zaman teknologi

| Zaman | Inti | Contoh isi |
|---|---|---|
| 1. Roda gigi | Create biasa | Kincir, sabuk, press, mixer |
| 2. Uap | Steam engine Create, boiler | Pabrik besar, kereta |
| 3. Listrik | Generator dari putaran, kabel, baterai, motor listrik | Lampu kota, mesin listrik, listrik bisa diubah lagi menjadi putaran |
| 4. Industri modern | Kilang minyak, plastik, baja paduan, kaca diperkuat | Pipa fluida besar (Tahap 3b), menara distilasi |
| 5. Elektronik | Silikon, sirkuit, prosesor | Komputer, radar, autopilot kereta |
| 6. Roket | Bahan bakar roket, paduan ringan, landasan peluncuran | Roket bertingkat dirakit sebagai contraption (Tahap 4) |
| 7. Luar angkasa | Stasiun orbit, Bulan, planet | Pakaian antariksa, oksigen, gravitasi rendah |

### "Peradaban", bukan sekadar mesin

Supaya terasa membangun peradaban, bukan hanya membuat item:
- **Pemukiman.**
  - Penduduk desa bisa direkrut menjadi pekerja dan teknisi di kota pemain.
  - Kota punya angka **populasi, listrik, dan riset**.
- **Riset.**
  - Laboratorium menghasilkan poin riset dari sampel: mineral baru, data dari Aether, dan
    rongsokan meteor.
  - Pohon riset membuka zaman berikutnya.
- **Syarat luar angkasa.**
  - Peluncuran roket pertama membutuhkan, misalnya:
    - kota dengan populasi tertentu;
    - jaringan listrik yang stabil;
    - riset Roket selesai.
  - Kunci ini dibuka lewat kemajuan peradaban, bukan lewat satu item.
- **Kereta antarkota.** Kereta Create menghubungkan kota-kota pemain dan memasok bahan ke
  landasan roket.

### Luar angkasa

- **Jalan ke sana lewat Aether.** Nama modpack *Aether & Gears*: Aether adalah langit, dan di
  atasnya adalah antariksa.
  - Ketinggian Aether menjadi tempat uji balon cuaca dan pesawat (Aeronautics).
  - Roket pertama diluncurkan dari pulau Aether yang tinggi.
- **Tujuan:**

  | Tujuan | Isi |
  |---|---|
  | Orbit | Stasiun yang dibangun pemain, gravitasi nol |
  | Bulan | Gravitasi rendah, debu regolit, helium-3 untuk reaktor |
  | Planet merah | Badai debu, gua es, sisa peradaban kuno |
  | Planet es | Laut di bawah es, makhluk bercahaya |

- **Kelangsungan hidup:** oksigen dari tabung atau tanaman di kubah, suhu ekstrem, dan radiasi saat
  badai matahari.
- **Bahan khusus tiap planet** menjadi kunci mesin dan senjata tingkat akhir. Jadi setiap planet
  punya alasan untuk dikunjungi.

## 3. Ide tambahan dari Claude

- **Tempur luar angkasa yang memakai sistem tempur yang sama.** Di gravitasi rendah, lompatan dan
  hantaman palu jadi lebih jauh. Musuh di Bulan dan planet merah punya pola sendiri.
- **Meteor jatuh.** Kejadian langka di Overworld: kawah berisi bahan riset dan mob asing.
  - Menjadi pemicu cerita bahwa ada sesuatu di luar sana.
  - Menjadi sumber poin riset awal.
- **Peradaban kuno di planet merah** sebagai jembatan ke Lost Realms dan Chronicles (Tahap 7–8).
  - Reruntuhannya menyimpan cetak biru mesin yang tidak bisa diriset sendiri.
  - Quest berantai membawa pemain dari Overworld ke Aether lalu ke antariksa.
- **Bos per zaman.** Contohnya:
  - golem uap di zaman uap;
  - drone pengintai rusak di zaman elektronik;
  - penjaga reruntuhan di planet merah.

  Bos memakai bar postur dari sistem tempur.
- **Peran dalam grup** untuk 3 pemain di server bersama: insinyur (pabrik), penjelajah (bahan dan
  planet), dan petarung (bos). Semua tetap bisa melakukan semuanya, tetapi peran memberi bonus
  kecil supaya kerja sama terasa.
- **Ramah HP.**
  - Mesin besar dan kota boleh banyak, tetapi:
    - simulasi di chunk yang jauh dari pemain diringkas, misalnya pabrik dihitung per menit
      bukan per tick;
    - partikel dan model dikurangi otomatis di HP.
  - Ini sejalan dengan host terbaik di [server bersama](server-bersama.md).

## 4. Urutan yang masuk akal (bila proyek dilanjutkan)

Tempur bisa dikerjakan bersamaan dengan Tahap A, karena sama-sama butuh kontrol sentuh.

```
Create 2b → kontrol Bedrock (A) → tempur dasar → fluida (3b) → contraption (4)
        → listrik dan industri modern → roket dan orbit → Bulan dan planet
```

Sihir (Runeforge), Lost Realms, Chronicles, dan bos (Tahap 6–9) disisipkan di antaranya,
sesuai urutan di rencana utama.

**Perkiraan ukuran:**

| Bagian | Ukuran |
|---|---|
| Tempur dasar | M–L |
| Listrik dan industri | L |
| Roket | L, karena butuh contraption |
| Tiap planet | M |
