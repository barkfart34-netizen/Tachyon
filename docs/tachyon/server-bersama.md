# Server bersama: dunia yang selalu ada, di-host oleh pemain

Rancangan untuk satu grup kecil, misalnya 3 orang dengan 1 HP dan 2 PC atau laptop. Tujuannya:
- siapa pun bisa main kapan pun, termasuk sendirian;
- bisa main dari mana pun, di LAN yang sama ataupun lewat internet;
- tetap lancar.

Rencana besarnya ada di [rencana-modpack.md](rencana-modpack.md).

## Ide pokok

Dunia tinggal di **server murah**, tetapi server itu **tidak menjalankan game**. Yang menjalankan
dunia adalah perangkat salah satu pemain yang sedang online. Pemain itu disebut **host**.

| Bagian | Dikerjakan oleh | Beban |
|---|---|---|
| Simulasi dunia: mesin Create, mob, redstone, tick | Host, yaitu perangkat terbaik yang sedang online | Berat |
| Penyimpanan dunia, cadangan harian, kunci host | Server murah | Ringan |
| Mempertemukan pemain dengan host | Server murah | Sangat ringan |
| Relay, bila pemain tidak bisa tersambung langsung | Server murah | Sedang: hanya meneruskan paket |
| Membuat chunk baru dan pencahayaan (opsional, tahap 5) | PC kedua yang sedang online | Berat, tetapi tidak harus cepat |

Server lambat tidak membuat game lag, karena tidak ada satu tick pun yang menunggu server.

**Kenapa dunia tidak dibagi per wilayah ke banyak perangkat:**
- Untuk 3 orang, satu PC mampu menjalankan seluruh dunia dengan mudah.
- Membagi wilayah justru menambah lag dan bug di batasnya: mob yang menyeberang, jaringan Create
  yang membentang dua wilayah, dan perangkat yang tiba-tiba hilang.

Gagasan "pembagian sesuai kemampuan perangkat" tetap dipakai dalam dua bentuk:
- memilih host dari skor perangkat;
- di tahap 5, menitipkan pekerjaan yang tidak butuh cepat ke PC kedua.

## Siapa yang menjadi host

Setiap perangkat menghitung **skor host** saat masuk.

| Faktor | Contoh bobot |
|---|---|
| Jenis perangkat | PC tersambung listrik 100, laptop pakai baterai 60, HP 20 |
| CPU | Hasil benchmark singkat saat pertama kali dijalankan, disimpan |
| RAM bebas | Di bawah 3 GB bebas: tidak boleh menjadi host bila ada perangkat lain |
| Jaringan | Kabel atau Wi-Fi di atas data seluler; upload terukur; bisa tersambung langsung atau tidak |
| Kondisi | HP yang panas, baterainya di bawah 30%, atau dalam mode hemat daya: turun jauh |

**Aturan memilih host:**
- Host yang sedang berjalan tetap host, kecuali ada perangkat lain dengan skor **jauh lebih
  tinggi**, misalnya 1,5× lipat. Ini mencegah host bolak-balik pindah.
- HP hanya menjadi host bila sedang sendirian.
  - Saat HP dijeda Android (pindah aplikasi, layar mati), tidak ada orang lain yang ikut terganggu.
  - Dunia tetap tersimpan, sama seperti singleplayer.

| Yang online | Host | Rasanya |
|---|---|---|
| PC A, PC B, HP | PC dengan skor tertinggi | PC lain dan HP tersambung ke PC itu |
| Satu PC dan HP | PC | HP tersambung ke PC |
| Hanya HP | HP | Seperti singleplayer, lalu disinkronkan ke server |
| Hanya satu PC | PC itu | Seperti singleplayer |

## Kunci host (lease): mencegah dua dunia bercabang

Supaya tidak pernah ada dua host sekaligus, server memberikan **lease**:
- Lease hanya untuk satu perangkat, dengan nomor urut yang terus naik.
- Host memperbarui lease tiap 5 detik. Lease habis 20 detik setelah pembaruan terakhir.
- Simpanan dari host dengan nomor lease lama ditolak server. Jadi PC yang sempat putus lalu
  tersambung lagi tidak bisa menimpa dunia.

## Sinkronisasi dunia

**Salinan di perangkat:**
- Setiap perangkat menyimpan salinan dunia di dirinya sendiri, jadi masuk dunia terasa cepat.
- Server menyimpan **versi per chunk**, yaitu nomor yang naik setiap chunk berubah, plus versi
  file data pemain dan file mesin Tachyon.
- Saat masuk, perangkat hanya mengunduh chunk yang versinya lebih baru dari salinannya.

**Saat menjadi host:**
- Host mengirim chunk yang berubah tiap 30 detik, terkompresi, dan di belakang layar.
- Host juga langsung mengirim saat:
  - pemain keluar;
  - host berpindah;
  - aplikasi HP akan dijeda (`onPause`).

**Keamanan data:**
- Server menyimpan cadangan harian 7 hari terakhir.
- Berguna kalau ada yang rusak, termasuk karena bug.

## Perpindahan host

**Pindah terencana**, misalnya HP sedang menjadi host lalu PC masuk:
1. Server memberi tahu host: "serahkan ke PC".
2. Host menyimpan semua chunk yang berubah dan menyerahkan lease.
3. PC memuat dunia dari salinannya sendiri plus selisih terbaru, lalu mulai menjalankan dunia.
4. HP tersambung kembali sebagai pemain.

Semua orang melihat layar "berpindah host" sekitar 3–10 detik. Tidak ada yang hilang.

