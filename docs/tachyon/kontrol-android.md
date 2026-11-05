# Port kontrol sentuh Android bergaya Bedrock

Dokumen ini menganalisis `Vasysik/horizon-android`, branch `android`, dan merencanakan port ke
Tachyon. Tahap 1 dan 2 sudah diimplementasikan: format, kontrak nilai, serta engine gerak/tombol
murni, belum dihubungkan ke event Android. Zalith tetap aktif dan default. Tidak ada unduhan
tekstur, build, atau tes yang dijalankan di mesin kerja ini.
Perilaku yang dijelaskan sebagai perilaku fork berasal dari pembacaan sumber, bukan pembandingan
langsung dengan Bedrock.

Referensi fork adalah commit `7301f6fdd7f470b11317276e7a11b4dc615c137a`, di
`_ref/horizon-android/`. Tachyon yang diperiksa adalah `17b1cc7fbafb59c00729f8ceeb11f1da153bc2e6`.
Keduanya mematok Minecraft Java **26.3**, bukan 1.21 seperti pengantar tugas. Jarak 1630 commit
dari upstream adalah konteks dari pengguna; ini bukan alasan untuk menggabungkan branch fork.
Port harus mengambil bagian kontrolnya dan mengikuti API Tachyon sekarang.

## Cara kerja fork

Jalur dalam bagian ini relatif terhadap `_ref/horizon-android/`. Berkas integrasi yang tidak ada
di salinan sparse dibaca melalui `git -C _ref/horizon-android show HEAD:<path>`.

| Berkas | Tanggung jawab |
| --- | --- |
| `crates/horizon-client-render/src/touch.rs` | `TouchControls`: pemilik jari, gerak, pandangan, tap/hold, tombol bawaan, penanda mining, dan gambar HUD. Juga helper klasifikasi item dan `screen_direction()`. |
| `crates/horizon-client-render/src/touch/layout.rs` | `Mode`, `Control`, `Placement`, `Settings`, posisi bawaan, validasi, migrasi JSON, dan penyimpanan. |
| `crates/horizon-client-render/src/touch/custom.rs` | `CustomButton`, pilihan key/action, kepemilikan tombol per jari, `key_events()`, dan tombol tambahan untuk inventaris. |
| `crates/horizon-client-render/src/touch/editor.rs` | `open_editor()`, `edit_touch()`, `cancel_editor()`, penggeseran, ukuran, opacity, palet, dan pemilih key. |
| `crates/horizon-client/src/touch_ui.rs` | `TouchUi`: tombol utilitas Java, kombinasi F3, keyboard, gesture menu, drag slider, dan virtual mouse. Bukan engine gerak dunia. |
| `crates/horizon-client/src/game/pad.rs`, `game/android_input.rs`, `game.rs` | Menyambungkan hasil sentuh ke aksi game, ray interaksi, item yang dipegang, menu, dan input Android. |
| `crates/horizon-client-render/src/android_gamepad.rs` | `InputMode` dan `Devices` untuk controller Android fisik: axis, tombol, perangkat aktif, serta press pendek yang menunggu poll. |
| `packaging/android/bedrock-controls.json`, `crates/horizon-client/src/android_resources.rs` | Manifest tekstur dan `controls_with_progress()` yang mengunduh serta memeriksa berkas. |

### Empat skema, tetapi tiga baris layout

`Mode` hanya mempunyai `Crosshair`, `Tap`, dan `Dpad`. `Mode::SCHEMES` menggabungkannya dengan
`Settings::split_controls`; jangan menganggap ada empat varian enum atau empat layout tersimpan.

| Skema | `mode` | `split_controls` | Bidikan |
| --- | --- | --- | --- |
| Joystick + crosshair | `Crosshair` | `false` | Tengah layar; ini default fork. |
| Joystick + tap | `Tap` | `false` | Titik sentuh untuk interaksi dunia. |
| D-pad + tap | `Dpad` | `false` | Titik sentuh. |
| D-pad + crosshair | `Dpad` | `true` | Tengah layar. |

Kedua skema D-pad berbagi baris posisi. `Settings::crosshair()` memeriksa `Crosshair` atau
`split_controls`. Dalam mode crosshair, Attack dan Use terlihat secara bawaan; gesture dunia
baru aktif jika `crosshair_gestures` dinyalakan. Default opsi itu adalah `false`.

### Jari, gerak, dan gesture dunia

- `TouchControls::update()` menerima ID jari dan koordinat `[0..1, 0..1]`. Tombol custom mendapat
  kesempatan pertama, lalu tombol bawaan, lalu area pandangan. Satu jari memegang gerak; jari lain
  dapat memandang, melompat, atau memakai item. Peran ditentukan saat jari turun.
- `state()` menghasilkan stick kiri dengan arah atas positif. D-pad membulatkan tiap axis menjadi
  `-1`, `0`, atau `1`, dengan daerah tengah `1/3` radius. Joystick menormalkan panjang maksimalnya.
  `sprint_with_joystick` mengaktifkan sprint jika jari melewati tepi joystick ke depan; defaultnya off.
- Swipe melewati jarak `hypot(dx * aspect, dy) > 0.018` dianggap drag. `camera_delta()` mengubah
  delta ternormalisasi menjadi derajat, dikalibrasi terhadap sisi layar yang lebih panjang, dengan
  sensitivitas umum serta horizontal/vertikal. Swipe sebelum hold tidak langsung memakan makanan.
- Jari pandangan yang dilepas sebelum `HOLD_DELAY` **350 ms**, tanpa drag, membuat tap. Tap menyimpan
  `tap_aim` dan snapshot `held_use` melalui `take_tap() -> Option<bool>`; bukan langsung held LMB/RMB.
- Hold yang belum drag berkomitmen setelah 350 ms. `poll_state()` menahan trigger Use atau Attack
  menurut niat item yang disimpan saat jari turun. Setelah berkomitmen, drag boleh memutar kamera
  tanpa membatalkan aksi. Pelepasan jari menghentikan hold.
- Integrasi game dapat mengubah hold menjadi Use jika item atau entitas menerima penggunaan.
  `latch_use()` mempertahankan pilihan itu ketika makanan/item terakhir habis, agar hold yang sama
  tidak mendadak menambang. Penanda mining berupa lingkaran dalam yang membesar, bukan irisan pie.
- `clear()` menghapus gesture, sprint latch, ray, permintaan UI, dan melepas key custom.
  `clear_world_gesture()` berbeda: ia mempertahankan key custom untuk overlay inventaris.
