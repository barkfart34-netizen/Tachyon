# Modpack Pembunuh Dewa: dokumen rancangan

Rancangan dari pengguna (30 Okt 2026, @Eko), disimpan di sini sebagai acuan cerita Tachyon. Bagian
terakhir, [Catatan penyesuaian Tachyon](#catatan-penyesuaian-tachyon), mencatat apa yang sudah berubah
sejak rancangan ditulis dan bagaimana rancangan ini masuk ke mod-mod Tachyon.

## Ringkasan konsep

Modpack ini adalah petualangan cerita di Horizon (Minecraft Java 26.3 yang ditulis ulang dengan Rust),
dibuat sebagai fork pribadi dan dimainkan bersama teman. Pemain membunuh tujuh dewa yang
menyeimbangkan semesta, lalu menghadapi penonton yang memanggilnya.

- **Platform:** fork Horizon, bukan modpack Java. Semua sistem ditulis langsung di kode game.
- **Gaya:** progres lewat boss berurutan. Teknologi, sihir, dan survival mendukung perjalanan itu.
- **Inti sistem:**
  - dua jenis sihir (mana dan aether);
  - kartu skill;
  - satu senjata untuk tiap dewa;
  - checkpoint ala waystone.
- **Pemain:** sekelompok teman. Boss diskalakan menurut jumlah pemain di arena.

## Lore dan asal usul

Pemain adalah makhluk dari dunia lain yang dipanggil oleh entitas misterius yang lebih tinggi dari
dewa mana pun. Entitas itu, The One Who Watches, membutuhkan penantang untuk membunuh dewa-dewa yang
sebenarnya menjaga keseimbangan semesta.

- **Memori palsu:** pemain diberi ingatan bahwa hidupnya menderita karena diatur oleh para dewa. Karena
  itu dia tidak mau dikendalikan siapa pun lagi. Ironinya: dia sedang dikendalikan oleh penonton.
- **Tontonan:** pembunuhan dewa adalah pertunjukan bagi The One Who Watches. Layar hologram, kartu
  skill, dan boss berurutan terasa seperti permainan karena memang begitu rancangannya.
- **Akibat membunuh dewa:** tiap dewa yang mati membuat dunia goyah (cuaca ekstrem, mob lebih kuat,
  wilayah rusak). Pemain punya alasan untuk ragu di tengah cerita.
- **Kebenaran terbuka bertahap:** setiap dewa yang mati membuka satu memori palsu. Di The Void,
  seluruhnya terbuka.
- **Panen aether:** pembunuhan dewa sebenarnya memanen aether. Tiap kali pemain menyerap aether,
  tubuhnya perlahan berubah menjadi susunan aether, karena penonton sedang membesarkan calon dewa
  berikutnya.

## Sistem sihir: mana dan aether

Dunia punya dua jenis sihir, dan hanya aether yang bisa melukai dewa. Aturan ini menjelaskan kenapa
dewa kebal terhadap serangan dan sihir biasa.

| | Mana | Aether |
|---|---|---|
| Menyusun | Sihir fisik, makhluk dunia | Dewa dan relik mereka |
| Dipakai untuk | Skill biasa, tongkat, kontrak hewan, mesin sihir | Melukai dewa, bangkit saat boss fight, evolusi skill |
| Sumber | Bar mana yang pulih sendiri | Sumber daya langka: relik, otoritas, serpihan aether |
| Melukai dewa | Tidak | Ya |

- **Tag "aether":** satu pengecekan di kode kerusakan.
  - Serangan dari senjata dewa dan unique skill berevolusi punya tag ini.
  - Relik Kraken tersusun dari aether, jadi senjata pertama yang bisa melukai dewa.
- **Unique skill:** dimulai dari mana. Setelah menjadi ultimate skill, ia mulai mengandung aether.
- **Harga aether:** makin banyak aether yang dipakai pemain, makin kuat kelemahan unique skill-nya.

## Alur cerita

Cerita berjalan dalam empat babak, dan tiap dewa yang mati membuka tahap berikutnya.

1. **Laut dan permukaan.**
   - Kraken, demigod pelayan dewa Laut, adalah boss pertama. Hadiahnya relik Hati Laut dan bahan untuk
     Create laut.
   - Ruin dan dungeon pertama muncul di sini.
   - Pemain belajar backpack, masak, hidrasi, dan waystone.
2. **Dungeon dan sihir.**
   - Dungeon Ender Dragon membuka sihir tingkat lanjut dan dunia sihir/isekai.
   - Kontrak darah dengan naga dan hewan fiktif mulai bisa dipakai.
3. **Dimensi rusak dan teknologi.** Satu dimensi rusak (terinspirasi dimensi Sift, dengan nama buatan
   sendiri) dan pohon teknologi tinggi dari Create, Tinkers, dan unsur StaTech.
4. **Angkasa sampai The Void.** Dari planet terdekat ke ujung semesta, melewati batas, lalu tiba di The
   Void untuk melawan The One Who Watches.

Tiga akhir, dipilih setelah penonton kalah:

- Tetap menjadi tontonan.
- Melawan penonton.
- Menggantikan posisinya. Pilihan ini berarti menerima aether sepenuhnya.

**Langit sebagai penanda progres:** tiap dewa yang mati menyalakan satu bintang, dan ketujuhnya
membentuk konstelasi baru. Bintang kedelapan tetap padam. Itu The One Who Watches. Di The Void bintang
itu menyala dan menatap balik pemain.

## Para dewa dan senjatanya

Ada tujuh dewa, masing-masing berakar dari satu mitologi dan satu konstelasi, dengan senjata yang
mencerminkannya. Nama dan wujud akhirnya bisa dibuat orisinal; tabel ini baru usulan.

| Dewa | Akar mitologi | Konstelasi | Senjata | Kemampuan khas | Jenis dasar |
|---|---|---|---|---|---|
| Laut | Poseidon, Ægir | Delphinus | Trisula | Menarik musuh dengan arus, bisa dilempar | Jarak dekat |
| Darah | Ereshkigal, Moloch | Scorpius | Sabit | Menyerap darah, terhubung ke kontrak hewan | Jarak dekat |
| Sihir | Hecate | Lyra | Tongkat | Merapal skill memakai mana | Perapal |
| Ruang | Ouranos, Janus | Ara | Busur | Berteleportasi ke titik jatuh panah | Jarak jauh |
| Besi | Hephaestus, Tubal-Cain | Fornax | Palu | Menghancurkan, mengisi mesin | Jarak dekat |
| Bintang | Orion, Astraeus | Orion | Meriam tangan | Tembakan jauh berdaya besar | Jarak jauh |
| Waktu | Kronos, Zurvan | Horologium | Belati | Memperlambat musuh | Jarak dekat |

### Aturan koleksi dan urutan

- **Penempaan:** tiap dewa menjatuhkan relik dan otoritas. Keduanya ditempa menjadi satu senjata unik
  di tempat tempa khusus (gabungan Create, Tinkers, dan sihir).
- **Senjata sebagai kunci:** senjata dewa sebelumnya jadi kunci untuk dewa berikutnya, karena kelemahan
  dewa berikutnya cocok dengan kemampuan senjata sebelumnya. Contoh: dewa Darah menyembuhkan diri lewat
  air, jadi kendali air milik trisula yang menghentikannya.
- **Aula koleksi:** di basis pemain ada satu podium per senjata. Senjata yang dipajang memberi bonus
  kecil permanen.
- **Batas bawaan:** pemain hanya membawa satu atau dua senjata dewa sekaligus, supaya pilihan sesuai
  tantangan tetap penting.
- **Setelah cerita selesai:** senjata tetap bisa dipakai, tetapi sifat aether-nya melemah terhadap
  makhluk biasa.
- **Urutan akhir:** dewa Waktu dan Bintang adalah dewa terakhir karena paling dekat dengan batas
  semesta.

## Boss pertama: Kraken

Kraken adalah demigod yang mengajarkan pelajaran pertama tentang boss fight. Dia penjaga gerbang dewa
Laut yang terkurung di palung dasar laut, setia pada tuannya dan tidak jahat. Itu memberi nada awal:
dewa-dewa punya pengikut.

**Arena:**
- reruntuhan kuil di dasar laut dengan kubah udara, jadi pemain tidak bertarung sambil kehabisan napas;
- waystone di pintu arena;
- bagian luar kuil terbuka ke air dan dipakai di fase 2.

### Tiga fase

1. **Tentakel (HP 100% sampai 66%):**
   - Kraken muncul sebagai empat tentakel dari lantai dan dinding.
   - Tentakel menghantam dan menarik pemain ke tepi air.
   - Menghancurkan keempatnya membuka fase berikutnya.
2. **Pusaran (66% sampai 33%):**
   - Badan utama muncul dan arena mulai terisi air.
   - Pemain berpindah antar platform sambil menghadapi tinta yang mengaburkan layar dan arus penarik.
   - Mesin Create laut yang dipasang sebelumnya bisa menguras sebagian air dan memberi pijakan.
3. **Putus asa (di bawah 33%):**
   - Serangan area besar, dan tentakel yang hancur tumbuh kembali dengan lambat.
   - Pemain harus fokus ke titik lemah di mata, yang terbuka hanya sesaat setelah dia menyerang.

**Skala:** HP naik per pemain di arena, dan jumlah tentakel di fase 1 bertambah saat pemain lebih dari
tiga.

**Hadiah:**
- relik Hati Laut (bahan inti trisula dewa Laut);
- sebagian bahan Create laut;
- bintang pertama di langit menyala.

**Catatan pembangunan:**
- Buat tentakel sebagai entitas terpisah yang terhubung ke Kraken.
- Perubahan fase cukup mengganti perilaku, bukan membuat entitas baru.
- Kalau perlu memangkas pekerjaan, kurangi bagian air fase 2 dulu.

## Boss akhir: The One Who Watches

Boss akhir berdiri di luar tujuh dewa dan bersumber dari satu ide: kekuatannya adalah diamati. Dia
tidak punya tubuh, hanya satu mata raksasa yang menatap dari langit. Bintang kedelapan yang padam
adalah matanya.

Selama cerita berjalan, namanya tidak disebut. Dunia memakai bisikan atau julukan seperti "sang
penonton", dan nama aslinya baru terungkap di The Void.

### Tiga fase

1. **Yang ditatap tidak bisa dilukai:** pemain harus keluar dari pandangannya dan memakai arena untuk
   bersembunyi. Dia menoleh setiap kali pemain bangkit memakai aether.
2. **Penghapusan:** dia "menghapus" apa pun yang tidak lagi dia amati, seperti platform, senjata, dan
   skill. Pemain harus membuatnya menatap tempat lain.
3. **Pemain berbalik menatap:** seluruh aether yang terkumpul jadi satu-satunya senjata. Yang mati
   menjadi penonton, sesuai cerita.

### Aturan di The Void

- Waystone tidak bisa dipakai, jadi tidak ada checkpoint.
- Aturan penonton tetap berlaku: yang mati menonton sampai boss kalah atau semua mati.
- Hologram skill mulai rusak (glitch), dan memori palsu pemain terbuka sepenuhnya.
- Semua senjata dewa di koleksi bereaksi, dan senjata yang dipegang pemain mulai retak karena
  sebenarnya pemberian si penonton.

**Catatan pembangunan:**
- Efek "arah pandang" dan "tidak bisa dilukai saat diamati" cukup berupa pengecekan garis pandang,
  tanpa model rumit.
- The Void sendiri murah dibuat: dimensi kosong dengan beberapa pulau melayang.

## Sistem skill

Pemain mulai lewat layar hologram bergaya RPG yang menampilkan tiga kartu. Satu sistem ini menyatukan
skill dan sihir, supaya pekerjaannya tidak dua kali lipat. Terinspirasi anime Tensura.

- **Kartu skill:** tiga kartu untuk skill, lalu tiga kartu untuk unique skill.
- **Risiko unique skill:** 75% sampai 80% kemungkinan mendapat kelemahan bawaan. Itu membuat tiap pemain
  berbeda, dan teman-teman jadi saling melengkapi.
- **Kelemahan:** sebaiknya sebagian terlihat sebelum dipilih. Kalau sepenuhnya tersembunyi, pemain
  merasa dicurangi. Kalau sepenuhnya terlihat, risikonya hilang.
- **Evolusi:** unique skill berevolusi menjadi ultimate skill. Tiap dewa yang mati meninggalkan satu
  otoritas sebagai bahan evolusi. Mengatasi kelemahan bisa menjadi syarat tambahan.
- **Kontrak darah:** salah satu unique skill, bukan sistem terpisah.
  - Hewan peliharaan, termasuk naga dan hewan fiktif, punya tingkat ikatan yang naik lewat pertarungan
    dan perawatan.
  - Naga adalah tingkat tertinggi dan bisa ditunggangi.

**Ukuran awal yang realistis:** sekitar 5 skill, 3 unique skill, dan satu jalur evolusi yang berfungsi
penuh. Kartu ditambah setelah sistemnya jalan.

## Checkpoint, kematian, dan boss fight

Kematian biasa kembali ke waystone terakhir. Hanya saat melawan boss kelas dewa, pemain bisa memakai
aether untuk bangkit lagi. Dalam cerita ini disebut "memutar waktu". Aslinya, The One Who Watches yang
mengulang adegan karena penonton ingin pertunjukan berlanjut.

### Waystone (checkpoint)

- Ditemukan lewat dungeon dan ruin. Mengaksesnya menandainya sebagai ditemukan, lalu bisa dipakai
  sebagai titik respawn.
- Tiap boss besar punya waystone di pintu arenanya.

### Aturan saat boss kelas dewa

1. Pemain yang mati dan masih punya muatan aether respawn di waystone arena, lalu bisa masuk lagi.
   Muatan berkurang satu. Hanya pemain yang mati yang kembali; boss dan pemain lain tidak diputar balik.
2. Pemain yang muatannya habis menjadi penonton (spectator). Mereka tidak bisa respawn sampai boss kalah
   atau semua pemain mati.
3. Jika semua pemain mati, boss kembali ke kondisi awal dan semua pemain respawn di waystone arena.
4. Di The Void, waystone tidak bisa dipakai.

### Isi ulang muatan (usulan, belum final)

- Muatan diisi ulang memakai Serpihan Aether, item langka dari dungeon, ruin, dan boss.
- Serpihan dimasukkan ke altar di waystone arena. Satu serpihan mengisi satu muatan.
- Kapasitas muatan dibatasi (usulan: 2 sampai 3). Mengisi ulang hanya bisa di waystone arena, bukan di
  tengah pertarungan.

**Harga bangkit:** tiap kali bangkit, pemain menyerap lebih banyak aether, sehingga kelemahan unique
skill makin kuat dan tubuhnya makin dekat menjadi aether. Itu memengaruhi pilihan akhir.

**Tampilan:** layar memutar mundur beberapa detik sebelum pemain muncul di waystone, dan hologram
menampilkan teks seperti "Adegan diulang" beserta sisa muatan. Keduanya hanya efek tampilan.

## Daftar fitur dan mod

Tujuh belas ide awal dikelompokkan menurut perannya. Kolom prioritas adalah usulan:

- **Inti:** harus ada agar ceritanya berjalan.
- **Menengah:** menguatkan pengalaman.
- **Bonus:** ditambah bila waktu cukup.

| # | Ide | Peran dalam cerita | Prioritas |
|---|---|---|---|
| 17 | Backpack | Penyimpanan inti, mulai dari sini | Inti |
| 10 | Checkpoint (waystone) | Titik respawn, kunci aturan boss | Inti |
| 1 | Kraken (demigod) | Boss pertama, relik Hati Laut | Inti |
| 9 | Boss dewa non-material | Tujuh dewa dan boss akhir | Inti |
| 3, 14 | Sihir dan kekuatan magis | Satu sistem skill dengan mana dan aether | Inti |
| 6 | Ruin dengan dungeon | Pintu masuk dungeon dan sumber serpihan aether | Inti |
| 5 | Create laut | Mesin bawah laut, hadiah Kraken | Menengah |
| 8, 13 | Tinkers Construct dan unsur StaTech | Satu pohon teknologi: bahan, mesin, peralatan | Menengah |
| 4 | Dungeon Ender Dragon | Dungeon tingkat tinggi, membuka sihir lanjut | Menengah |
| 15 | Dunia sihir/isekai dan naga | Dunia kedua dengan monster besar | Menengah |
| 16 | Kontrak darah ke hewan peliharaan | Salah satu unique skill | Menengah |
| 2 | Area angkasa | Babak akhir, dari planet ke ujung semesta | Menengah |
| 7 | Dimensi ala Sift | Dimensi rusak, memakai nama dan aset buatan sendiri | Bonus |
| 11 | Masak detail | Survival | Bonus |
| 12 | Tingkat hidrasi | Survival | Bonus |

**Ide tambahan yang sudah masuk rancangan:**
- segel dewa (tiap dewa menyimpan satu);
- relik demigod (hadiah Kraken dan boss lain);
- bintang yang menyala di langit;
- aula koleksi senjata;
- Serpihan Aether.

**Referensi desain, bukan mod yang dipakai:**
- Waystones (checkpoint);
- StaTech Industry (progres teknologi bertingkat berbasis quest);
- dimensi Sift (kerangka dimensi rusak).

## Catatan penyesuaian Tachyon

Ditambahkan 31 Okt 2026 saat rancangan ini diterima.

- **Fork dan build sudah ada.** Tachyon adalah fork Horizon yang dibuild dan diuji lewat CI GitHub,
  karena laptop tidak cukup RAM dan disk untuk build lokal. Langkah "build dan jalankan fork dari
  komputer" di rancangan asli tidak diperlukan lagi.
- **Create sekarang berjalan.** Create asli diunduh dari Modrinth di perangkat pemain dan mesinnya
  digerakkan oleh mesin Kinetica. Karena Tachyon pribadi, kode Create (MIT) boleh dibaca dan diporting
  ke Rust dengan pemberitahuan lisensinya. Aset Create (All Rights Reserved) tidak pernah masuk repo
  atau rilis.
- **Tinkers Construct** bisa diperlakukan sama, setelah lisensinya diperiksa.
- **Pemetaan ke mod Tachyon:**
  - Kinetica + Create: teknologi, termasuk Create laut;
  - Arcana: mana, aether, kartu skill;
  - Chronicles: alur cerita, quest, memori palsu;
  - Lost Realms: ruin, dungeon, waystone, arena;
  - Create Aeronautics: kapal laut, kapal udara, sampai angkasa.
- **Pekerjaan teknis terbesar:** entitas baru (boss multi-bagian seperti Kraken, naga yang ditunggangi).
  Mod konten Horizon belum bisa menambah entitas, jadi mesin Tachyon perlu sistem entitasnya sendiri
  dulu.
- **Lokasi kode:** jalur kode di rancangan asli (misalnya `G/dragon.rs`, `L/menu.rs`) belum dicocokkan
  dengan repo. Jalur itu diperiksa ulang saat tiap fitur dikerjakan.