**Pindah mendadak**, misalnya PC host mati listrik:
- Lease habis setelah 20 detik.
- Perangkat dengan skor tertinggi berikutnya mengambil alih dari simpanan terakhir.
- Paling banyak 30 detik permainan terakhir hilang.
- Mesin Create melanjutkan dari file mesinnya.

## Sambungan: urutan yang dicoba

1. **LAN yang sama.** Sama seperti "Open to LAN" sekarang. Lag hampir nol.
2. **Langsung lewat internet.**
   - Lewat IPv6, yang sudah dipakai banyak operator seluler dan ISP rumah di Indonesia.
   - Bila tidak bisa, lewat UDP hole punching yang dikoordinasi server.
3. **Relay di server**, bila keduanya gagal (misalnya CGNAT di kedua sisi).
   - Server dipilih di dekat pemain, misalnya Singapura atau Jakarta.
   - Tambahan lag biasanya 10–30 ms.

Protokol permainannya tetap protokol LAN yang sudah ada. Lapisan baru hanya ada di bawahnya:
siapa tersambung ke siapa, dan lewat jalur mana.

## Perkiraan rasa main untuk 3 orang (1 HP, 2 PC)

| Keadaan | Host | Ping ke host | Rasanya |
|---|---|---|---|
| Ketiganya serumah | PC terbaik | < 5 ms | Seperti LAN |
| Beda tempat, tersambung langsung | PC terbaik | 20–60 ms | Seperti server Minecraft lokal yang bagus |
| Beda tempat, lewat relay | PC terbaik | 40–90 ms | Seperti server Minecraft Asia |
| Sendirian (PC atau HP) | Dirinya sendiri | 0 | Seperti singleplayer |

Yang tetap bisa membuat lag:
- PC host yang sedang berat dipakai untuk hal lain;
- koneksi upload host yang buruk.

Kedua hal ini ikut masuk ke skor, jadi host bisa pindah otomatis.

## Server murah

**Kebutuhannya kecil:**
- 1 vCPU dan 1 GB RAM;
- penyimpanan beberapa GB untuk dunia dan cadangan;
- yang perlu diperhatikan adalah **kuota bandwidth bulanan** bila relay sering dipakai.

| Kebutuhan | Perkiraan |
|---|---|
| Dunia Create yang sudah dijelajahi | 100 MB – 1 GB |
| Sinkronisasi selisih | Beberapa MB per jam bermain |
| Relay per pemain | Sekitar 30–200 KB/detik saat bermain, lebih saat memuat chunk |

**Pilihan tempat:**
- VPS kecil di Singapura atau Jakarta;
- tingkat gratis penyedia cloud, selama masih ada.

**Isi servernya:**
- Satu program kecil berbahasa Rust di workspace Tachyon. Bukan server Minecraft.
- Isinya:
  - API lease;
  - penyimpanan chunk berversi;
  - koordinator hole punching;
  - relay.

**Akses:**
- Hanya perangkat yang terdaftar yang boleh masuk.
- Setiap perangkat punya pasangan kunci sendiri.
- Pemilik grup menambahkan kunci publik perangkat ke daftar server.

**Versi:**
- Server menolak perangkat yang versi Tachyon atau Create-nya berbeda dari dunia.
- Aset Create tetap diambil di perangkat masing-masing, tidak pernah lewat server.

## Yang sengaja tidak dibuat

- **Main offline lalu digabung.** Dua orang yang main offline di dunia yang sama akan membuat dua
  cabang yang tidak bisa digabung dengan aman.
  - Tanpa internet, pilihannya hanya membuka **salinan** dunia sebagai dunia terpisah.
- **Dunia dibagi per wilayah ke beberapa host** (lihat alasannya di atas).
- **Server publik untuk orang asing.** Host bisa saja curang, dan untuk grup teman itu wajar.

## Tahapan

| Tahap | Isi | Hasil yang bisa dicoba | Ukuran |
|---|---|---|---|
| S1 | Server: penyimpanan chunk berversi, lease, akun perangkat. Klien: sinkronisasi dunia. | Satu orang main kapan pun dari perangkat mana pun; dunianya sama | M |
| S2 | Bergabung ke host lewat LAN, lalu langsung (IPv6 atau hole punching), lalu relay | Bertiga main dari tempat berbeda | M |
| S3 | Skor host, pemilihan host, pindah terencana | HP menyerahkan dunia ke PC yang masuk | M |
| S4 | Pindah mendadak, cadangan harian, pemulihan | PC host mati, game lanjut di perangkat lain | M |
| S5 (opsional) | Pembuatan chunk dan pencahayaan dititipkan ke PC kedua | Menjelajah lebih ringan bagi host | L |

**Waktu dan urutan:**
- S1–S4 kira-kira 4–8 minggu kerja, dengan GPT menulis sebagian besar kodenya.
- Dikerjakan setelah Create bisa dimainkan dan kontrol Android ala Bedrock (Tahap A) selesai.

**Pengujian:**
- Logika lease, versi chunk, dan pemilihan host diuji dengan unit test.
- Sambungan diuji di emulator dan PC yang ada.

## Pertanyaan yang masih terbuka

- Apakah Horizon bisa menyimpan dan memuat chunk satu per satu tanpa memuat ulang dunia?
  - Ini menentukan cara sinkronisasi S1.
  - Perlu dicek di kode penyimpanan dunia.
- Seberapa sering Android membunuh proses di latar belakang pada HP pemain, dan apakah `onPause`
  cukup waktu untuk menyimpan.
- Penyedia server mana yang dipakai. Diputuskan pemilik grup saat S1 dimulai.