- Sneak D-pad di darat membutuhkan dua tap dalam **300 ms**, lalu menghasilkan `RightStick`.
  Dalam fork, efek toggle masih bergantung pada dispatch controller; gesture itu sendiri belum
  menjamin toggle jongkok. Fly/swim turun dan turun dari kendaraan mempunyai perlakuan tersendiri.

### Item yang memakai tap atau hold

Helper berikut memakai ID `minecraft:`; pola nama tidak otomatis berlaku pada namespace mod.

| Helper | Item yang dicakup |
| --- | --- |
| `item_places_entity_on_tap()` | `*_boat`, `*_raft`, `*_spawn_egg`, `minecart`, `*_minecart`, `armor_stand`, `item_frame`, `glow_item_frame`, `painting`, `end_crystal`. |
| `item_uses_on_tap()` | Semua item di atas; `*_bucket` kecuali `milk_bucket`; juga `bucket`, `bone_meal`, `flint_and_steel`, `fire_charge`, `honeycomb`, `glass_bottle`. |
| `projectile_crosshair()` | `bow`, `crossbow`, `trident`, `snowball`, `egg`, `ender_pearl`, `experience_bottle`, `splash_potion`, `lingering_potion`, `wind_charge`, `fishing_rod`. |

`Game::touch_item_held_use()` dalam `game.rs` mengecualikan `item_uses_on_tap()` **lebih dulu**.
Setelah itu ia memeriksa klaim mod, `remote::item_has_use()`, bundle, `firework_rocket`, `lead`,
`name_tag`, blok dengan kelas crop/stem/nether wart/cocoa, dan komponen pemblokir serangan.
Jadi makanan, milk bucket, alat ber-use, dan banyak item lain menunggu hold; bukan semua item
nonblok. Dye dan music disc biasa tidak mendapat pengecualian hold dari daftar tersebut dan
umumnya jatuh ke penggunaan dengan tap. Komponen item dapat mengubah hasil klasifikasi.

Dalam mode tap, daftar `projectile_crosshair()` mengarahkan tap/hold ke `[0.5, 0.5]`, dan
`Game::interaction_crosshair()` mempertahankan crosshair untuk item itu. Ini tidak mengganti
`Mode`. `firework_rocket` termasuk hold-use tetapi **tidak** termasuk daftar projectile crosshair.
`screen_direction()` membentuk ray perspektif dari forward, up, aspect, FOV, dan titik layar tanpa
memutar pandangan. `interaction_direction()` memasukkannya ke ray interaksi game.

### Dari sentuhan menjadi aksi

`TouchControls::poll_state()` mengeluarkan `gamepad::PadState`: stick, bit tombol, dan trigger.
`Game::poll_pads()` menggabungkannya dengan controller fisik dan pad injeksi. `Edges` menghitung
perubahan, `pad_button()` mencari `settings.controller.actions_of(button)`, lalu `pad_action()`
memakai mapping key/mouse game. Pelepasan singkat dapat ditunda sampai tick sempat membacanya.

| Kontrol bawaan | Keluaran fork |
| --- | --- |
| Jump, Sneak, Sprint | `South`, `RightStick`, `LeftStick`. |
| Attack, Use, Shield | `RightTrigger`, `LeftTrigger`, `LeftTrigger`. |
| Inventory, Pause | `North`, `Start`; Start saat bermain menuju `pad_back()`. |
| Offhand | Press/release `action:key.swapOffhand`. |
| Chat, Settings, Debug, Perspective | Flag permintaan terpisah, bukan `Button`. |

Tap dunia mempunyai resolusi tambahan di `poll_pads()`. Jika ada `entity_targeted()`, tidak sedang
memakai item, dan layar Playing, fork segera memanggil `remote_start_attack()` atau `attack()`.
Ia tidak menjadwalkan held LMB: entitas yang hilang sebelum tick tidak boleh membuat blok di
belakangnya ikut ditambang. Jika bukan kasus itu, hanya tap dengan `held_use == false` yang
menjadi Use sekali. Tap makanan tidak memulai makan, tetapi masih bisa menyerang entitas.

### Layout, editor, dan tombol custom

- `Settings` menyimpan `layouts: [[Placement; 14]; 3]`, `button_visibility: [u8; 14]`, serta
  maksimal 32 `custom_buttons`. Visibility `0` mengikuti skema, `1` memaksa tampil, `2` menyembunyikan;
  array visibility berlaku bersama, bukan terpisah untuk setiap skema.
- `Placement` berisi pusat ternormalisasi, ukuran sebagai fraksi tinggi layar, dan opacity.
  `rect(aspect)` membuat tombol tetap persegi. Left-handed mencerminkan x; `button_scale` mengubah
  gambar dan area sentuh tanpa menulis ulang ukuran individual. `sanitize()` menolak nilai buruk
  dan membatasi ukuran, sensitivitas, jumlah tombol, serta token.
- Deserializer menerima tiga baris lama dengan 10/12/14 kontrol; kontrol baru diisi dari default.
  `Settings::load()`/`save()` memakai `horizon-touch.json` relatif ke working directory. Ada versi
  `load_from()`/`save_to()`; save menulis `json.tmp`, lalu rename.
- Editor menyimpan snapshot `original`. Done memanggil `Settings::save()` langsung; Cancel
  mengembalikan snapshot. Reset mengatur ulang posisi skema aktif dan visibility bersama. Palet
  mengembalikan/menyembunyikan bawaan, atau menambah custom; custom yang dihapus benar-benar hilang.
  Ukuran dan opacity dapat diterapkan ke semua tombol dalam baris aktif. Editor mengambil semua jari.
- `CustomButton` menyimpan satu `key: String` dan tiga `Placement`. Token berupa `key.keyboard.*`
  atau `action:key.*`, bukan `GLFW_*`. `key.keyboard.unknown` adalah tombol belum diberi aksi.
  Tidak ada macro multikey; kombinasi dibuat dengan beberapa jari atau utilitas seperti Hold F3.
- `key_events()` menghasilkan `(String, bool)`. Dua jari yang memegang key sama hanya membuat satu
  press; release keluar setelah pemilik terakhir lepas, meskipun jari sudah bergeser keluar tombol.
  `android_custom_events()` menerjemahkan raw key melalui keybind pemain, debug, dan hook mod.
  Token `action:` memakai mapping kanonis, sehingga aksi Jump tidak bergantung pada posisi key fisik.

