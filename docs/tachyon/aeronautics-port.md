# Port Create Aeronautics (`simulated` + `aeronautics` + `offroad`) — Handoff & Progress Ledger

> **Untuk agen AI berikutnya:** Baca dokumen ini lebih dulu sebelum memindai ulang kode. Seluruh status progres, peta file, bug yang sudah terverifikasi, aset/kode Create yang bisa dipakai ulang, dan tahapan kerja terurut dicatat secara jujur di sini.

- **Tanggal pembaruan:** 28 November 2026
- **Commit baseline:** `586c9dda2a` (cabang `tachyon` → `tachyon/main`)
- **Target mod referensi:** `create-aeronautics-bundled-1.21.1-1.3.2.jar` (berbasis **Create 6.0.10** + **Sable 2.x** `dev.ryanhcode.sable`, total **905 kelas Java top-level**: `simulated` 634, `aeronautics` 202, `offroad` 69)
- **Progres fungsional/fisika/visual keseluruhan (estimasi jujur):** **~24%**
- **Dokumen pendamping:** [rencana-modpack.md](rencana-modpack.md) · [create-port.md](create-port.md) · [tce.md](tce.md)

---

## 1. Status Singkat: Apa yang Sudah Jalan vs Belum

### Sudah berjalan per commit `586c9dda2a`
- **Unduh & staging aset runtime (tanpa aset ARR di repo):** `tachyon/fetch.rs` mengunduh JAR bundel Create Aeronautics 1.3.2 dan `tachyon/aeronautics_port.rs` (`VERSION = "4"`) mengekstrak `aeronautics`, `simulated`, dan `offroad` secara bersih (strip namespace `create` lintas-mod & direktori nested `horizon/` supaya `Physics Assembler`, `Honey Glue`, `Steering Wheel`, dll. termuat tanpa bentrok dengan `mod-create`), merestorasi JSON `neoforge:obj`, serta menormalkan cuboid model yang terbalik/di luar rentang.
- **Tab Kreatif tunggal:** Semua blok dan item `aeronautics`, `simulated`, dan `offroad` tergabung dalam 1 tab kreatif `"Create Aeronautics"` (`"aeronautics:tachyon_tab"`), tetap dapat dicari per namespace.
- **Perakitan & pembongkaran dasar (prototype):** `/aero assemble`, `/aero land`, dan klik kanan `simulated:physics_assembler` / `simulated:steering_wheel` membentuk `RigidContraption` (6-DOF pose + kuaternion) dan membongkarnya kembali ke grid dunia 90° (`snap_disassembly`). Deadlock mutex `/aero land` sudah diperbaiki (`4b58c21930`).
- **Render model blok asli 6-DOF:** `Game::aeronautics_vessel_geometry` (`crates/horizon-client/src/game.rs:2185`) menggambar setiap blok kontrapsi memakai mesh model blok/item asli (`block_model_vertices` + ban `offroad:*_tire` lewat `resolve_item` / `submit_item_world`), menggantikan kotak warna placeholder.
- **Kamera bebas & kursi dasar:** Spam `entity.teleport_to` tiap tick di server sudah dihapus sehingga kamera pilot tidak lagi terkunci; `BlockRole::Seat` mengenali `create:*_seat` & `simulated:*_seat`; merakit tidak otomatis mengunci pemain; `Shift` turun dari pilot (`Ctrl`/`C` turun ketinggian); HUD pilot berupa strip tipis berbingkai brass di atas hotbar (hanya tampil saat mengemudi).