### Tekstur fork: sumber dan cache sebenarnya

Manifest `packaging/android/bedrock-controls.json` mematok revision Mojang
`46ba6ea985fb5a92d79a9419198f10dda14c199d`. Setiap entri mencatat `path`, `source`, `sha256`, `size`,
dan biasanya `git_blob`. Downloader memakai SHA-256 dan ukuran; bukan Git blob sebagai verifikasi.

`android_resources::controls_with_progress(root, progress)` membentuk URL:

```text
https://raw.githubusercontent.com/Mojang/bedrock-samples/{revision}/resource_pack/textures/{entry.source}
https://raw.githubusercontent.com/Mojang/bedrock-samples/{revision}/LICENSE.md
```

URL kedua adalah kasus khusus `LICENSE.md`; `source: ../../LICENSE.md` tidak diterapkan mentah.
Tujuannya `root/assets/minecraft/textures/gui/bedrock/<entry.path>`. Cache-nya adalah berkas tujuan
itu sendiri: berkas dipakai ulang hanya jika ukuran dan hash tepat. Instalasi resmi fork lewat
`official_install.rs` menyiapkan `<home>/.local/horizon-client/official.partial`, lalu memasangnya
sebagai `<home>/.local/horizon-client/26.3`. Jangan menyebutnya cache launcher Tachyon.

`ureq` memakai timeout connect 30 detik/read 60 detik dan tanpa redirect. Ukuran tiap entri dibatasi
1 MiB, destination harus komponen path normal, pembacaan unduhan maksimal `size + 1`, dan hasil
harus tepat ukuran/hash. Setelah lolos ia melakukan `fs::write()` langsung; penulisan tiap berkas
belum atomik. Sprite dibaca sebagai `gui/bedrock/<nama>` oleh `TouchControls::draw()`.

`tools/android_bedrock_controls.py` adalah alat pengembang terpisah: clone privat
`https://github.com/Mojang/bedrock-samples.git` ke `.local/android/bedrock-reference/samples` dan
membuat `.local/android/bedrock-controls.zip`. Itu bukan mekanisme unduh pemain dan tidak dijalankan
dalam analisis ini. Manifest boleh menjadi metadata port; PNG dan ZIP jangan masuk repo atau APK.

## Input Tachyon sekarang

Jalur berikut berada di worktree Tachyon, bukan fork referensi.

| Berkas | Pemakaian sekarang |
| --- | --- |
| `crates/horizon-client/src/controls.rs` | `Controls`: hit-test, kepemilikan jari, tombol held/toggle/swipple/penetrable, joystick, event, dan gambar. |
| `crates/horizon-client/src/controls/layout.rs` | Format Zalith `editorVersion` 12: layer, tombol, text box, joystick, style, visibility dunia/menu, ukuran dp/persentase/wrap. Posisi memakai 0..10000 dari ruang kosong di sekitar widget. |
| `crates/horizon-client/src/controls/editor.rs` | `Editor`, `Outcome`, pengubahan layout dan properti; hasil save ditangani AndroidGame. |
| `crates/horizon-client/src/controls/menu.rs`, `controls/widgets.rs` | Bola menu, `Choice`, pengaturan sentuh, widget, dan tema UI. |
| `crates/horizon-client/src/controls/default_layout.json` | Layout Zalith bawaan yang tetap harus tersedia. |
| `crates/horizon-client/src/android/{mod,shell,launcher,controls}.rs` | Startup/JNI, satu event loop launcher/game, pemilihan layout, serta import/export. |
| `crates/horizon-client/src/game/android.rs`, `game.rs` | Routing sentuh saat game berjalan dan pemasangan overlay pada HUD, termasuk layar loading. |

`android/mod.rs::main()` memilih `install::Paths`, memasang JNI, dan mengatur working directory
serta `HORIZON_HOME` ke `Paths.home`. `Shell::play()` dalam `android/shell.rs` menyerahkan launcher
ke `AndroidGame::start()`. `window_event()` meneruskan `WindowEvent::Touch` ke `AndroidGame::touch()`.

`touch()` memberi editor/menu prioritas, lalu `Controls::down()` mengembalikan `Claim::Taken`,
`MoveOnly`, atau `None`. Sisa jari menjadi hotbar, pandangan dunia, atau pointer menu. `moved()` dan
`up()` melanjutkan kepemilikan itu. `about_to_wait()` menjalankan timer, event kontrol, gyro, dan IME.
`Overlay` tersimpan dalam `OVERLAY: RefCell<Option<Overlay>>` thread-local; `draw_touch()` dipanggil
oleh jalur gambar di `game.rs`. `resumed()`/`suspended()` mengelola surface, audio, gyro, dan release.

`Controls::move_stick()` mengeluarkan event direction/lock key, bukan stick analog `PadState`.
`take_events()` mengeluarkan `ControlEvent::Key`, `Launcher`, atau `SendText`. Dalam
`apply_control_events()`, nama GLFW didekode menjadi key/mouse, lalu menuju `mapped_key()`,
`play_mouse()`, event mouse menu, scroll, IME, atau chat. Jalur key mempertahankan debug dan
`native_key_event()`, tetapi **tidak** menjalankan `settings.keys.translate_all()` seperti jalur
keyboard biasa. Jangan menganggap raw key Zalith dan raw key custom fork sudah sama semantiknya.

Gesture dunia Zalith defaultnya off. Jika on, tap menahan RMB 60 ms dan hold diam 300 ms menekan
Attack; tidak ada klasifikasi makanan/projectile atau ray titik sentuh. Hotbar memilih saat down,
double tap menukar tangan, dan hold 300 ms membuang item. Menu memakai pointer relatif: tap LMB,
hold untuk drag. Fork menambah scroll-or-tap langsung dan virtual mouse opsional dengan hold RMB
500 ms. Port menu ini terpisah dari gesture dunia; jangan mengganti JNI IME Tachyon dengan runtime fork.

Layout tersimpan di `<home>/controls/<stem>.json`, dipilih melalui `controlLayout` dalam
`horizon.txt`; default ditandai stem kosong. `load_controls()` dan `save_layout()` ada di
`game/android.rs`; `android/controls.rs::{layouts,selected,import,export}` melayani launcher.
Preferensi sekarang adalah `touchControls`, `touchGestures`, `touchGyroscope`, `touchOpacity`,
`touchSize`, `touchMenuBall`, dan `largerInterface`. `controls::menu::Settings` bukan
`horizon_client::settings::Settings`; alias `TouchSettings` sekarang menunjuk yang pertama.