### Belum berjalan / masih jauh dari standar Create Aeronautics asli
- **Collision & fisika rigid-body masih sangat mentah (~2/10):** tidak ada OBB collision ke sisi/bawah/atas untuk pemain atau mob/item, benturan ke dunia cuma 1 titik bawah per blok tanpa impuls/torsi, ada bug transformasi koordinat pada raycast roda (`wheels.rs:56`), dan belum ada collision antar-kendaraan.
- **Honey Glue tidak benar-benar dipakai saat merakit & 0% visual:** `glued_boxes` disimpan lalu dihapus tanpa pernah dibaca `assemble_from_world`; tidak ada box outline hijau/kuning, partikel madu, suara, resize Ctrl+Scroll, atau penghapusan lem.
- **Tidak ada animasi bagian & efek:** baling-baling tidak berputar, roda tidak berputar/belok/naik-turun sesuai suspensi, burner tidak mengeluarkan api/asap/suara.
- **Belum terhubung ke sistem Kinetik Create onboard:** baling-baling dan wheel mount di versi asli digerakkan **RPM Kinetik Create** + **sinyal redstone** (bukan kontrol arcade WASD langsung), dan pemain belum bisa klik-kanan/interaksi blok di atas kendaraan yang sedang aktif.
- **GUI kustom & blok lanjutan `simulated`/`aeronautics`/`offroad` belum diporting** (`DiagramScreen`, `LinkedTypewriterScreen`, `AltitudeSensorScreen`, `NameplateScreen`, `Borehead`, `SwivelBearing`, `Spring`, `RopeWinch`, `DockingConnector`, `Plunger`, `MountedPotatoCannon`, `BalloonLayerGraph` 3D).

---

## 2. Peta File Cepat (Zero-Scan Map)

### A. Crate Fisika Pure-Rust — `crates/aeronautics-physics/src/`
| File | Peran | Catatan penting |
|---|---|---|
| `lib.rs` | Re-export publik (`BlockRole`, `RigidContraption`, `assemble_from_world`, dll.) + 10 unit test | Lulus `10/10` pada baseline `586c9dda2a` |
| `catalog.rs` | Klasifikasi ID blok (`BlockRole`: `Hull`, `Seat`, `Helm`, `Assembler`, `Propeller`, `Balloon`, `Burner`, `WheelMount`, `Wing`, `Rudder`, `Buoyant`, dll.), `TireKind`, `is_unmovable` | Sudah mengenali `create:*_seat` & `simulated:*_seat` |
| `mass.rs` | `BlockCell`, `MassProperties` (massa total, `center_of_mass`, tensor inersia diagonal) | — |
| `math.rs` | `Vec3`, `Quat`, `Pos`, `Facing`, `quat_rotate`, `quat_rotate_inv`, `quat_slerp`, Euler | — |
| `assembly.rs` | `assemble_from_world` (BFS flood-fill 6-arah, baris 32–108) | **BUG #4:** mengabaikan zona Honey Glue / Super Glue & Chassis |
| `body.rs` | `RigidContraption::step` (integrasi 6-DOF), `resolve_world_collision` (baris 190–252), `snap_disassembly` (baris 256+) | **BUG #3:** collision dunia hanya 1 titik bawah per blok, tanpa impuls/torsi |
| `aero.rs` | `PilotInput`, gaya dorong baling-baling, lift sayap, drag, kontrol arcade | Masih dikendalikan `PilotInput` langsung, belum dari RPM Kinetik |
| `buoyancy.rs` | Gaya apung air & panas balon udara sederhana | Belum memakai 3D `BalloonLayerGraph` & `LiftingGasType` |
| `wheels.rs` | Raycast suspensi roda, traksi, grip lateral, steering Ackerman | **BUG #1 (baris 56):** salah hitung `hub_world` saat orientasi berotasi |
| `sensors.rs` | Pembacaan altimeter/speedometer/compass dasar | Belum sinkron dengan output redstone blok onboard |

### B. Mod Native Horizon — `crates/mod-aeronautics/src/`
| File | Peran | Catatan penting |
|---|---|---|
| `lib.rs` | Registrasi hook mod native (`ID`, `BUILT`, `hooks()`), input keyboard pilot (`Shift` dismount, `Ctrl`/`C` descend, `W/A/S/D/R/F/Q/E/B/[/]`) | 4 unit test (`mod-aeronautics`: `4/4`) |
| `net.rs` | Protokol sinkronisasi `VesselSnapshot`, `RenderBlock` (termasuk `block_id: String`), `role_from_code` | Payload `RenderBlock` sudah membawa `block_id` penuh |
| `server.rs` | State server per dimensi (`vessels`, `glue_first_corner`, `glued_boxes`), command `/aero`, interaksi klik kanan `physics_assembler`/`steering_wheel`/`honey_glue`/`contraption_diagram`, tick server | Deadlock `/aero land` sudah fix (`find_land_vessel`); `glued_boxes` belum diteruskan ke assembler |
| `solid.rs` | Registry `VESSELS` klien, interpolasi pose (`render_vessels`), penempatan kursi pilot (`helm_local_pos`), dan `local_player_pass` (baris 199–275) | **BUG #2:** hanya cek strip atas dek untuk pemain lokal, belum OBB 3D penuh & belum ada pass server untuk mob/item |
| `render.rs` | `render_gui_post` (strip HUD pilot berbingkai brass di atas hotbar) | Render blok 3D sudah dipindah ke `horizon-client/src/game.rs` |