`GAMEPAD: Cell<PadState>` dalam `game/android.rs` hanya menampung controller **fisik**, dari
`NativeKeyCode::Android` dan axis `DeviceEvent::Motion`. `Controls::gamepad` adalah flag visibility.
`game/pad.rs` membaca `android::gamepad()` pada backend tanpa fitur controllers, lalu memakai
`poll_pads()`/`pad_move()`. Touch Zalith tidak digabung ke `GAMEPAD`.

### Perbedaan tepat kedua `gamepad.rs`

Perbandingan seluruh `crates/horizon-client-input/src/gamepad.rs` menunjukkan tambahan berikut
di fork; `Button`, `PadState`, `Edges`, `PadSettings`, `merge()`, dan `move_vector()` tidak berubah.

| Tambahan fork | Relevansi port |
| --- | --- |
| `BITDO_ULTIMATE2_VID_PID = (0x2dc8, 0x6012)` | Identitas controller 8BitDo Ultimate 2, bukan kontrol layar. |
| `stick_axes(left, right, left_z, vendor, product, sdl_mapped)` | Koreksi right stick perangkat itu ketika gilrs belum mendapat mapping SDL. Tidak diperlukan untuk touch. |
| `java_move_vector(stick, deadzone) -> Option<[f32; 2]>` | Memakai `move_vector()`, lalu delapan sektor dengan ambang `max(abs(axis)) * (SQRT_2 - 1)` dan `input::keyboard_direction()`. Berguna untuk gerak touch default. |
| `java_stick_has_exact_keyboard_directions_at_every_magnitude`, `ultimate2_right_stick_on_z_without_sdl` | Dua tes tambahan untuk helper di atas. |

`android_gamepad.rs` adalah modul lain, bukan perubahan tersembunyi pada tipe `PadState`.
Tidak perlu menyalin seluruh `gamepad.rs` atau membawa perbaikan 8BitDo untuk memasang touch.

## Rancangan port ke API sekarang

Format dan kontrak nilai tahap 1 serta engine gerak/tombol tahap 2 sudah tersedia; wiring dan
bagian engine lainnya masih **usulan**. Engine Bedrock dan Zalith harus saling eksklusif untuk
satu jari/gesture. Controller fisik tetap pada `Pads` sekarang.

### Berkas dan pemilik state

| Tujuan | Yang dibawa atau diubah |
| --- | --- |
| `crates/horizon-client-render/src/touch.rs`, `touch/{layout,custom,editor}.rs` | Port engine, geometri, editor, dan gambar; tidak boleh mengakses `Game`, JNI, atau unduhan. Pisahkan save editor dari I/O. |
| `crates/horizon-client-render/src/lib.rs`, `crates/horizon-client/src/lib.rs` | Daftarkan dan re-export `touch`; render sudah me-re-export `gamepad`, `keybinds`, `text`, dan `items`. Tidak perlu dependency baru. |
| `crates/horizon-client/src/game/android.rs` | Pilihan engine dalam `Overlay`, routing jari, viewport, HUD, settings panel, IME, dan lifecycle. Pertahankan engine/editor Zalith. |
| `crates/horizon-client/src/game/touch_input.rs` dan `game.rs` | Adapter gameplay baru, state touch terpisah, konteks item, resolusi tap, ray, dan release. Isi semua initializer `Game`; tanpa state touch aktif pada server. |
| `crates/horizon-client/src/touch_ui.rs` | Port hanya gesture menu/utilitas yang diperlukan; hubungkan ke JNI dan menu mouse sekarang. Jangan membawa `platform` fork. |
| `crates/horizon-client-input/src/gamepad.rs` | Bila dipakai, tambahkan hanya `java_move_vector()` dan tesnya. |
| `crates/horizon-client/src/android/touch_assets.rs`, `android/install.rs`, `android/launcher.rs` | Downloader kecil, task/progress, cache persisten, serta pemulihan sebelum resource dimuat. |
| `packaging/android/bedrock-controls.json`, `THIRD_PARTY_NOTICES.md` | Metadata revision/hash dan atribusi kode; tanpa berkas gambar. |

`TouchControls` disarankan dimiliki `Overlay`, agar event dan HUD membaca geometri yang sama.
`Game` hanya memiliki `TouchInput` gameplay. Jangan menaruh seluruh `TouchControls` di `Pads`
seperti fork: Tachyon sudah mempunyai pembungkus Android dan ownership berbeda.

Gunakan kontrak nilai `TouchContext`, `TouchFrame`, dan `TouchFeedback` tahap 1, dengan tambahan
gerak dan edges tahap 2:

1. Ambil snapshot item, projectile, shield, fly/swim/mount, sneak, dan status layar dari `Game`;
   niat item harus disnapshot juga sebelum `update()` menerima down, bukan setelah item berubah.
2. Dalam borrow singkat `with_overlay()`, update konteks, poll state, dan ambil frame milik sendiri:
   `PadState`, delta derajat, ray, tap, key edges, serta permintaan UI. Setelah itu tutup borrow.
3. `Game` menerapkan frame sebelum tick melalui `AndroidGame::about_to_wait()`, menentukan target
   dengan ray frame itu, dan mengembalikan feedback seperti latch-use/progress. Update overlay
   setelah borrow `Game` selesai; jangan memanggil game/render lagi saat `RefCell` masih dipinjam.

Ray tap harus bertahan sampai aksi/tick mengonsumsinya. Frame lama dibuang saat pembatalan atau
pergantian engine. Antrian tap bernilai lebih aman daripada satu `Option` yang dapat tertimpa;
perubahan dari fork ini perlu tes agar dua tap tidak menghasilkan use tambahan.

### Ketidakcocokan yang harus diadaptasi