### C. Titik Kait (Hooks) di `crates/horizon-client/src/`
| File & Baris | Peran |
|---|---|
| `tachyon/fetch.rs` | Sumber unduhan JAR `AERONAUTICS` (tanpa mengambil `data/create/` lintas-namespace) |
| `tachyon/aeronautics_port.rs` | `VERSION = "4"`, `refresh_all`, `fix`, `restore_recipes`, `strip_foreign_namespaces`, `remove_nested_horizon_dirs`, normalisasi cuboid & model OBJ (`5/5` unit test) |
| `tachyon/import.rs:138-140` | Memanggil `aeronautics_port::fix` & `restore_recipes` saat impor mod |
| `creative.rs` | Menggabungkan `aeronautics`, `simulated`, `offroad` ke tab `"Create Aeronautics"` (`"aeronautics:tachyon_tab"`) |
| `tachyon_native_mods.rs:14,17,112` | Pendaftaran `mod_aeronautics` bersama `horizon_quests` dan `mod_create` |
| `game.rs:2185-2315` & `:5575` | `Game::aeronautics_vessel_geometry` — merender semua blok & ban kapal secara 6-DOF dengan tekstur asli |
| `game/network/remote/mod.rs:7540-7573` | `mod_aeronautics_local_player_solid()` — memanggil `mod_aeronautics::solid::local_player_pass` |
| `desktop/mod.rs:130`, `android/mod.rs:105`, `android/install.rs:155` | Pemanggilan `aeronautics_port::refresh_all` di Desktop & Android |

### D. KODE CREATE YANG SUDAH ADA & WAJIB DIPAKAI ULANG (JANGAN TULIS ULANG!)
`mod-create` di repo ini sudah memiliki implementasi collision OBB, solid contraption, super glue visual, seat, dan ValueBox yang matang. **Gunakan ulang atau adaptasi langsung modul-modul ini untuk Tahap 1–2:**
- **Collision OBB & Entity Carry (Create asli dalam Rust):**
  - `crates/mod-create/src/kinetics/src/rules/collider_obb.rs` — `Aabb`, `ContinuousOBBCollider::collide_many`, `CollisionList::from_blocks`
  - `crates/mod-create/src/kinetics/src/rules/collider.rs` — `collide_entities`, `save_client_player_from_clipping` (`safetyLock`)
  - `crates/mod-create/src/kinetics/src/rules/solid.rs` — `Solid`, `SolidBlock`, bentuk tumbukan blok
  - `crates/mod-create/src/solid/{mod.rs,client.rs,server.rs,shapes.rs,registry.rs}` — pola arsitektur lengkap client local-player pass + server entity pass + ekstrak `LevelRef::block_shape`
- **Visual Lem & Seleksi Area (Super Glue → adaptasi untuk Honey Glue):**
  - `crates/mod-create/src/kinetics/src/rules/super_glue.rs` — aturan konektivitas kotak lem & verifikasi keterhubungan
  - `crates/mod-create/src/machines/super_glue.rs` — render outline kotak 3D, partikel, interaksi klik/Ctrl+Scroll/hapus lem
- **Kursi & ValueBox Sprite Asli Create:**
  - `crates/mod-create/src/kinetics/src/rules/seat.rs` & `crates/mod-create/src/machines/seat.rs` — duduk di kursi & keluar dengan Shift
  - `crates/mod-create/src/machines/{value_box.rs,value_box_draw.rs,value_box_screen.rs}` — panel ValueBox dengan sprite asli `create:gui/value_settings` & `create:gui/icons` (berguna untuk `WheelMount` suspension, `SteeringWheel` max angle, dll.)

---

## 3. Tabel Progres Porting: Create Aeronautics 1.3.2 Asli vs Port Saat Ini

| # | Subsistem | Referensi Asli (`1.3.2.jar` — Sable 2.x + Create 6.0.10) | Kondisi Port Rust Saat Ini (`586c9dda2a`) | Progres | Prioritas |
|---|---|---|---|---:|---|
| 1 | Asset Staging, Model & Tab Kreatif | 3 mod (`simulated`, `aeronautics`, `offroad`), model JSON + `neoforge:obj`, tab kreatif | Staging v4 bebas bentrok namespace `create`, OBJ & cuboid fix, 1 tab `"Create Aeronautics"` | **85%** | Selesai (rawat minor) |
| 2 | Render Blok Kapal 6-DOF | Sable SubLevel world renderer (pencahayaan dinamis, blok entity, animasi) | Render model blok asli + ban `offroad:*_tire` dengan interpolasi pose (`game.rs:2185`), namun statis (tanpa animasi/lightmap dinamis) | **45%** | Tahap 2 |
| 3 | Collision Entity ↔ Kapal (Pemain, Mob, Item) | Sable + Create `ContraptionCollider` OBB penuh 6 sisi + carry + anti-clipping | Hanya cek strip atas dek untuk pemain lokal (`solid.rs:199`); 0% sisi/bawah/atap, 0% mob/item server | **15%** | **Tahap 1** |
| 4 | Collision Kapal ↔ Dunia & Antar-Kapal | Sable rigid-body voxel hull solver (impuls, torsi, gesekan, resting contact, body-body) | 1 titik bawah per blok (`body.rs:190`), geser Y tanpa impuls/torsi, `push_xz += 0.12` tembus dinding, 0% body-body | **15%** | **Tahap 1** |
| 5 | Honey Glue & Sistem Perakitan | `HoneyGlueEntity` + `HoneyGlueRenderer` (kotak 3D, partikel, Ctrl+Scroll, hapus), `SimAssemblyContraption` (`glueCache` + `honeyGlueCache` + Chassis), animasi tuas `PhysicsAssemblerBlockEntity`, disassembly alignment constraint | `glued_boxes` hanya pesan chat lalu dihapus tanpa dibaca `assembly.rs` (masih flood-fill buta); 0% visual/outline/partikel; disassembly langsung snap 90° | **20%** | **Tahap 1** |
| 6 | Offroad Wheels & Suspensi | `WheelMountBlockEntity` (extend `KineticBlockEntity`, RPM Create, steer via redstone kiri/kanan, rem via redstone atas, ValueBox suspensi, render roda berputar/belok/naik-turun) | Raycast suspensi dasar di `wheels.rs`, namun **ada bug `hub_world` di baris 56**; dikontrol WASD bukan RPM/redstone; roda tidak beranimasi | **25%** | **Tahap 1 & 2** |
| 7 | Baling-baling & Aerodinamika | `BasePropellerBlockEntity` (`KineticBlockEntity` + `BlockEntitySubLevelPropellerActor`), torsi reaksi, `ControlSurface`, `AeroTurbine`, animasi putar bilah & partikel angin | Gaya dorong & lift sederhana dari `PilotInput` di `aero.rs`; belum baca RPM Create; bilah tidak berputar | **25%** | Tahap 2 & 3 |
| 8 | Balon Udara, Burner & Daya Apung | `ServerBalloon` (`BalloonLayerGraph` 3D, `LiftingGasType`, isi bertahap, efek panas/glow/ember, `applyForces`) | Estimasi volume balon sederhana + tingkat panas `0..15` (`buoyancy.rs`), tanpa graf lapisan 3D, tanpa partikel/suara burner | **20%** | Tahap 2 & 3 |
| 9 | Kursi, Steering Wheel & Kontrol | `SteeringWheelBlockEntity` (`GeneratingKineticBlockEntity`, tahan klik kanan + gerak mouse, `angleInput` ValueBox) + duduk di Create Seat + interaksi blok saat terbang | Kamera sudah bebas, duduk di `create:*_seat` / `simulated:*_seat`, `Shift` turun; kontrol masih keyboard arcade, belum hold-click mouse steering & belum bisa klik blok onboard | **35%** | Tahap 2 & 3 |
| 10 | Simulasi Kinetik & Redstone Onboard | SubLevel penuh: jaringan kinetik Create, redstone, mesin, dan blok entity tetap hidup di atas kendaraan | **0%** — blok di atas kendaraan hanya array statis `BlockCell`, jaringan Create & redstone mati sampai dibongkar | **0%** | Tahap 3 |
| 11 | Sensor & Redstone Link Onboard | `AltitudeSensor`, `SpeedSensor`, `Compass`, `Gyroscope`, `AnalogLever`, sinyal ke redstone onboard | Pembacaan telemetri pasif di `sensors.rs`, belum memancarkan sinyal redstone onboard | **10%** | Tahap 3 |
| 12 | GUI Kustom `simulated` | `DiagramScreen`, `LinkedTypewriterScreen`, `AltitudeSensorScreen`, `NameplateScreen`, ValueBox suspensi/sudut | Hanya strip HUD pilot + output chat teks untuk `contraption_diagram` | **10%** | Tahap 4 |
| 13 | Blok Mekanik Lanjutan `simulated` | `SwivelBearing`, `Spring`, `RopeWinch`, `DockingConnector`, `Plunger`, `Borehead` (mining), `MountedPotatoCannon`, `Anchor` | Belum diporting (hanya terdaftar sebagai blok biasa jika ada di asset) | **0%** | Tahap 4 |
| 14 | Audio & Partikel | Suara propeller, burner, gesekan roda/suspensi, suara lem, partikel asap/api/madu/angin/debu | **0%** — sunyi total dan tanpa partikel khusus | **0%** | Tahap 2 |
| 15 | Persistensi, Multiplayer & Lifecycle | Simpan/muat vessel di dunia, sinkron LAN/server, rejoin/shutdown bersih | Snapshot sync + rejoin/shutdown dasar ada; penyimpanan penuh state SubLevel & lem antar-restart belum lengkap | **40%** | Tahap 1 & 3 |
| 16 | Kontrol Sentuh Android | Overlay kontrol kendaraan di Android | Lapisan tombol sentuh Aero dasar ada (`f807249446`), perlu sinkron dengan skema kontrol baru & GUI layar kecil | **40%** | Tahap 4 |
| — | **TOTAL KESELURUHAN** | **905 kelas Java (`simulated` + `aeronautics` + `offroad`)** | **Fondasi staging, render 6-DOF statis, dan prototype 6-DOF** | **~24%** | — |