| Referensi fork | Kondisi Tachyon dan penyesuaian |
| --- | --- |
| `Pads.touch`, `touch_aim`, `touch_ui`, `touch_shield`, `custom_keys`, `hardware_keys`, `last_input_mode` | Tidak ada pada `Pads` Tachyon. Gunakan `TouchInput`/overlay dan ledger sumber input, bukan menempelkan field tanpa wiring. |
| `settings.touch`, `settings.keypad`, halaman Touch fork | Tidak ada pada render `Settings`/opsi Tachyon. Pakai konfigurasi Android tersendiri dan panel yang membuka konfigurasi overlay. |
| `android_runtime::{active_input,keypad_state,gamepad_state}`, `crate::platform`, `device_runtime` | Runtime fork tidak tersedia. Pakai `AndroidGame`, `android::jni`, `GAMEPAD`, dan shell sekarang; keypad/TV bukan prasyarat port. |
| `poll_pads()` fork tetap berjalan untuk Android ketika controller disabled | Tachyon `poll_pads()` dan `pad_move()` langsung berhenti jika controller disabled. Poll touch terpisah; jangan mengaktifkan controller sebagai jalan pintas. |
| `pad_action()` fork mengirim release mouse di luar Playing | Versi Tachyon hanya memanggil `play_mouse()` saat Playing, meski caller berniat melepas selalu. Adapter touch wajib membersihkan hold lintas layar dan suspend. |
| `remote::item_has_use()`, `remote_start_attack()` | Keduanya private dalam `game/remote.rs`. Tambahkan wrapper sempit di sana atau visibility `pub(super)` yang diperlukan, bukan membuka seluruh modul. |
| `self.mods.mods.claims(&stack.item)`, `mod_key()` | Runtime klaim mod lama hilang; `game/mods.rs::ModsState` sekarang berisi content mod. Pertahankan `native_key_event()` dan beri kebijakan use berbasis metadata/hook baca-saja, tanpa menghidupkan API lama. |
| `interaction_direction()`, `block_reach`, `entity_reach` | Helper/field fork itu tidak tersedia. Adaptasi di `Game::aim()` memakai `reach()` dan aturan entity/raycast sekarang, bukan menyalin perubahan reach fork. |
| Mutasi yaw/pitch pada jalur touch fork | Gunakan `game/perspective.rs::turn_player()`, yang memperbarui model dan membatasi putaran penumpang boat. Delta derajat mengikuti konvensi `/ 0.15` seperti `pad_look()`. |
| `TouchControls::viewport()` lazy-load dan editor `Settings::save()` | Hindari load/save CWD tersembunyi. Muat path eksplisit saat startup; editor mengeluarkan outcome Save/Cancel dan AndroidGame menyimpan hasil. |

Dispatch tombol Bedrock harus memakai aksi tetap, tidak `controller.actions_of()`, glyph,
deadzone, atau opsi menu controller. `PadState` masih boleh menjadi format antara, dengan edges
touch sendiri. Pause dapat memakai `pad_back()`; aksi key/mouse memakai jalur kanonis game.
Sneak D-pad, sneak hold joystick, descend fly/swim, dan sprint latch perlu kebijakan touch eksplisit:
jangan membalik lagi state yang sudah latched melalui opsi toggle keyboard/controller.

Ledger input harus mencatat sumber, pemilik, mapping saat press, dan tujuan dunia/menu. Release
touch tidak boleh melepas key/mouse fisik yang masih held, atau berubah menjadi klik di menu baru.
Raw key custom harus melalui `key_code()`, debug, native hook, dan `translate_all()`; `action:`
tetap kanonis. Refaktor helper bersama untuk keyboard/Zalith/touch harus menjaga perilaku layout
lama secara sadar, karena jalur Zalith sekarang melewatkan rebinding.

Gerak memerlukan satu helper gabungan dengan hasil `Option<[f32; 2]>`, bukan mengganti semua
`pad_move()` menjadi state touch. Sambungkan ke `game.rs`, `game/remote.rs`, dan `game/tutorial.rs`.
Sumber jump/shift/sprint juga harus terlihat oleh aksi seperti `use_hand()` dan `hands_busy()`.
Default touch delapan arah; analog 360 derajat opsional. Pertahankan `remote.rs` yang menghapus
`keys.analog` pada server eksternal: prediksi dan packet input harus tetap sesuai aturan sekarang.

`aim()` harus mempertahankan prioritas `camera_mob()`, serta `forced_hit`/`tick_hit` dalam `pick()`.
Ray touch hanya aktif saat engine Bedrock bermain. Pakai FOV aktual dari `locator_camera` bila
tersedia; uji aspect dan perspektif, jangan mengganti semua target dengan sudut jari secara global.
Pertahankan cakupan `game/hand.rs::entity_targeted()` yang sekarang termasuk cushion, interaction
entity, dragon, guest, dan projectile. Resolusi tap entitas harus segera, tanpa held attack lanjutan.
Use harus tetap lewat jalur game yang mencapai `use_hand()`, `tachyon_use()`, dan native hooks;
predicate konteks tidak boleh menjalankan aksi tersebut sekadar untuk mencari tahu apakah bisa dipakai.

### Default, pilihan layout, dan penyimpanan

- Usulkan `touchEngine=bedrock|zalith` dalam `horizon.txt`. Instalasi baru memakai Bedrock joystick
  + crosshair, mengikuti default fork. Empat skema tersedia dalam panel Kontrol sentuh.
- Jika preferensi engine belum ada dan `controlLayout` lama tidak kosong, pertahankan Zalith
  dan simpan pilihan itu sekali. Selain itu pilih Bedrock. Kebijakan migrasi ini perlu persetujuan;
  jangan menimpa layout/pengaturan lama setiap APK diperbarui.
- Launcher tetap menyediakan Zalith Default dan seluruh `<home>/controls/*.json`. Memilihnya
  mengubah engine sekaligus stem; berpindah ke Bedrock tidak menghapus `controlLayout` terakhir.
  Dalam game, pilihan engine tersedia dari menu kontrol; perpindahan selalu membatalkan semua hold.
- Simpan Bedrock secara terpisah, misalnya `<home>/touch/bedrock.json` dengan versi format.
  Jangan memasukkannya ke direktori yang diparse `Layout::parse()` sebagai layout Zalith.
  Import/export kedua format harus berlabel jelas, tanpa janji konversi otomatis yang kehilangan aksi.
- Pertahankan `touchControls` sebagai saklar kontrol virtual, bukan controller fisik. Gunakan
  `touchGyroscope` sebagai sumber preferensi gyro bersama dan JNI sekarang, bukan dua nilai berbeda.
  `touchGestures` dan `touchMenuBall` tetap milik Zalith; Bedrock memakai aturan skema sendiri.