---

## 4. Daftar Bug Kritis & Utang Teknis Terverifikasi (Exact File:Line)

Setiap poin di bawah ini sudah diverifikasi langsung di kode sumber baseline `586c9dda2a`:

1. **BUG #1 — Transformasi hub roda salah saat berotasi (`crates/aeronautics-physics/src/wheels.rs:56`)**
   - Baris 56 menulis: `let hub_world = v_add(body_pos, quat_rotate(orientation, local_center));`
   - Seharusnya (konsisten dengan `RigidContraption::local_to_world` dan `solid::to_world`):
     `let hub_world = v_add(v_add(body_pos, mass.center_of_mass), quat_rotate(orientation, r_local));`
   - Dampak: begitu kendaraan berbelok/miring, titik raycast roda melenceng dari posisi ban yang sebenarnya sehingga mobil melompat liar atau kehilangan grip.

2. **BUG #2 — Collision pemain hanya cek permukaan atas, 0% mob/item (`crates/mod-aeronautics/src/solid.rs:199-275`)**
   - `local_player_pass` hanya memeriksa `local[1] >= top - 0.65 && local[1] <= top + 0.25`. Pemain tembus dinding samping dan bagian bawah kapal, dan mob/item jatuh menembus kapal sama sekali.
   - Solusi: integrasikan `create_kinetics::rules::{collider, collider_obb, solid}` (`ContinuousOBBCollider::collide_many` + `save_client_player_from_clipping`) untuk klien **dan** tambahkan server entity pass mencontoh `crates/mod-create/src/solid/server.rs`.

3. **BUG #3 — Collision badan kapal ke dunia tidak realistis (`crates/aeronautics-physics/src/body.rs:190-252`)**
   - `resolve_world_collision` hanya menguji 1 titik (`w[1] - 0.5`) di tengah bawah tiap blok, tidak memperhitungkan 8 sudut blok atau rotasi OBB, hanya mendorong `position[1] += push_y` tanpa impuls/torsi kontak, dan memakai `push_xz += 0.12` tetap yang mudah tembus tembok/lereng.
   - Solusi: ganti dengan multi-corner OBB-vs-voxel contact manifold + sequential impulse solver (normal impulse + torsi $r \times J$ + Coulomb friction + Baumgarte position stabilization) serta broadphase AABB → OBB untuk collision antar-vessel.

4. **BUG #4 — `glued_boxes` tidak pernah dibaca `assemble_from_world` & 0% visual Honey Glue (`crates/aeronautics-physics/src/assembly.rs:32-108` & `crates/mod-aeronautics/src/server.rs:46-47,278,511-549`)**
   - Di `server.rs:522`, klik kanan Honey Glue memasukkan `[lo, hi]` ke `s.glued_boxes`, lalu di `server.rs:278` kotak lem dihapus saat dirakit, tetapi **`assemble_from_world` tidak menerima parameter zona lem sama sekali** — ia tetap melakukan BFS 6-arah ke semua blok yang menyentuh sehingga tanah/lantai ikut terangkat jika tidak diisolasi!
   - Solusi: terapkan graf konektivitas lem (`HoneyGlue` + `SuperGlue` + `Chassis`, mencontoh `create_kinetics::rules::super_glue` dan `SimAssemblyContraption`), bangun kendaraan di tanah tanpa menempel ke lantai jika tidak dilem ke lantai, plus tambahkan sinkronisasi klien + render box outline 3D & partikel madu mencontoh `crates/mod-create/src/machines/super_glue.rs`.

5. **BUG #5 — Render kapal statis (tanpa animasi bagian & partikel) (`crates/horizon-client/src/game.rs:2185-2315`)**
   - Baling-baling, roda offroad, dan burner digambar pada pose tetap tanpa sudut putar bilah (`engine_rpm`), sudut belok/putar ban, atau jarak kompresi suspensi per roda.
   - Solusi: kirim status roda (kompresi suspensi, sudut steer, fase rotasi) & fase rotasi baling-baling per kapal/blok di `VesselSnapshot`, lalu transformasi matriks model per bagian di `aeronautics_vessel_geometry`.

6. **GAP #6 — Belum ada interaksi blok & simulasi Create onboard**
   - Saat kapal aktif, pemain tidak bisa klik kanan kursi tertentu untuk duduk (saat ini hanya tombol pilot global / proximity), membuka peti, memutar tuas, atau menjalankan jaringan Kinetik Create di atas kapal.

7. **GAP #7 — Belum ada GUI kustom `simulated` & mekanika lanjutan**
   - `ContraptionDiagram`, `LinkedTypewriter`, `AltitudeSensor`, `Nameplate`, `Borehead`, `SwivelBearing`, `Spring`, `RopeWinch`, `DockingConnector`, `Plunger`, dan `BalloonLayerGraph` 3D belum diimplementasikan.

---

## 5. Tahapan Pengerjaan (Roadmap Terurut)

> **Catatan eksekusi:** Kerjakan berurutan per tahap. Jangan loncat ke Tahap 3–4 sebelum Tahap 1 lulus playtest fisika & collision!

### Tahap 1 — Fondasi Fisika, OBB Collision & Honey Glue Asli *(PRIORITAS UTAMA)*
**Tujuan:** Membuat kendaraan benar-benar solid seperti Create contraption, bisa dirakit di tanah dengan Honey Glue visual, tidak tembus dinding/lantai, dan stabil saat dikendarai.
1. **Fix bug `wheels.rs:56`** — perbaiki `hub_world` pakai `body_pos + mass.center_of_mass + quat_rotate(orientation, r_local)`, tambah unit test rotasi 90°/180°/45° di `aeronautics-physics` agar tidak regresi.
2. **OBB Collision Entity ↔ Kapal (Client & Server):**
   - Port/adaptasi `create_kinetics::rules::{collider, collider_obb, solid}` untuk pose 6-DOF penuh (`Vec3` + `Quat` + `center_of_mass`, memperluas `Sample` agar mendukung 3-axis quaternion, bukan hanya yaw+pitch seperti kereta).
   - Pasang pada `crates/mod-aeronautics/src/solid.rs` (pemain lokal bisa berjalan, melompat, menabrak dinding samping/bawah kapal, terbawa rotasi + translasi kapal, dengan `save_client_player_from_clipping`).
   - Tambahkan pass collision server (`server.rs`) untuk mob & dropped item mencontoh `crates/mod-create/src/solid/server.rs`.
   - Ekspor bentuk tumbukan blok akurat (`LevelRef::block_shape`, bukan hanya kubus `1×1×1`) di `VesselSnapshot`.