- Opsi Bedrock: skema, gesture crosshair, contextual interact, sensitivitas umum/x/y, left-handed,
  tukar jump/sneak D-pad, skala tombol, lingkaran mining, sprint button/toggle/joystick, analog,
  offhand/shield, kontrol inventaris, virtual mouse/sensitivitas, dan editor/custom buttons.
  Ambil default fork dahulu; opsi tambahan seperti gyro dan analog tetap off.
- Opacity global `touchOpacity` dapat mengalikan opacity individual. `touchSize` Auto Zalith
  tidak identik dengan `button_scale`; jangan menyalin angkanya tanpa aturan migrasi. Pertahankan
  `largerInterface`, `debugGuiScale`, dan safe insets Tachyon; skala HUD bukan skala hit-test fisik.
- Panel Android dapat dibuka dari launcher, menu jeda, dan `Control::Settings`. Tautan dari
  Options/Controls membutuhkan request ke panel ini, bukan salinan penuh `options_ui.rs` fork.
  Save harus membuat direktori, sanitize, menulis sementara, lalu mengganti hasil; kegagalan
  dilaporkan dan file lama dipertahankan. Cancel/Back tidak menyimpan preview editor.

### Tekstur dan atribusi port

`android/install.rs::Paths` memisahkan `root` resources, `home` game, dan `cache`. `unpack()`
mengganti seluruh root resource pada pembaruan APK. Karena itu cache fork di dalam root saja
akan hilang; unduhan harus mempunyai salinan persisten di luar root yang diganti.

Usulkan salinan terverifikasi di `<Paths.home>/touch-assets/<revision>/`, dengan berkas sementara
di `Paths.cache`. Sebelum setiap Play, baik setelah unpack maupun ketika resources sudah terbaru,
pulihkan ke `Paths.resources()/assets/minecraft/textures/gui/bedrock/<path>`. Alternatifnya layer
resource terpisah, tetapi perlu plumbing loader baru. Pemulihan kecil sebelum loading lebih sederhana.
Pemulihan harus selesai **sebelum** `loading_app()`, content/pack overlay, dan atlas dibangun.
`crates/horizon-client-render/src/hud.rs::HudAtlas::build_over()` sudah mengemas `textures/gui`;
keberadaan PNG yang datang setelah atlas dibangun tidak otomatis memperbarui HUD.

Pakai `ureq`, `sha2::Sha256`, dan `android/install.rs::Task::spawn()` yang sudah tersedia.
`tachyon/fetch.rs` memberi pola timeout/hash/progress, tetapi `Fetch` terikat Modrinth/JAR/SHA-512;
bukan API download PNG yang bisa dipanggil langsung. Jangan menambah dependency atau membawa
seluruh installer resmi fork. Pertahankan URL/revision/hash pin, ukuran bounded, tanpa redirect,
validasi destination/source, serta penulisan sementara dan rename setelah verifikasi.

Unduhan harus dapat ditolak atau dicoba lagi, berjalan di worker, dan tidak menghalangi permainan
offline. Fork `fitted_sprite()` tidak menggambar apa pun jika sprite hilang. Maka fallback asli
atau prosedural yang jelas untuk semua tombol wajib ada sebelum Bedrock menjadi default.
Resource pack tetap boleh override `assets/minecraft/textures/gui/bedrock/<relative path>`;
salinan hasil unduh adalah lapisan default, bukan override paksa di atas pack pemain.
PNG, ZIP pengembang, dan cache pemain tidak masuk repository, APK, atau artifact publik.

`_ref/horizon-android/LICENSE` adalah MIT dengan copyright
`Copyright (c) 2026 Ludovic Marie and the Horizon contributors`. Saat kode disalin, catat fork,
branch, commit, dan berkas asal dalam notice; sertakan **seluruh** teks izin dan disclaimer MIT
dalam `THIRD_PARTY_NOTICES.md`/notice distribusi, bukan hanya label MIT atau tautan GitHub.
Atribusi layout Zalith GPL-3.0 yang sudah ada di `THIRD_PARTY_NOTICES.md` tetap dipertahankan.
MIT kode fork tidak otomatis memberi izin redistribusi PNG Mojang. Manifest mengunduh
`LICENSE.md` sampel, tetapi teks dan cakupan lisensinya pada revision tersebut belum diverifikasi
di sini; lisensi aset/EULA dan persetujuan mekanisme unduh perlu diperiksa terpisah.

## Tahapan yang dapat dikompilasi terpisah

Ukuran berikut adalah perkiraan baris yang dibawa/diubah, termasuk tes saat implementasi, bukan
hasil build. Tiap tahap mendaftarkan modul, tipe, initializer, dan guard platform yang dibutuhkannya;
jangan meninggalkan referensi ke modul tahap berikutnya. Zalith tetap aktif sampai jalur baru siap.

| Tahap | Hasil mandiri | Perkiraan |
| --- | --- | --- |
| 1. Kontrak dan format — selesai | `Mode`/`Placement`/konfigurasi, tipe custom minimal, kontrak frame, sanitasi dan roundtrip; belum mengambil input. Notice MIT masuk bersama kode pertama. | 350–500 baris |
| 2. Engine gerak/tombol — selesai | Kepemilikan jari, joystick/D-pad, edges dan helper gerak delapan arah; API murni dengan tes desktop, belum dipasang pada event Android atau diuji lokal. | 350–500 baris (perkiraan awal) |
| 3. HUD | Port gambar/indikator dalam modul terpisah bila perlu, fallback prosedural, geometri gambar/hit-test sama. Preview opsional, tanpa unduhan. | 350–500 baris |
| 4. Adapter Android | `Overlay` memilih engine secara opt-in, frame menuju `TouchInput`, fixed actions, ledger sumber dan cleanup suspend/screen/cancel. Semua initializer diperbarui. | 350–500 baris |
| 5. Gesture | Tap/hold 350 ms, snapshot item, latch-use, classifier dan clock yang bisa diinjeksi tes; fitur gesture belum diaktifkan sampai ray tahap 6 siap. | 250–450 baris |
| 6. Target dan gameplay | Ray `aim()`, wrapper remote sempit, tap entitas segera, konteks use, gerak remote/tutorial dan feedback HUD. Uji server-thread, bukan hanya in-frame. | 300–500 baris |
| 7. Tombol custom | Token key/action, rebinding/native/debug, ownership bersama dan release lintas layar; editor custom belum diperlukan. | 250–450 baris |
| 8a/8b/8c. Editor | 8a drag/size/opacity bawaan; 8b palet/reset/visibility; 8c custom key picker dan Save/Cancel berbasis outcome. Masing-masing selesai tanpa bagian berikutnya. | 250–400 baris per bagian |
| 9. Menu sentuh | Bagian `TouchUi` yang diperlukan, scroll/tap/slider/inventory drag dan virtual mouse; tetap memakai JNI IME. | 250–450 baris |
| 10. Assets | Manifest metadata, downloader terverifikasi, cache/pemulihan setiap Play, retry/offline dan pack precedence. Fallback tetap berfungsi tanpa network. | 250–450 baris + manifest |
| 11. Pilihan/default | Panel opsi, launcher selector, persist/migrasi, import/export terpisah; baru sekarang Bedrock menjadi default instalasi baru. | 300–500 baris |
| 12. Bukti emulator | Driver sentuh, assertion state/log, skenario lifecycle, screenshot dan dokumentasi hasil; wiring CI dikerjakan pada tugas implementasi terpisah. | 150–350 baris |