3. **Rigid-Body Hull Collision ↔ Dunia & Antar-Kapal (`crates/aeronautics-physics/src/body.rs`):**
   - Ganti `resolve_world_collision` 1-titik dengan kontak multi-sudut (8 sudut OBB blok tepi hull / permukaan luar saja untuk performa) + solver impuls sekuensial (gaya normal, torsi $r \times J$, gesekan Coulomb, stabilisasi posisi, anti-tunneling sub-step saat kecepatan tinggi).
   - Tambahkan deteksi & resolusi collision antar-kapal (broadphase AABB → narrowphase OBB blok permukaan).
4. **Honey Glue Asli (Logika + Visual 3D + Persistensi):**
   - Ubah `assemble_from_world` (`assembly.rs`) agar memakai konektivitas `HoneyGlue` + `SuperGlue` (integrasi dengan kotak lem `mod-create` bila ada) + blok `Chassis`; blok yang tidak terhubung lem tidak ikut terangkat sehingga kendaraan bisa dirakit langsung di atas tanah.
   - Sinkronkan daftar kotak `HoneyGlue` ke klien (`net.rs`), render kotak outline 3D semi-transparan + preview saat memilih titik kedua + partikel madu + klik kanan/kiri untuk menghapus kotak lem (mencontoh `crates/mod-create/src/machines/super_glue.rs`).
   - Persistensi kotak lem & vessel saat save/load dunia.
- **Kriteria selesai Tahap 1:** Mobil/kapal dirakit di atas tanah hanya mengangkat blok yang dilem; kotak lem terlihat jelas di dunia; pemain, mob, dan item berdiri & menabrak seluruh sisi kapal tanpa tembus; mobil bisa melaju, belok, dan menabrak tembok/lereng dengan respons fisika masuk akal; seluruh unit test lulus + verifikasi desktop & Android `cargo check`.

### Tahap 2 — Visual Dinamis, Animasi Bagian, Audio & Interaksi Kursi/Blok
**Tujuan:** Membuat kendaraan hidup secara visual dan interaktif seperti mod aslinya.
1. **Animasi model 6-DOF (`game.rs:aeronautics_vessel_geometry` + `net.rs`):**
   - Baling-baling (`Propeller`, `SmallPropeller`, `LargePropeller`) berputar mulus sesuai RPM & arah.
   - Roda `offroad:*_tire` memanjang/memendek sesuai kompresi suspensi aktual per roda, berbelok (steering yaw), dan berputar sesuai kecepatan maju-mundur.
   - Animasi tuas `PhysicsAssembler` saat dirakit/dibongkar.
2. **Partikel & Audio:**
   - Partikel api/asap/bara pada burner balon, angin pada baling-baling, debu/cipratan di titik kontak ban, partikel madu saat memasang/melepas Honey Glue.
   - Efek suara dasar (suara mesin/baling-baling berdasar RPM, burner, lem, tumbukan).
3. **Raycast & Interaksi Blok Onboard:**
   - Transformasi raycast kursor pemain ke ruang lokal kapal (`to_local`) sehingga pemain bisa klik kanan langsung kursi (`create:*_seat` / `simulated:*_seat`) untuk duduk, klik kanan `SteeringWheel` untuk memegang kendali, serta menekan tuas/tombol atau membuka penyimpanan di atas kapal.

### Tahap 3 — Integrasi Kinetik Create Onboard & Mekanika Asli Aeronautics
**Tujuan:** Menjembatani gap terbesar terhadap versi asli (`simulated` + Sable + Create 6.0.10).
1. **Simulasi Kinetik Create & Redstone Onboard:**
   - Jalankan solver jaringan `create-kinetics` pada grid lokal kapal aktif (sumber energi seperti Creative Motor / Steam Engine / Windmill / Hand Crank di atas kapal menggerakkan `Propeller` dan `WheelMount` lewat RPM sungguhan!).
   - Jalankan propagasi sinyal redstone onboard untuk mengendalikan steer kiri/kanan & rem `WheelMount` (mengikuti `WheelMountBlockEntity`), `Clutch`, `Gearshift`, dan output `AltitudeSensor` / `SpeedSensor`.
   - Sediakan opsi mode kompatibilitas/fallback kontrol langsung (pilot kursi/steering wheel) bila kapal belum memasang sistem redstone penuh agar tetap ramah pemain kasual.