Perkiraan total sekitar 4–6 ribu baris ditambah metadata; bukan port beberapa ratus baris.
Jika bagian membesar, pecah lagi menjadi PR sekitar 500 baris yang tetap lengkap. CI perlu mengecek
desktop, Android tanpa default features, dan server; kode UI/Android tidak boleh bocor ke server.

### Hasil tahap 1

- `horizon_client_render::touch`, juga di-re-export sebagai `horizon_client::touch`, tersedia di
  semua platform. `layout.rs` membawa tiga `Mode`, empat `SCHEMES`, urutan 14 `Control`, posisi dan
  default fork, geometri persegi, mirror left-handed, swap jump/sneak D-pad, serta visibility bersama.
- `Settings::parse()` dan `to_json()` hanya mengolah string milik caller. Format JSON memakai
  `version: 1`; versi tanpa field itu diterima sebagai format lama fork, versi JSON lain ditolak. Tiga
  baris dengan 10/12/14 kontrol dan visibility lama 12 entri diisi dari default tanpa memindahkan
  kontrol tersimpan. Sanitasi mengganti nilai nonfinite, membatasi geometri dan sensitivitas, serta
  menyimpan maksimal 32 custom button; visibility di luar 0/1/2 kembali ke default skema.
- `CustomButton` baru menyimpan token dan tiga placement, dengan validator keyboard/action fork.
  Tombol kosong `key.keyboard.unknown` tetap disimpan. Belum ada dispatch key, ownership jari, editor,
  gambar, atau penyimpanan berkas. Migrasi posisi factory dalam `load_from()` fork belum dibawa.
- `TouchContext`, `TouchFrame`, `TouchTap`, `TouchRequest`, dan `TouchFeedback` adalah nilai owned,
  tanpa `Game`, JNI, clock, atau `RefCell`. Aim berupa titik layar ternormalisasi; proyeksi ray baru
  pada tahap 6. Setiap tap dalam `VecDeque` membawa aim dan snapshot niat use sendiri, bukan satu
  `Option` yang tertimpa. Kontrak cleanup dan handoff borrow belum merupakan adapter yang berjalan.
- Perbedaan sengaja: tidak ada load/save CWD tersembunyi; `virtual_mouse` menjadi opsi format dengan
  default off; preferensi gyro tidak diduplikasi karena nanti memakai `touchGyroscope`/JNI Tachyon.
  Belum ada `touchEngine`, migrasi preferensi, atau perubahan default aplikasi maupun layout Zalith.
- Tes berada di `crates/horizon-client/src/desktop/touch_tests.rs`, didaftarkan sebagai modul
  `desktop::tests` dengan `#[cfg(test)]`. Filter CI sekarang
  `cargo test -p horizon-client --bin tachyon-desktop -- tachyon desktop native_hooks` mencakup tes
  default/skema, roundtrip, format lama/invalid, sanitasi, token, geometri, dan antrian dua tap.
  Tes ditulis tetapi tidak dijalankan; hasil kompilasi dan pengujian menunggu CI Linux.
- `THIRD_PARTY_NOTICES.md` memuat commit, berkas asal, copyright, dan seluruh notice MIT fork.
  Tidak ada tekstur, manifest unduhan, maupun aset lain yang dibawa.

### Hasil tahap 2

- Engine dan sembilan tes dari `gpt/15-touch-stage-2` dipasang di atas tahap 1, tanpa mengganti
  `Mode`, `Placement`, `Control`, `Settings`, format JSON, atau tipe custom. `TouchControls` memakai
  `TouchContext` dan `TouchFrame` yang sama; seluruh opsi/default dan tes format tahap 1 tetap ada.
- API `down`/`moved`/`up`, `state`, `poll_frame`, dan `clear` murni, tanpa `Game`, JNI, file atau
  jaringan. Satu jari memegang gerak; jari kedua di area gerak diabaikan sampai lepas. Peran tombol
  tidak berubah saat drag keluar; release menunggu pemilik terakhir. Press singkat sebelum poll
  bertahan satu poll. Jari di luar kontrol belum diklaim.
- Angka fork dipertahankan: joystick dibatasi panjang 1; D-pad memakai daerah tengah `1/3` radius
  dan diagonal ternormalisasi; auto-sprint joystick hanya untuk axis depan mentah `> 1`. Sneak
  D-pad darat membutuhkan dua down dengan selisih `< 300 ms`; fly/swim/dismount memakai hold biasa.
  Waktu sneak berupa `Duration` milik caller, aman saat waktu mundur. Sprint toggle menyimpan latch
  sampai toggle berikutnya atau clear; sneak masih berupa edges `RightStick`, belum toggle gameplay.
- `TouchFrame` ditambah `movement` dan edges touch tersendiri, tanpa preferensi controller.
  Request memakai enum tahap 1 `TouchRequest`, ditambah `Offhand` agar aksi sekali per down dari
  port sebelumnya tetap tersedia; bukan key custom held. `shield_held` diisi dari pemilik Shield.
  Look, aim, antrian tap, key custom dan hold dunia tetap netral. `clear()` membuang pekerjaan yang
  belum dipoll dan mengembalikan release untuk hold yang sudah keluar; caller wajib menerapkannya.
- `Placement::fitted()` ditambahkan pada `touch/layout.rs` tahap 1 untuk geometri hit-test/HUD
  yang sama: sanitasi, skala, dan clamp viewport hanya pada salinan. Penataan khusus shortcut
  portrait dan factory Offhand di samping Inventory menunggu tahap HUD, seperti port sebelumnya.
- `gamepad::java_move_vector()` mengikuti delapan sektor fork dengan ambang
  `max(abs(axis)) * (SQRT_2 - 1)` dan `input::keyboard_direction()`. Frame memakai deadzone touch
  `0`; analog 360 derajat tetap opt-in. Tidak ada perubahan input controller fisik atau `Pads`.
- Sembilan tes engine tetap di `game/tachyon_engine/touch.rs`, dalam `#[cfg(test)] mod tests`,
  dipilih oleh filter CI `tachyon desktop native_hooks`. Fixture memakai roundtrip `Settings`
  tahap 1; tes juga memeriksa output gesture netral dan `shield_held` pada frame. Tes tidak dijalankan
  lokal; kompilasi desktop Linux, pengujian, dan check Android menunggu CI.
- Notice MIT yang sudah ada diperluas dengan helper `gamepad.rs`; tidak ada notice kedua, aset,
  dependency, perubahan Create, atau wiring Android. Engine tetap belum dipasang; pemilihan opt-in
  masuk tahap 4, dan default Zalith tidak berubah sampai tahap 11.

## Pengujian implementasi di emulator CI

`.github/workflows/tachyon-smoke.yml` berjalan manual melalui `workflow_dispatch`. Ia membangun
APK `x86_64-linux-android`, lalu memakai emulator API 33 `google_apis`, profil Pixel 6, 4 core,
RAM 6144M dan `swiftshader_indirect`. SDK build Android 36 tidak berarti emulator memakai API 36.
Artifact `smoke/` disimpan tiga hari. Tidak ada perubahan workflow atau skrip CI dalam tugas ini.

`.github/tachyon/smoke.sh` sekarang hanya memasang APK, menunggu log resource/world, mengecek
proses tetap hidup, dan mengambil screenshot. Peluncurannya:

```sh
adb shell am start -n com.tachyon.game/.TachyonActivity --es tachyon.smoke "--world,Smoke,--creative,--seed,5"
```

Itu belum menyuntikkan touch atau membuktikan aksi benar. Untuk implementasi, tambahkan driver
pengembang di `tools/` dan hubungkan pada job yang sama dalam perubahan CI tersendiri:

- Awali data app/dunia uji baru. Fixture dan server pendamping memakai home scratch, bukan data
  pemain. Tunggu world siap dan tentukan koordinat dari resolusi, insets, serta layout aktif.
- `adb shell input tap`/`swipe` cukup untuk tap dan satu drag, bukan bukti multitouch. Untuk
  joystick + look + jump bersamaan, gunakan probe uji `MotionEvent` dengan pointer ID/index dan
  `ACTION_POINTER_DOWN/UP/CANCEL` yang benar, memakai API Android standar tanpa dependency baru.
  Probe harus melewati event activity/winit, bukan hanya memanggil adapter atau `horizon-ctl pad`.
- Jalankan empat skema; cek arah/kecepatan diagonal, sprint, double-tap sneak, fly/swim descend,
  mount, hotbar dan offhand. Cek bahwa mematikan controller tidak mematikan touch dan remap pad
  tidak mengubah tombol Bedrock. Beralih Zalith/Bedrock tidak menyisakan hold.
- Fixture item: block/place, boat/spawn egg/minecart, water bucket, bone meal, makanan/milk bucket,
  bow/crossbow/trident/throwable/fishing rod, firework, shield, crop, dye/music disc, serta item mod.
  Buktikan swipe tidak makan, tap makanan tidak use, hold melewati depletion tidak mining,
  dan tap entitas yang hilang tidak merusak blok belakang. Cek cancel tidak menjadi tap.
- Buka inventaris/chat/opsi saat hold, drag stack/slider, scroll tanpa klik baris, RMB virtual,
  kombinasi F3, key custom rebound, dan key fisik/custom sama. Uji IME, Back, suspend/resume,
  kehilangan fokus serta perubahan ukuran/perspektif; release tidak boleh meninggalkan mining/use.
- Uji JSON lama/invalid, editor Done/Cancel/Back, dan restart. Unduhan pertama, cache hit offline,
  hash salah/terpotong, gagal network, resource APK diganti, serta override pack harus diuji.
  Jangan unggah tekstur cache sebagai artifact; screenshot diagnostik mengikuti kebijakan proyek.
- Gunakan assertion perubahan posisi, target, jumlah item/blok dan state held selain screenshot
  normal/pressed/mining. Log hidup saja tidak cukup. Pure tests memakai waktu eksplisit, bukan sleep.
  Uji single-player server-thread dan server Java eksternal untuk ray/prediksi, lalu perangkat ARM
  nyata untuk gyro, latensi, cutout dan controller; emulator x86_64 tidak membuktikan semuanya.

## Risiko dan pertanyaan terbuka

- Penggabungan sumber input adalah risiko utama: release dapat melepas input fisik, toggle dapat
  terbalik dua kali, dan frame lama dapat menyerang saat layar berubah. Tetapkan ownership dahulu.
- Ray sentuh pada third-person/front view, kamera entitas, FOV berubah, GUI scale dan portrait
  perlu hasil uji. Tentukan batas dukungan awal; jangan mengklaim projection fork sudah benar di sini.
- Daftar item fork hardcoded, sedangkan `remote::item_has_use()` Tachyon juga mengenal
  `blue_egg`, `brown_egg`, dan spear. Kebijakan projectile/item mod, komponen override, konteks TCE,
  serta shield saat sneak perlu keputusan dan tes; jangan meniru API `mods.claims` yang sudah hilang.
- Apakah pengguna lama tanpa layout custom langsung beralih? Apakah dua skema D-pad tetap berbagi
  posisi/visibility? Rekomendasi awal mengikuti penyimpanan fork dan mempertahankan pilihan custom.
- Apakah menu Bedrock memakai sentuh langsung atau virtual mouse sebagai default? Pisahkan pilihan
  itu dari bidikan dunia, dan pastikan format export tidak diberi label kompatibel Zalith jika bukan.
- Perlu verifikasi lisensi sampel Mojang, pemberitahuan/izin unduh, lokasi cache persisten, dan
  kebijakan pembersihan. Unduh yang gagal tidak boleh membuat kontrol default hilang atau game macet.
- Ukuran kerja, compile tiap tahap, performa atlas, dan rasa kontrol masih perkiraan sumber.
  Analisis ini memberi peta port; kesetaraan Bedrock dan kelayakan runtime menunggu implementasi/uji.