2. **Mekanika Asli Blok Inti:**
   - `SteeringWheelBlockEntity`: tahan klik kanan + gerak mouse untuk memutar kemudi + ValueBox `angleInput` (memakai `value_box_screen` sprite asli Create).
   - `WheelMountBlockEntity`: ValueBox kekuatan suspensi + pemasangan/pelepasan ban langsung dengan klik kanan.
   - `ServerBalloon` (`BalloonLayerGraph` 3D + `LiftingGasType`): volume balon dihitung dari ruang tertutup 3D nyata, pengisian gas bertahap, gaya angkat akurat di pusat volume balon.
   - Disassembly alignment lembut sebelum snap ke grid dunia (mengikuti `PhysicsAssemblerBlockEntity`).

### Tahap 4 — GUI Kustom `simulated`, Blok Lanjutan & Polish Android
1. **GUI Kustom (`simulated`):** `DiagramScreen` (visualisasi titik pusat massa, berat, gaya angkat/dorong), `AltitudeSensorScreen`, `LinkedTypewriterScreen`, `NameplateScreen` — bergaya sprite Create/Aeronautics asli, mendukung desktop & sentuh Android.
2. **Blok Mekanik Lanjutan:** `ControlSurface`, `AeroTurbine`, `SwivelBearing`, `Spring`, `RopeWinch`, `DockingConnector`, `Plunger`, `Borehead` (mining), `MountedPotatoCannon`.
3. **Polish Android:** Sinkronisasi layout sentuh Bedrock/Aero untuk mengemudi mobil/pesawat/kapal dengan nyaman di layar HP landscape.

---

## 6. Panduan Build, Test & Aturan Kerja (Wajib Baca Agen)

### Lingkungan Host (Windows, RAM 8 GB, PowerShell 5.1)
- **Jangan pakai `&&` di PowerShell 5.1** — gunakan `; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }`.
- **Hindari OOM (`0xc0000409`) saat compile/test Rust di host 8 GB:** selalu set variabel lingkungan ini dan batasi paralelisme:
  ```powershell
  $env:RUSTFLAGS = "-C debuginfo=0"
  $env:CARGO_INCREMENTAL = "0"
  ```
  - **Jangan** jalankan `cargo test -p horizon-client --lib` penuh sekaligus (sering OOM). Jalankan per target sempit dengan `-j 1`:
    ```powershell
    Set-Location E:\Experiment\horizon
    $env:RUSTFLAGS = "-C debuginfo=0"; $env:CARGO_INCREMENTAL = "0"
    cargo test -j 1 -p aeronautics-physics
    cargo test -j 1 -p mod-aeronautics
    cargo test -j 1 -p horizon-client-lib -- creative
    cargo test -j 1 -p horizon-client --bin tachyon-desktop -- aeronautics_port
    cargo check -j 1 -p horizon-client --bin tachyon-desktop
    cargo check -j 1 -p tachyon-android
    ```
- **Catatan tes Create yang sudah gagal sebelum perubahan kita (pre-existing):**
  - `mod-create`: `machines::trains::tests::a_tree_survives_the_tag` sudah gagal secara pre-existing (tidak terkait Aeronautics/GUI). Tes terfilter (`create-kinetics` 2272/2272 dan tes `mod-create` non-trains) lulus.

### Aturan Repo & Push
- **DILARANG keras menaruh/commit aset Mojang atau aset ARR mod** (Create / Create Aeronautics textures, models, sounds, JAR) ke dalam repo. Verifikasi selalu dengan `python tools/check_public_tree.py` sebelum commit.
- Remote push Tachyon: cabang lokal `tachyon` dipush ke `tachyon/main` (`https://github.com/barkfart34-netizen/Tachyon.git`):
  ```powershell
  $env:GIT_TERMINAL_PROMPT = "0"
  git push tachyon HEAD:main
  ```
- Setiap selesai satu tahap atau memperbaiki bug besar, **selalu perbarui dokumen ini** (commit baseline, tabel progres, daftar bug tersisa) agar agen berikutnya selalu memulai dengan konteks akurat.
