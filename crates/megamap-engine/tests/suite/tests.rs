use crate::engine::baker::{bake_chunk, Layer};
use crate::engine::far::{FarGrid, FarSource};
use crate::engine::radar::Radar;
use crate::engine::region::{Region, SIZE};
use crate::engine::lang::{Lang, Tr};
use crate::engine::settings::{Mode, Settings, Shape};
use crate::engine::store::{self, Store};
use crate::engine::reveal;
use crate::engine::trail::Trail;
use crate::engine::ui::{Action, MapUi, Rect};
use crate::engine::view::{self, Lookup, View};
use crate::engine::waypoints::{self, Kind, Waypoint, Waypoints};
use crate::fake::{table, Plains, World};

fn baked_store(r: i32) -> Store {
    let w = World { r, cave: false };
    let t = table();
    let mut s = Store::new(64);
    for cz in -r..r {
        for cx in -r..r {
            let c = bake_chunk(&w, &Plains, &t, cx, cz, Layer::Surface).unwrap();
            s.put(&c);
        }
    }
    s
}

struct StoreLookup<'a>(&'a Store);
impl Lookup for StoreLookup<'_> {
    fn region(&self, rx: i32, rz: i32) -> Option<&Region> {
        self.0.get(rx, rz)
    }
}

#[test]
fn bake_surface_heights_and_colours() {
    let w = World { r: 2, cave: false };
    let c = bake_chunk(&w, &Plains, &table(), 0, 0, Layer::Surface).unwrap();
    for (i, h) in c.height.iter().enumerate() {
        let (x, z) = ((i % 16) as i32, (i / 16) as i32);
        let top = w.height(x, z);
        let want = if top <= 62 { 61 } else { top - 1 };
        assert_eq!(*h as i32, want, "column {x},{z}");
        assert_ne!(c.color[i] >> 24, 0);
        assert_eq!(crate::engine::color::water_depth(c.color[i]) > 0, top < 62, "material {x},{z}");
    }
    assert!(bake_chunk(&w, &Plains, &table(), 5, 5, Layer::Surface).is_none(), "unloaded chunk baked");
}

#[test]
fn bake_tints_grass_and_water() {
    let w = World { r: 4, cave: false };
    let t = table();
    let mut grass = 0;
    let mut water = 0;
    for cx in -4..4 {
        let c = bake_chunk(&w, &Plains, &t, cx, 0, Layer::Surface).unwrap();
        for p in c.color {
            let (r, g, b) = ((p >> 16) & 255, (p >> 8) & 255, p & 255);
            if g > r && g > b {
                grass += 1;
            }
            if b > r && b > g {
                water += 1;
            }
        }
    }
    assert!(grass > 100, "grass {grass}");
    assert!(water > 10, "water {water}");
}

#[test]
fn bake_cave_layer_finds_floor() {
    let w = World { r: 1, cave: true };
    let c = bake_chunk(&w, &Plains, &table(), 0, 0, Layer::Cave(28, i32::MIN)).unwrap();
    assert!(c.height.iter().all(|&h| h == 19), "{:?}", &c.height[..4]);
}

#[test]
fn region_roundtrip_and_size() {
    let s = baked_store(16); // a full region (0,0) plus parts of three others
    let r = s.get(0, 0).unwrap();
    assert_eq!(r.explored_count(), 256);
    let bytes = r.encode();
    let back = Region::decode(&bytes).unwrap();
    assert_eq!(&back.colors[..], &r.colors[..]);
    assert_eq!(&back.heights[..], &r.heights[..]);
    assert_eq!(back.explored, r.explored);
    let raw = SIZE * SIZE * 6;
    assert!(bytes.len() * 4 < raw, "encoded {} of raw {raw}", bytes.len());
    // An empty region is tiny.
    assert!(Region::new(3, 4).encode().len() < 200);
}

#[test]
fn region_rejects_corruption() {
    let s = baked_store(2);
    let mut bytes = s.get(0, 0).unwrap().encode();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0x40;
    assert!(Region::decode(&bytes).is_err());
    assert!(Region::decode(b"MMR1").is_err());
    assert!(Region::decode(&[0u8; 300]).is_err());
}

#[test]
fn region_put_reports_change_once() {
    let w = World { r: 1, cave: false };
    let c = bake_chunk(&w, &Plains, &table(), 0, 0, Layer::Surface).unwrap();
    let mut r = Region::new(0, 0);
    assert!(r.put_chunk(&c));
    let g = r.generation;
    assert!(!r.put_chunk(&c), "same bake must not count as a change");
    assert_eq!(r.generation, g);
}

#[test]
fn mips_average() {
    let s = baked_store(2);
    let mut r = Region::decode(&s.get(0, 0).unwrap().encode()).unwrap();
    r.ensure_mips();
    assert_eq!(r.level(4).len(), 32 * 32);
    assert_eq!(r.level(4)[0] >> 24, 0xFF); // chunk (0,0) explored
    assert_eq!(r.level(4)[31 * 32 + 31], 0); // far corner not
}

#[test]
fn store_files_roundtrip_and_evict() {
    let dir = std::env::temp_dir().join(format!("megamap-test-{}", std::process::id()));
    let mut s = baked_store(2);
    for ((rx, rz), b) in s.dirty() {
        store::write_region(&dir, rx, rz, &b).unwrap();
    }
    assert!(s.dirty().is_empty(), "saved regions are clean");
    let r = store::read_region(&dir, -1, -1).unwrap().unwrap();
    assert_eq!(r.explored_count(), 4);
    assert!(store::read_region(&dir, 9, 9).unwrap().is_none());
    s.limit = 1;
    let out = s.evict(&[(0, 0)]);
    assert!(out.is_empty(), "clean regions evict without a write");
    assert_eq!(s.regions.len(), 1);
    assert!(s.get(0, 0).is_some());
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(store::sanitize("My World: 1/2"), "my_world__1_2");
    assert_eq!(store::sanitize(".."), "_");
}

#[test]
fn view_north_up_and_rotation() {
    let s = baked_store(4);
    let lk = StoreLookup(&s);
    let (w, h) = (64usize, 64usize);
    let mut a = vec![0u8; w * h * 4];
    let mut b = vec![0u8; w * h * 4];
    let mut v = View::new([0.0, 0.0], 1.0);
    v.fog = false;
    v.style.relief = 0;
    view::render(&mut a, w, h, &v, &lk);
    // Pixel (32+x, 32+y) is block (x, y).
    let r = s.get(0, 0).unwrap();
    let px = |buf: &[u8], x: usize, y: usize| u32::from_le_bytes(buf[(y * w + x) * 4..][..4].try_into().unwrap());
    let rgba = |c: u32| u32::from_le_bytes(crate::engine::color::to_rgba(c));
    assert_eq!(crate::engine::color::water_depth(r.colors[8 * SIZE + 5]), 0, "test block is land");
    assert_eq!(px(&a, 37, 40), rgba(r.colors[8 * SIZE + 5]));
    // A half turn maps block (5, 8) to the mirrored pixel.
    let v2 = View { rotation: std::f64::consts::PI, ..v };
    view::render(&mut b, w, h, &v2, &lk);
    assert_eq!(px(&b, 64 - 37 - 1, 64 - 40 - 1), px(&a, 37, 40));
    // to_image / to_world are inverse.
    let v3 = View { rotation: 0.7, bpp: 2.0, center: [10.0, -3.0], ..v };
    let p = v3.to_image(w, h, 17.0, 5.0);
    let q = v3.to_world(w, h, p[0], p[1]);
    assert!((q[0] - 17.0).abs() < 1e-9 && (q[1] - 5.0).abs() < 1e-9);
}

#[test]
fn view_round_mask_and_fog() {
    let s = baked_store(1);
    let lk = StoreLookup(&s);
    let mut a = vec![0u8; 32 * 32 * 4];
    let v = View { center: [1000.0, 1000.0], bpp: 1.0, rotation: 0.0, round: true, fog: true, style: Default::default() };
    view::render(&mut a, 32, 32, &v, &lk);
    assert_eq!(a[3], 0, "corner outside the circle is transparent");
    let c = (16 * 32 + 16) * 4;
    assert_eq!(a[c + 3], 255, "unexplored centre shows fog");
    let mut regions = Vec::new();
    view::visible_regions(&View { center: [0.0, 0.0], bpp: 4.0, ..v }, 256, 256, &mut regions);
    assert!(regions.contains(&(-1, -1)) && regions.contains(&(0, 0)));
    assert_eq!(view::mip_for(1.0), 0);
    assert_eq!(view::mip_for(4.0), 2);
    assert_eq!(view::mip_for(1000.0), 4);
}

#[test]
fn explored_percent_counts() {
    let s = baked_store(2);
    let lk = StoreLookup(&s);
    let v = View { center: [0.0, 0.0], bpp: 1.0, rotation: 0.0, round: false, fog: true, style: Default::default() };
    let p = view::explored_percent(&v, 64, 64, &lk);
    assert!(p > 99.0, "{p}");
    let v = View { center: [5000.0, 0.0], ..v };
    assert_eq!(view::explored_percent(&v, 64, 64, &lk), 0.0);
}

#[test]
fn far_grid_fills_in_slices() {
    struct Flat;
    impl FarSource for Flat {
        fn surface(&mut self, x: i32, _z: i32) -> Option<(i32, u32)> {
            (x < 0).then_some((70, 0x00FF00))
        }
    }
    let mut g = FarGrid::new(8, 16);
    g.recenter(0, 0);
    let mut n = 0;
    while !g.complete() {
        n += 1;
        g.fill(&mut Flat, 10);
    }
    assert_eq!(n, 26);
    assert_ne!(g.sample(-20, 0), 0);
    assert_eq!(g.sample(20, 0), 0);
    assert_eq!(g.sample(10_000, 0), 0);
}

#[test]
fn waypoints_text_roundtrip_filter_share() {
    let mut w = Waypoints::default();
    let mut a = Waypoint::new("Base\tone", [1, 64, 2], "minecraft:overworld");
    a.color = 3;
    a.icon = 2;
    w.add(a);
    w.add(Waypoint::new("Mine", [100, 12, 100], "minecraft:overworld"));
    w.add(Waypoint::new("Fortress", [10, 70, 10], "minecraft:the_nether"));
    w.add_death([5, 60, 5], "minecraft:overworld", 2, "1");
    w.add_death([6, 60, 6], "minecraft:overworld", 2, "2");
    w.add_death([7, 60, 7], "minecraft:overworld", 2, "3");
    assert_eq!(w.list.iter().filter(|x| x.kind == Kind::Death).count(), 2);
    assert!(!w.list.iter().any(|x| x.name == "Death 1"));
    let back = Waypoints::from_text(&w.to_text());
    assert_eq!(back.list, w.list);
    let ow = w.filter("", Some("minecraft:overworld"), [0.0, 0.0, 0.0]);
    assert_eq!(w.list[ow[0]].name, "Base\tone");
    assert_eq!(w.filter("MIN", None, [0.0; 3]).len(), 1);
    let share = w.list[1].share_text();
    assert_eq!(share, "[megamap] Mine @ 100 12 100 (overworld)");
    let (n, p, d) = waypoints::parse_share(&format!("<Steve> {share}")).unwrap();
    assert_eq!((n.as_str(), p, d.as_str()), ("Mine", [100, 12, 100], "minecraft:overworld"));
    assert_eq!(w.next_name("Waypoint"), "Waypoint 1");
}

#[test]
fn trail_spacing_segments_and_text() {
    let mut t = Trail::new(5, 4);
    assert!(t.push([0, 64, 0]));
    assert!(!t.push([1, 64, 1]));
    assert!(t.push([4, 64, 0]));
    assert!(t.push([1000, 64, 0])); // teleport: new segment
    assert_eq!(t.segments().count(), 1);
    let mut u = Trail::new(5, 4);
    u.from_text(&t.to_text());
    assert_eq!(u.points, t.points);
    for i in 0..10 {
        t.push([2000 + i * 10, 64, 0]);
    }
    assert_eq!(t.points.len(), 5);
}

#[test]
fn radar_counts_hostiles() {
    let mut r = Radar::new(32.0);
    let ents = [
        ("minecraft:zombie", [5.0, 64.0, 5.0], true),
        ("minecraft:creeper", [2.0, 64.0, 2.0], true),
        ("minecraft:cow", [3.0, 64.0, 3.0], true),
        ("minecraft:skeleton", [100.0, 64.0, 0.0], true),
        ("minecraft:zombie", [1.0, 64.0, 1.0], false),
    ];
    r.scan([0.0, 64.0, 0.0], ents.iter().map(|e| (e.0, e.1, e.2)), false);
    assert_eq!(r.hostile_count, 2);
    assert_eq!(r.blips.len(), 2);
    assert!(r.blips.iter().any(|b| b.urgent));
    r.scan([0.0, 64.0, 0.0], ents.iter().map(|e| (e.0, e.1, e.2)), true);
    assert_eq!(r.blips.len(), 3);
}

#[test]
fn settings_roundtrip_and_buttons() {
    let mut s = Settings::default();
    s.shape = Shape::Round;
    s.layer = 2;
    s.mode = Mode::Hidden;
    s.cycle_zoom();
    s.max_deaths = 10;
    assert_eq!(Settings::from_text(&s.to_text()), s);
    assert_eq!(Settings::from_text("zoom=99\nopacity=x\njunk"), Settings { zoom: 4, ..Settings::default() });
    let n = s.buttons().len();
    for i in 0..n {
        let before = s.clone();
        s.click(i);
        assert_ne!(before, s, "button {i} does nothing");
    }
}

#[test]
fn settings_v2_roundtrip_cycles_and_style() {
    use crate::engine::settings::{BUTTON_COUNT, RESET_ALL};
    let d = Settings::default();
    assert_eq!(d.buttons().len(), BUTTON_COUNT);
    // Every new option survives the text form after any number of clicks.
    let mut s = d.clone();
    for round in 0..5 {
        for i in (0..BUTTON_COUNT).filter(|&i| i != RESET_ALL) {
            s.click(i);
            assert_eq!(Settings::from_text(&s.to_text()), s, "button {i}, round {round}");
        }
    }
    // An old 0.1.0 file loads with the new defaults; bad values fall back.
    let old = "shape=round\nzoom=2\nfog=false\n";
    let o = Settings::from_text(old);
    // The old 15 fps cloud default migrates to "as the game".
    assert_eq!((o.relief, o.clouds, o.cloud_fps, o.far), (2, true, 0, true));
    let bad = Settings::from_text("relief=9\ncontours=7\ncloud_density=3\ncloud_fps=999\nui_theme=8");
    assert_eq!((bad.relief, bad.contours, bad.cloud_density, bad.cloud_fps, bad.ui_theme), (3, 0, 85, 0, 2));
    // Reset.
    s.click(RESET_ALL);
    assert_eq!(s, d);
    // Style and cloud params follow the options.
    let mut t = d.clone();
    t.day_night = false;
    t.contours = 16;
    let st = t.style(40, true);
    assert_eq!((st.light, st.contours, st.mark_unknown), (256, 16, true));
    assert_eq!(d.style(40, false).light, 40);
    t.cloud_speed = 0;
    t.cloud_theme = 3;
    let p = t.cloud_params();
    assert_eq!(p.speed, 0.0);
    assert_eq!(p.theme.index(), 3);
}

#[test]
fn ui_context_menu_and_edit_flow() {
    let mut ui = MapUi::new();
    ui.screen = Rect::new(0.0, 0.0, 480.0, 270.0);
    ui.open([0.0, 64.0, 0.0], "minecraft:overworld", false);
    let wps = Waypoints::default();
    // Right click at the centre + (10, 0) opens the menu at block (10, 0).
    assert_eq!(ui.press([250.0, 135.0], 1, &wps), Action::None);
    let m = ui.menu.unwrap();
    assert_eq!(m.world, [10, 0]);
    // "Coordinates to chat".
    let r = ui.menu_item_rect(&m, 1);
    assert_eq!(ui.press([r.x + 1.0, r.y + 1.0], 0, &wps), Action::ChatPrefill("10 ~ 0".into()));
    // "Waypoint here" opens the editor, typing names it, Enter saves.
    ui.press([250.0, 135.0], 1, &wps);
    let m = ui.menu.unwrap();
    let r = ui.menu_item_rect(&m, 0);
    ui.press([r.x + 1.0, r.y + 1.0], 0, &wps);
    assert!(ui.edit.is_some());
    for _ in 0..20 {
        ui.key("key.keyboard.backspace");
    }
    for c in "Camp".chars() {
        assert!(ui.char_typed(c));
    }
    assert_eq!(ui.key("key.keyboard.m"), (true, Action::None), "typing keeps M from closing the map");
    match ui.key("key.keyboard.enter") {
        (true, Action::SaveWaypoint(None, w)) => assert_eq!((w.name.as_str(), w.x, w.z), ("Camp", 10, 0)),
        other => panic!("{other:?}"),
    }
    assert_eq!(ui.key("key.keyboard.escape"), (false, Action::Close));
}

#[test]
fn ui_zoom_keeps_cursor_point_and_drag_pans() {
    let mut ui = MapUi::new();
    ui.open([100.0, 64.0, 100.0], "minecraft:overworld", false);
    let p = [300.0, 50.0];
    let before = ui.world_at(p);
    let wps = Waypoints::default();
    ui.scroll(p, 1.0, &wps);
    let after = ui.world_at(p);
    assert!((before[0] - after[0]).abs() < 1e-9 && (before[1] - after[1]).abs() < 1e-9);
    ui.press([240.0, 135.0], 0, &wps);
    let c = ui.center;
    ui.drag([10.0, 0.0]);
    assert!(ui.center[0] < c[0]);
}

#[test]
fn ui_list_search_and_select() {
    let mut ui = MapUi::new();
    ui.open([0.0, 64.0, 0.0], "minecraft:overworld", true);
    let mut wps = Waypoints::default();
    wps.add(Waypoint::new("Alpha", [50, 64, 50], "minecraft:overworld"));
    wps.add(Waypoint::new("Beta", [-300, 64, 20], "minecraft:overworld"));
    for c in "bet".chars() {
        ui.char_typed(c);
    }
    assert_eq!(ui.filtered(&wps), vec![1]);
    let r = ui.row_rect(0);
    ui.press([r.x + 5.0, r.y + 2.0], 0, &wps);
    assert_eq!(ui.center, [-299.5, 20.5]);
    // A right click on a row: the waypoint's menu (0.3.10), whose first entry opens the editor.
    ui.press([r.x + 5.0, r.y + 2.0], 1, &wps);
    let m = ui.menu.unwrap();
    assert_eq!(m.wp, Some(1));
    let e = ui.menu_item_rect(&m, 0);
    ui.press([e.x + 2.0, e.y + 2.0], 0, &wps);
    assert_eq!(ui.edit.as_ref().unwrap().index, Some(1));
    let d = ui.dialog_button(1);
    assert_eq!(ui.press([d.x + 1.0, d.y + 1.0], 0, &wps), Action::DeleteWaypoint(1));
}

#[test]
fn worker_side_save_helpers() {
    let mut s = baked_store(2);
    let copies = s.dirty_copies();
    assert_eq!(copies.len(), 4);
    assert!(s.dirty_copies().is_empty(), "copies mark regions saved");
    let a = copies.iter().find(|r| (r.rx, r.rz) == (0, 0)).unwrap();
    assert_eq!(a.encode(), s.get(0, 0).unwrap().encode());
    let mut c = baked_store(2);
    c.limit = 1;
    let out = c.evict_regions(&[(0, 0)]);
    assert_eq!(out.len(), 3, "unsaved victims are handed over whole");
    assert!(c.get(0, 0).is_some());
}

// --- v2 -------------------------------------------------------------------------------------------------------

use crate::engine::clouds::{self, Clouds, Params, Theme};
use crate::engine::color;
use crate::engine::far::{bake_surfaces, Surface, TintTable};
use crate::engine::hlod::{self, HlodRegion};
use std::io::{Cursor, Read, Write};

fn inflate(data: &[u8], out: &mut Vec<u8>, limit: usize) -> bool {
    out.clear();
    flate2::read::DeflateDecoder::new(data).take(limit as u64).read_to_end(out).is_ok()
}

fn deflate(raw: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
    e.write_all(raw).unwrap();
    e.finish().unwrap()
}

/// A payload in Horizon's cache layout: column i has height 60 + i % 7, water over every 5th, one span each.
fn payload() -> Vec<u8> {
    let mut p = Vec::new();
    for i in 0..256u32 {
        let water = i % 5 == 0;
        p.push(if water { 2 } else { 1 });
        p.extend_from_slice(&(60 + (i % 7) as i32).to_le_bytes());
        p.extend_from_slice(&crate::fake::GRASS.to_le_bytes());
        p.extend_from_slice(&1u16.to_le_bytes());
        if water {
            p.extend_from_slice(&66i32.to_le_bytes());
            p.extend_from_slice(&crate::fake::WATER.to_le_bytes());
        }
        p.extend_from_slice(&1u16.to_le_bytes());
        p.extend_from_slice(&(-10i16).to_le_bytes());
        p.extend_from_slice(&(40i16).to_le_bytes());
        p.extend_from_slice(&crate::fake::STONE.to_le_bytes());
        p.extend_from_slice(&1u16.to_le_bytes());
        p.extend_from_slice(&0u16.to_le_bytes());
    }
    p
}

fn hlod_file(records: &[(i32, i32, Vec<u8>)], torn: bool) -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(b"HLOD");
    f.extend_from_slice(&1u32.to_le_bytes());
    f.extend_from_slice(&0xABCDu64.to_le_bytes());
    for (x, z, raw) in records {
        let d = deflate(raw);
        f.extend_from_slice(&x.to_le_bytes());
        f.extend_from_slice(&z.to_le_bytes());
        f.extend_from_slice(&(d.len() as u32).to_le_bytes());
        f.extend_from_slice(&d);
    }
    if torn {
        f.extend_from_slice(&5i32.to_le_bytes());
        f.extend_from_slice(&5i32.to_le_bytes());
        f.extend_from_slice(&1000u32.to_le_bytes());
        f.extend_from_slice(&[1, 2, 3]);
    }
    f
}

#[test]
fn hlod_reads_records_and_survives_damage() {
    let good = payload();
    let mut other = payload();
    other[1] = 99; // first column height 99 + ...
    let file = hlod_file(&[(3, 4, good.clone()), (7, 1, good.clone()), (3, 4, other)], true);
    let mut r = HlodRegion::open(Cursor::new(file)).unwrap();
    assert_eq!(r.key, 0xABCD);
    assert_eq!(r.len(), 2, "torn tail ignored, repeated chunk counted once");
    let (mut buf, mut raw) = (Vec::new(), Vec::new());
    let mut out = [None; 256];
    assert!(r.read(3, 4, inflate, &mut buf, &mut raw, &mut out));
    // The later record wins.
    assert_eq!(out[0].unwrap().height & 0xFF, 99);
    assert!(r.read(7, 1, inflate, &mut buf, &mut raw, &mut out));
    assert_eq!(out[1], Some(Surface { height: 61, state: crate::fake::GRASS, biome: 1, water: None }));
    assert_eq!(out[5].unwrap().water, Some((66, crate::fake::WATER)));
    assert!(!r.read(5, 5, inflate, &mut buf, &mut raw, &mut out));
    // Damaged payloads are refused, not half read.
    let mut short = good.clone();
    short.pop();
    assert!(!hlod::parse(&short, &mut out));
    let mut long = good.clone();
    long.push(0);
    assert!(!hlod::parse(&long, &mut out));
    let mut bad_tag = good;
    bad_tag[0] = 3;
    assert!(!hlod::parse(&bad_tag, &mut out));
    assert!(HlodRegion::open(Cursor::new(b"HLOD\x02\0\0\0........".to_vec())).is_none());
    assert_eq!(hlod::file_name(-1, 33), "r.-1.1.hlod");
}

/// Reads the real cache files of the local saves when there are any (skips otherwise).
#[test]
fn hlod_reads_real_cache_if_present() {
    let Some(home) = std::env::var_os("HOME") else { return };
    let saves = std::path::Path::new(&home).join(".local/share/horizon/saves");
    let Ok(worlds) = std::fs::read_dir(&saves) else { return };
    let (mut buf, mut raw) = (Vec::new(), Vec::new());
    let mut out = [None; 256];
    let (mut files, mut chunks, mut cols) = (0, 0, 0);
    for w in worlds.flatten() {
        let dir = hlod::dir_of(&w.path().join("dimensions/minecraft/overworld"));
        let Ok(list) = std::fs::read_dir(&dir) else { continue };
        for f in list.flatten().take(3) {
            let Ok(file) = std::fs::File::open(f.path()) else { continue };
            let Some(mut r) = HlodRegion::open(std::io::BufReader::new(file)) else { continue };
            files += 1;
            let keys: Vec<_> = r.chunks().take(20).collect();
            for (cx, cz) in keys {
                assert!(r.read(cx, cz, inflate, &mut buf, &mut raw, &mut out), "chunk {cx},{cz} of {:?}", f.path());
                chunks += 1;
                cols += out.iter().flatten().count();
            }
        }
    }
    eprintln!("real hlod: {files} files, {chunks} chunks, {cols} columns");
    if chunks > 0 {
        assert!(cols > chunks * 16, "almost no columns decoded");
    }
}

#[test]
fn far_surfaces_bake_like_live_chunks() {
    let t = table();
    let tints = TintTable { tints: vec![(0, 0, 0), (0x91BD59, 0x77AB2F, 0x3F76E4)] };
    let mut cols = [None; 256];
    hlod::parse(&payload(), &mut cols);
    let c = bake_surfaces(2, -1, &cols, &t, &tints).unwrap();
    assert_eq!(c.height[1], 60, "block below the face");
    assert_eq!(color::water_depth(c.color[0]), 6, "water face 66 over ground 60");
    assert_eq!(color::water_depth(c.color[1]), 0);
    assert!(bake_surfaces(0, 0, &[None; 256], &t, &tints).is_none());
    // A far region renders where no live region is.
    let mut far = Region::new(0, -1);
    far.put_chunk(&c);
    struct FarOnly(Region);
    impl Lookup for FarOnly {
        fn region(&self, _: i32, _: i32) -> Option<&Region> {
            None
        }
        fn far_region(&self, rx: i32, rz: i32) -> Option<&Region> {
            ((rx, rz) == (0, -1)).then_some(&self.0)
        }
    }
    let mut img = vec![0u8; 16 * 16 * 4];
    let v = View::new([2.0 * 16.0 + 8.0, -8.0], 1.0);
    let lk = FarOnly(far);
    view::render(&mut img, 16, 16, &v, &lk);
    assert!(img.chunks(4).all(|p| p[3] == 255));
    assert_ne!(&img[..3], &[0x1B, 0x1E, 0x24], "not fog");
    // For the cloud pass far terrain is not seen ground: marked with FAR_ALPHA (live tiles and the coarse grid).
    let mut v = v;
    v.style.mark_unknown = true;
    view::render(&mut img, 16, 16, &v, &lk);
    assert!(img.chunks(4).all(|p| p[3] == view::FAR_ALPHA));
    struct Grid;
    impl Lookup for Grid {
        fn region(&self, _: i32, _: i32) -> Option<&Region> {
            None
        }
        fn far(&self, x: i32, _: i32) -> u32 {
            if x >= 0 { 0xFF40_8040 } else { 0 }
        }
    }
    let v = View { center: [0.0, 0.0], ..v };
    view::render(&mut img, 16, 16, &v, &Grid);
    assert!(img.chunks(4).enumerate().all(|(i, p)| p[3] == if i % 16 >= 8 { view::FAR_ALPHA } else { 0 }));
}

#[test]
fn relief_water_and_unknown_marking() {
    let s = baked_store(4);
    let lk = StoreLookup(&s);
    let (w, h) = (64usize, 64usize);
    let mut flat = vec![0u8; w * h * 4];
    let mut lit = vec![0u8; w * h * 4];
    let mut v = View::new([0.0, 0.0], 1.0);
    v.style.relief = 0;
    view::render(&mut flat, w, h, &v, &lk);
    v.style.relief = 3;
    view::render(&mut lit, w, h, &v, &lk);
    assert_ne!(flat, lit, "relief changes the hills");
    // Night darkens.
    let mut night = vec![0u8; w * h * 4];
    v.style.light = 40;
    view::render(&mut night, w, h, &v, &lk);
    let sum = |b: &[u8]| b.iter().map(|&x| x as u64).sum::<u64>();
    assert!(sum(&night) < sum(&lit));
    // Unknown ground comes out with alpha 0 for the cloud pass.
    let mut v = View::new([5000.0, 0.0], 1.0);
    v.style.mark_unknown = true;
    view::render(&mut night, w, h, &v, &lk);
    assert!(night.chunks(4).all(|p| p[3] == 0));
    // Deep water darker than shallow.
    let tone = |d: u32| color::shade(color::water(0x4040FF, d) | color::OPAQUE, 268 - d * 9) & 0xFF;
    assert!(tone(12) < tone(2));
    assert_eq!(color::water_depth(color::average4([color::water(1, 4), color::water(1, 6), color::OPAQUE, 0])), 5);
    assert_eq!(color::water_depth(color::average4([color::water(1, 4), color::OPAQUE, color::OPAQUE, 0])), 0);
}

#[test]
fn clouds_cover_unknown_and_stay_cheap() {
    let s = baked_store(2);
    let lk = StoreLookup(&s);
    let (w, h) = (160usize, 120usize);
    let mut img = vec![0u8; w * h * 4];
    let mut v = View::new([0.0, 0.0], 1.0);
    v.style.mark_unknown = true;
    view::render(&mut img, w, h, &v, &lk);
    let known_before: Vec<u8> = img.chunks(4).filter(|p| p[3] != 0).flat_map(|p| p.to_vec()).collect();
    assert!(!known_before.is_empty() && img.chunks(4).any(|p| p[3] == 0));
    let mut c = Clouds::new();
    let p = Params { density: 1.0, ..Params::default() };
    assert!(c.update(&img, w, h, &v, 3.0, &p));
    let mut a = img.clone();
    c.composite(&mut a, w, h, &p, 3.0);
    assert!(a.chunks(4).all(|p| p[3] == 255), "composite leaves the image opaque");
    // Deep inside the unexplored area (corner) the cloud is light: brighter than the fog tone.
    let corner = &a[..4];
    assert!(corner[0] as u32 + corner[1] as u32 + corner[2] as u32 > 0x1B + 0x1E + 0x24 + 60, "{corner:?}");
    // The clouds move.
    let mut b = img.clone();
    c.update(&img, w, h, &v, 60.0, &p);
    c.composite(&mut b, w, h, &p, 60.0);
    assert_ne!(a, b);
    // Nothing unknown: nothing to do.
    let mut v2 = View::new([0.0, 0.0], 0.25);
    v2.style.mark_unknown = true;
    view::render(&mut img, w, h, &v2, &lk);
    assert!(!c.update(&img, w, h, &v2, 3.0, &p));
    // Palettes follow time and weather.
    let day = clouds::palette(&Params { day_time: 6000, ..p });
    let night = clouds::palette(&Params { day_time: 18000, ..p });
    assert!(day.lit[0] > night.lit[0] + 80.0);
    let storm = clouds::palette(&Params { day_time: 6000, rain: 1.0, thunder: 1.0, ..p });
    assert!(storm.lit[2] < day.lit[2]);
    let calm = clouds::palette(&Params { day_time: 6000, rain: 1.0, weather: false, ..p });
    assert_eq!(calm, day);
    assert_eq!(Theme::from_index(Theme::Storm.index()), Theme::Storm);
    for t in [0u32, 6000, 12000, 18000] {
        let (dx, dz) = clouds::sun_dir(t);
        assert!(((dx * dx + dz * dz) - 1.0).abs() < 1e-4);
    }
    let n = clouds::fbm(0.3, 7.1, 2.0, 4);
    assert!((0.0..=1.0).contains(&n));
}

#[test]
fn far_spiral_covers_square_once_nearest_first() {
    use crate::engine::far::Spiral;
    let mut seen = std::collections::HashSet::new();
    let mut last_ring = 0;
    for p in Spiral::new([5, -3], 4) {
        let ring = (p[0] - 5).abs().max((p[1] + 3).abs());
        assert!(ring >= last_ring && ring <= 4);
        last_ring = ring;
        assert!(seen.insert(p));
    }
    assert_eq!(seen.len(), 81);
    let mut s = Spiral::new([0, 0], 1);
    assert_eq!(s.by_ref().count(), 9);
    assert!(s.finished());
    s.restart([1, 1]);
    assert_eq!(s.next(), Some([1, 1]));
}

#[test]
fn far_mca_header_presence() {
    use crate::engine::far::mca_present;
    let mut h = vec![0u8; 4096];
    // slot 0: offset 2, 1 sector; slot 33 (x 1, z 1): offset 0x010203, 2 sectors; slot 5: offset 1 (inside the
    // header, damaged); slot 7: offset set but zero sectors.
    h[0..4].copy_from_slice(&[0, 0, 2, 1]);
    h[33 * 4..33 * 4 + 4].copy_from_slice(&[1, 2, 3, 2]);
    h[5 * 4..5 * 4 + 4].copy_from_slice(&[0, 0, 1, 1]);
    h[7 * 4..7 * 4 + 4].copy_from_slice(&[0, 0, 9, 0]);
    let p = mca_present(&h);
    assert_eq!(p.iter().filter(|b| **b).count(), 2);
    assert!(p[0] && p[33] && !p[5] && !p[7]);
    // A short file holds nothing.
    assert!(mca_present(&h[..100]).iter().all(|b| !b));
    let dir = std::env::temp_dir().join(format!("megamap-mca-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("r.0.0.mca"), &h).unwrap();
    assert_eq!(crate::engine::far::read_mca_present(&dir.join("r.0.0.mca")), Some(p));
    assert_eq!(crate::engine::far::read_mca_present(&dir.join("r.9.9.mca")), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn far_plan_skips_explored_and_done_nearest_first() {
    use crate::engine::far::{plan_region, regions_around, sort_tasks, SRC_SAVE, SRC_TERRAIN};
    let mut a = [false; 1024];
    let mut b = [false; 1024];
    for i in 0..1024 {
        a[i] = i % 2 == 0;
        b[i] = i < 512;
    }
    let mut skip = [0u64; 16];
    skip[0] = 1; // chunk (0, 0) explored on the map already
    let focus = [-40, 10];
    let radius = 50;
    let done = |c: [i32; 2]| c == [-30, 8];
    let mut tasks = Vec::new();
    for key in regions_around(focus, radius) {
        plan_region(key, &[(SRC_TERRAIN, a), (SRC_SAVE, b)], &skip, focus, radius, &done, &mut tasks);
    }
    sort_tasks(&mut tasks, focus);
    assert!(!tasks.is_empty());
    let mut last = 0;
    let mut seen = std::collections::HashSet::new();
    for t in &tasks {
        let d = crate::engine::far::dist2(t.chunk, focus);
        assert!(d >= last && d <= (radius * radius) as i64);
        last = d;
        assert!(seen.insert(t.chunk));
        assert_ne!(t.chunk, [-30, 8]);
        let (lx, lz) = (t.chunk[0].rem_euclid(32) as usize, t.chunk[1].rem_euclid(32) as usize);
        let i = lz * 32 + lx;
        assert!(!(lx == 0 && lz == 0), "explored chunks are skipped");
        let want = if a[i] { SRC_TERRAIN } else { 0 } | if b[i] { SRC_SAVE } else { 0 };
        assert_eq!(t.sources, want);
    }
    // Regions come nearest first, and cover the square around the focus.
    let r = regions_around([0, 0], 40);
    assert_eq!(r[0..4].iter().filter(|k| [(0, 0), (-1, 0), (0, -1), (-1, -1)].contains(k)).count(), 4);
    assert_eq!(r.len(), 16);
}

#[test]
fn far_spread_grid_fills_chunk_from_coarse_cells() {
    use crate::engine::far::{spread_grid, Surface};
    let side = 8usize;
    let stride = 8;
    let (x0, z0) = (-64, 64);
    let grid: Vec<Option<Surface>> = (0..side * side)
        .map(|i| (i % 3 != 0).then_some(Surface { height: i as i32, state: 1, biome: 0, water: None }))
        .collect();
    let mut out = [None; 256];
    // Chunk (-3, 5) starts at block (-48, 80): grid cells x 2..3, z 2..3.
    assert!(spread_grid(-3, 5, x0, z0, stride, side, &grid, &mut out));
    for lz in 0..16 {
        for lx in 0..16 {
            let (ix, iz) = ((-48 + lx - x0) / stride, (80 + lz - z0) / stride);
            let want = grid[(iz * side as i32 + ix) as usize];
            assert_eq!(out[(lz * 16 + lx) as usize], want);
        }
    }
    // Outside the grid: nothing.
    assert!(!spread_grid(10, 10, x0, z0, stride, side, &grid, &mut out));
    assert!(out.iter().all(|s| s.is_none()));
}

#[test]
fn explored_mask_reads_only_the_header() {
    crate::engine::region::set_block_stamp(TEST_STAMP);
    let w = World { r: 128, cave: false };
    let t = table();
    let mut r = Region::new(-1, 2);
    for (cx, cz) in [(-32, 64), (-1, 95), (-20, 70)] {
        r.put_chunk(&bake_chunk(&w, &Plains, &t, cx, cz, Layer::Surface).unwrap());
    }
    let dir = std::env::temp_dir().join(format!("megamap-mask-{}", std::process::id()));
    store::write_region(&dir, -1, 2, &r.encode()).unwrap();
    let m = store::explored_mask(&dir, -1, 2, false);
    assert_eq!(m, r.explored);
    assert_eq!(m.iter().map(|w| w.count_ones()).sum::<u32>(), 3);
    assert_eq!(store::explored_mask(&dir, 5, 5, false), [0; 16]);
    // A far layer without block ids (before 0.3.5) does not count when they are asked for: it is baked again.
    assert_eq!(store::explored_mask(&dir, -1, 2, true), r.explored, "v3 with ids counts");
    let mut old = r.clone_for_test();
    old.blocks = None;
    store::write_region(&dir, -1, 2, &old.encode()).unwrap();
    assert_eq!(store::explored_mask(&dir, -1, 2, false), r.explored);
    assert_eq!(store::explored_mask(&dir, -1, 2, true), [0; 16], "v2: baked again");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn store_never_evicts_a_region_still_loading() {
    let w = World { r: 64, cave: false };
    let t = table();
    let mut s = Store::new(1);
    s.put(&bake_chunk(&w, &Plains, &t, 0, 0, Layer::Surface).unwrap());
    s.pending_load.insert((0, 0));
    s.put(&bake_chunk(&w, &Plains, &t, 40, 0, Layer::Surface).unwrap());
    s.pending_load.insert((1, 0));
    // Both are being read: nothing goes (saving either would write over its file with one chunk).
    assert!(s.evict_regions(&[]).is_empty());
    assert_eq!(s.regions.len(), 2);
    s.pending_load.remove(&(0, 0));
    let out = s.evict_regions(&[]);
    assert_eq!(out.len(), 1);
    assert_eq!((out[0].rx, out[0].rz), (0, 0));
}


#[test]
fn settings_v3_migration_info_and_entities() {
    use crate::engine::entities::{self, Category};
    use crate::engine::settings::{Info, MAP_TAB_FIRST, BUTTON_COUNT, VERSION};
    // A v2 file: separate show_* flags, passive radar on, a 15 fps cloud cap, the old frame style.
    let old = "shape=round\ncoords=false\nlight=true\nradar_passive=true\ncloud_fps=15\nframe_style=2\n";
    let o = Settings::from_text(old);
    assert!(!o.shows(Info::Coords));
    assert!(o.shows(Info::Light));
    assert!(o.shows(Info::Biome));
    assert_eq!(o.cloud_fps, 0);
    assert!(o.entity_style("minecraft:cow").mini);
    assert!(o.to_text().starts_with(&format!("version={VERSION}")));
    // A v3 file keeps its cloud cap and its info order; unknown and duplicate entries are dropped,
    // missing kinds are appended switched off.
    let v3 = "version=3\ncloud_fps=15\ninfo=time:on,coords:off,bogus:on,time:off\n";
    let n = Settings::from_text(v3);
    assert_eq!(n.cloud_fps, 15);
    assert_eq!(n.info.len(), Info::ALL.len());
    assert_eq!(n.info[0], (Info::Time, true));
    assert_eq!(n.info[1], (Info::Coords, false));
    assert!(n.info[2..].iter().all(|&(_, on)| !on));
    assert_eq!(Settings::from_text(&n.to_text()), n);
    // Reordering.
    let mut m = n.clone();
    assert!(m.info_move(1, true));
    assert_eq!(m.info[0].0, Info::Coords);
    assert!(!m.info_move(0, true));
    assert!(!m.info_move(m.info.len() - 1, false));
    // Entity styles: an override survives the text form, setting the default back drops it.
    let mut e = Settings::default();
    let mut st = e.entity_style("minecraft:zombie");
    assert_eq!(st, entities::default_style("minecraft:zombie"));
    st.color = 0x12AB34;
    st.icon = 4;
    st.size = 3;
    st.mini = false;
    e.set_entity_style("minecraft:zombie", st);
    let back = Settings::from_text(&e.to_text());
    assert_eq!(back.entity_style("minecraft:zombie"), st);
    assert_eq!(back, e);
    e.set_entity_style("minecraft:zombie", entities::default_style("minecraft:zombie"));
    assert!(e.entities.is_empty());
    // Tabs cover every button once.
    let (a, b) = (Settings::tab_items(0), Settings::tab_items(1));
    assert!(a.contains(&0) && b.contains(&MAP_TAB_FIRST) && !a.contains(&MAP_TAB_FIRST));
    let mut all: Vec<usize> = a.iter().chain(b.iter()).copied().collect();
    all.sort();
    all.dedup();
    assert_eq!(all, (0..BUTTON_COUNT).collect::<Vec<_>>());
    // Catalog and search.
    assert_eq!(entities::category("minecraft:creeper"), Category::Hostile);
    assert_eq!(entities::category("minecraft:cow"), Category::Passive);
    let seen = vec!["mymod:weird_thing".to_string()];
    let l = entities::list(&seen, "", None);
    assert!(l.iter().any(|k| k == "mymod:weird_thing"));
    assert!(l.iter().any(|k| k == "minecraft:creeper"));
    let z = entities::list(&seen, "zomb", Some(Category::Hostile));
    assert!(!z.is_empty() && z.iter().all(|k| k.contains("zomb")));
    for icon in 0..entities::ICONS.len() as u8 {
        assert!(!entities::icon_rects(icon, [10.0, 10.0], 4.0).is_empty(), "icon {icon}");
    }
}

#[test]
fn cloud_looks_lightning_and_quality() {
    use crate::engine::clouds::{self, Clouds, Look, Params, Theme};
    use crate::engine::view::View;
    assert_eq!(Look::of_dimension("minecraft:the_nether"), Look::Nether);
    assert_eq!(Look::of_dimension("minecraft:the_end"), Look::End);
    assert_eq!(Look::of_dimension("minecraft:overworld"), Look::Overworld);
    assert_eq!(Look::of_dimension("mymod:deep_end"), Look::Overworld);
    let s = Settings::default();
    assert_eq!(s.cloud_params_for("minecraft:the_nether").look, Look::Nether);
    let (w, h) = (96usize, 96usize);
    let blank = vec![0u8; w * h * 4];
    let view = View::new([0.0, 0.0], 2.0);
    let mean = |img: &[u8]| {
        let mut m = [0f64; 3];
        for px in img.chunks_exact(4) {
            for c in 0..3 {
                m[c] += px[c] as f64;
            }
        }
        m.map(|v| v / (img.len() / 4) as f64)
    };
    let run = |p: &Params, t: f32| {
        let mut c = Clouds::new();
        let mut img = blank.clone();
        assert!(c.update(&img, w, h, &view, t, p));
        c.composite(&mut img, w, h, p, t);
        assert!(img.chunks_exact(4).all(|px| px[3] == 255));
        img
    };
    for q in 0..4 {
        run(&Params { quality: q, density: 1.0, ..Default::default() }, 2.0);
    }
    // Nether is red, End is violet (blue over green), overworld day is near white-grey.
    let n = mean(&run(&Params { look: Look::Nether, density: 1.0, ..Default::default() }, 1.0));
    assert!(n[0] > n[1] * 1.5 && n[0] > n[2] * 1.5, "nether {n:?}");
    let e = mean(&run(&Params { look: Look::End, density: 1.0, ..Default::default() }, 1.0));
    assert!(e[2] > e[1] && e[0] > e[1], "end {e:?}");
    // The Nether ignores the clock: noon and midnight give the same palette.
    let pn = Params { look: Look::Nether, day_time: 6000, ..Default::default() };
    assert_eq!(clouds::palette(&pn), clouds::palette(&Params { day_time: 18000, ..pn }));
    // Lightning brightens a storm frame at some point; with lightning off it never does.
    let storm = Params { theme: Theme::Storm, density: 1.0, lightning_rate: 2.5, ..Default::default() };
    let mut c = Clouds::new();
    let mut c_off = Clouds::new();
    let mut lit = false;
    for k in 0..400 {
        let t = k as f32 * 0.05;
        let mut a = blank.clone();
        let mut b = blank.clone();
        c.update(&a, w, h, &view, t, &storm);
        c.composite(&mut a, w, h, &storm, t);
        let off = Params { lightning: 0, ..storm };
        c_off.update(&b, w, h, &view, t, &off);
        c_off.composite(&mut b, w, h, &off, t);
        let (ma, mb) = (mean(&a), mean(&b));
        if ma[0] + ma[1] + ma[2] > mb[0] + mb[1] + mb[2] + 6.0 {
            lit = true;
            break;
        }
    }
    assert!(lit, "no lightning flash in 20 s of a storm");
    // Explored pixels under the feather keep most of their colour.
    let mut img = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w / 2 {
            let i = (y * w + x) * 4;
            img[i..i + 4].copy_from_slice(&[10, 200, 10, 255]);
        }
    }
    let p = Params { density: 1.0, ..Default::default() };
    let mut c = Clouds::new();
    c.update(&img, w, h, &view, 1.0, &p);
    c.composite(&mut img, w, h, &p, 1.0);
    let far = (h / 2 * w + 4) * 4;
    assert!(img[far + 1] > 150, "known ground far from the edge is veiled: {:?}", &img[far..far + 4]);
}

#[test]
fn ui_settings_entities_and_dimension_panels() {
    use crate::engine::settings::{Settings, INFO_FIRST};
    use crate::engine::ui::{EntityField, PanelKind, BUTTONS};
    let mut ui = MapUi::new();
    ui.screen = Rect::new(0.0, 0.0, 480.0, 270.0);
    ui.open([0.0, 64.0, 0.0], "minecraft:overworld", false);
    let wps = Waypoints::default();
    let at = |r: Rect| [r.x + r.w / 2.0, r.y + r.h / 2.0];
    let button = |k: PanelKind| {
        BUTTONS
            .iter()
            .position(|(b, _, _)| {
                matches!(
                    (b, k),
                    (crate::engine::ui::Button::Settings, PanelKind::Settings)
                        | (crate::engine::ui::Button::Entities, PanelKind::Entities)
                        | (crate::engine::ui::Button::Dimension, PanelKind::Dimensions)
                )
            })
            .unwrap()
    };
    // Settings: categories on the left, headers + items + "reset category" on the right, a search over all.
    use crate::engine::settings::{Row, CATEGORIES};
    let labels = Settings::default().buttons();
    ui.settings_cat = 0;
    ui.press(at(ui.button_rect(button(PanelKind::Settings))), 0, &wps);
    ui.refresh_settings_rows(&labels);
    assert!(matches!(ui.settings_rows[0], Row::Header(_)));
    assert_eq!(ui.press(at(ui.panel_row_rect(0)), 0, &wps), Action::None, "a header does nothing");
    let Row::Item(first) = ui.settings_rows[1] else { panic!() };
    assert_eq!(ui.press(at(ui.panel_row_rect(1)), 0, &wps), Action::SettingClick(first));
    // The rows sit right of the nav and left of the preview, and do not overlap them.
    let (nav, row, pre) = (ui.settings_nav_rect(0).unwrap(), ui.panel_row_rect(0), ui.settings_preview_rect().unwrap());
    assert!(nav.x + nav.w <= row.x && row.x + row.w <= pre.x, "{nav:?} {row:?} {pre:?}");
    // Nav: the second category (info lines) is chosen and remembered.
    assert_eq!(ui.press(at(ui.settings_nav_rect(1).unwrap()), 0, &wps), Action::SettingsCat(1));
    ui.refresh_settings_rows(&labels);
    let info_row = ui.settings_rows.iter().position(|r| *r == Row::Item(INFO_FIRST + 1)).unwrap();
    ui.panel.as_mut().unwrap().scroll = info_row;
    assert_eq!(ui.press(at(ui.panel_row_box(0, 0)), 0, &wps), Action::InfoMove(1, true));
    assert_eq!(ui.press(at(ui.panel_row_box(0, 1)), 0, &wps), Action::InfoMove(1, false));
    ui.cursor = at(ui.panel_row_rect(0));
    assert_eq!(ui.settings_hover(), Some(INFO_FIRST + 1));
    let last = ui.settings_rows.len() - 1;
    ui.panel.as_mut().unwrap().scroll = last;
    assert_eq!(ui.press(at(ui.panel_row_rect(0)), 0, &wps), Action::ResetCategory(1));
    // Preview weather switch: local to the preview.
    assert_eq!(ui.press(at(ui.settings_weather_rect(2).unwrap()), 0, &wps), Action::None);
    assert_eq!(ui.panel.as_ref().unwrap().preview_weather, 2);
    // Map tab: jumps to its first category.
    let first_map = CATEGORIES.iter().position(|c| c.tab == 1).unwrap();
    assert_eq!(ui.press(at(ui.panel_tab_rect(1)), 0, &wps), Action::SettingsCat(first_map));
    // Search: typed into the settings search box, spans every category.
    ui.press(at(ui.panel_search_rect().unwrap()), 0, &wps);
    for c in "отдал".chars() {
        assert!(ui.char_typed(c));
    }
    ui.tr = Tr::new(Lang::Ru);
    ui.refresh_settings_rows(&Settings::default().buttons_in(&ui.tr));
    assert_eq!(ui.settings_rows, vec![Row::Item(47)]);
    assert_eq!(ui.press(at(ui.panel_row_rect(0)), 0, &wps), Action::SettingClick(47));
    // Reopening remembers the category.
    ui.toggle_panel(PanelKind::Settings);
    ui.toggle_panel(PanelKind::Settings);
    assert_eq!(ui.panel.as_ref().unwrap().cat, first_map);
    // Entities: the boxes toggle, a click on the name opens the details.
    ui.press(at(ui.button_rect(button(PanelKind::Entities))), 0, &wps);
    assert_eq!(ui.panel.as_ref().unwrap().kind, PanelKind::Entities);
    ui.panel_rows = vec!["minecraft:zombie".into(), "minecraft:cow".into()];
    let z = "minecraft:zombie".to_string();
    assert_eq!(ui.press(at(ui.panel_row_box(0, 0)), 0, &wps), Action::EntityEdit(z.clone(), EntityField::Mini));
    assert_eq!(ui.press(at(ui.panel_row_box(0, 1)), 0, &wps), Action::EntityEdit(z.clone(), EntityField::Map));
    assert_eq!(ui.press(at(ui.panel_row_rect(0)), 0, &wps), Action::None);
    assert_eq!(ui.panel.as_ref().unwrap().selected.as_deref(), Some("minecraft:zombie"));
    assert_eq!(ui.press(at(ui.panel_row_rect(3)), 0, &wps), Action::EntityEdit(z.clone(), EntityField::Color));
    // The 3D preview turns with a drag on it and leaves the map where it was.
    let pr = ui.entity_preview_rect().expect("preview fits");
    let center = ui.center;
    assert_eq!(ui.press(at(pr), 0, &wps), Action::None);
    assert!(ui.spin_drag && !ui.dragging);
    ui.drag([10.0, 4.0]);
    assert_eq!(ui.center, center);
    assert!((ui.spin_yaw - 15.0).abs() < 1e-4 && (ui.spin_pitch - 4.0).abs() < 1e-4);
    ui.release(0);
    assert!(!ui.spin_drag);
    // Escape goes back to the list, then closes the panel.
    ui.key("key.keyboard.escape");
    assert!(ui.panel.as_ref().unwrap().selected.is_none());
    ui.key("key.keyboard.escape");
    assert!(ui.panel.is_none());
    // Dimensions: row 0 is the player's own, others name a folder; the choice closes the panel.
    ui.press(at(ui.button_rect(button(PanelKind::Dimensions))), 0, &wps);
    ui.panel_rows = vec!["minecraft_overworld".into(), "minecraft_the_nether".into()];
    assert_eq!(ui.press(at(ui.panel_row_rect(1)), 0, &wps), Action::ViewDimension(Some("minecraft_the_nether".into())));
    assert!(ui.panel.is_none());
    ui.press(at(ui.button_rect(button(PanelKind::Dimensions))), 0, &wps);
    ui.panel_rows = vec!["minecraft_overworld".into(), "minecraft_the_nether".into()];
    assert_eq!(ui.press(at(ui.panel_row_rect(0)), 0, &wps), Action::ViewDimension(None));
}

#[test]
fn settings_info_move_swaps_neighbours() {
    let mut s = crate::engine::settings::Settings::default();
    let before = s.info.clone();
    assert!(!s.info_move(0, true));
    assert!(s.info_move(0, false));
    assert_eq!(s.info[0], before[1]);
    assert_eq!(s.info[1], before[0]);
    assert!(!s.info_move(s.info.len() - 1, false));
}

/// A map whose explored part ends in a chunk staircase (12 px steps), unknown pixels filled with `unk`.
fn staircase(w: usize, h: usize, unk: [u8; 3]) -> Vec<u8> {
    let mut img = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let px = &mut img[(y * w + x) * 4..][..4];
            if x / 12 + y / 12 < 22 {
                px.copy_from_slice(&[40, 120, 30, 255]);
            } else {
                px.copy_from_slice(&[unk[0], unk[1], unk[2], 0]);
            }
        }
    }
    img
}

#[test]
fn clouds_lie_over_the_border_without_steps() {
    use crate::engine::clouds::{Clouds, Params};
    let (w, h) = (320usize, 240usize);
    let v = View::new([0.0, 0.0], 3.0);
    for q in 0..4u8 {
        // A wide reach (40 px here: 120 blocks, past the slider, as the 0.3.2 pixel floor was), so the mask
        // smoothness is checked over a real feather; narrow reaches are checked against their own fade below.
        let p = Params { quality: q, density: 0.85, reach: 120.0, ..Params::default() };
        let img_a = staircase(w, h, [0, 0, 0]);
        let img_b = staircase(w, h, [255, 0, 255]);
        let mut c = Clouds::new();
        assert!(c.update(&img_a, w, h, &v, 5.0, &p));
        // Every unexplored pixel is fully hidden: its own colour never shows.
        let (mut a, mut b) = (img_a.clone(), img_b.clone());
        c.composite(&mut a, w, h, &p, 5.0);
        c.composite(&mut b, w, h, &p, 5.0);
        for i in (0..w * h * 4).step_by(4) {
            if img_a[i + 3] == 0 {
                for k in 0..3 {
                    assert!((a[i + k] as i32 - b[i + k] as i32).abs() <= 1, "q{q} unknown pixel {} shows through", i / 4);
                }
            }
        }
        // The coverages change smoothly pixel to pixel everywhere: no step at the chunk staircase.
        // (The deck alpha has the clouds' own texture; its step limit is looser than the fill's.)
        let (mut jg, mut ja) = (0f32, 0f32);
        for y in 0..h - 1 {
            for x in 0..w - 1 {
                let (cg, al) = c.mask_at(x, y);
                for (cn, an) in [c.mask_at(x + 1, y), c.mask_at(x, y + 1)] {
                    jg = jg.max((cg - cn).abs());
                    ja = ja.max((al - an).abs());
                }
            }
        }
        assert!(jg < 0.12 && ja < 0.3, "q{q} mask jumps: fill {jg}, deck {ja}");
        // A narrow reach is steeper, but no steeper than its own soft limit (a smoothstep over the fade): the
        // lattice adds no step of its own.
        for reach in [64.0f32, 40.0, 20.0] {
            let pn = Params { reach, ..p };
            let mut cn = Clouds::new();
            cn.update(&img_a, w, h, &v, 5.0, &pn);
            cn.composite(&mut img_a.clone(), w, h, &pn, 5.0);
            let fade = crate::engine::clouds::reach_fade_px(crate::engine::clouds::reach_px(reach, 3.0, w, h));
            let mut j = 0f32;
            for y in 0..h - 1 {
                for x in 0..w - 1 {
                    let (g, a) = cn.mask_at(x, y);
                    for (g2, a2) in [cn.mask_at(x + 1, y), cn.mask_at(x, y + 1)] {
                        j = j.max((g - g2).abs()).max((a - a2).abs() * 0.5);
                    }
                }
            }
            assert!(j <= 1.5 / fade + 0.04, "q{q} reach {reach}: mask jump {j} past the fade slope {}", 1.5 / fade);
        }
        // The output does not jump between neighbouring pixels across the border more than the map itself.
        let mut worst = 0i32;
        for y in 0..h {
            for x in 0..w - 1 {
                let (i, j) = ((y * w + x) * 4, (y * w + x + 1) * 4);
                if (img_a[i + 3] == 0) != (img_a[j + 3] == 0) {
                    let d: i32 = (0..3).map(|k| (a[i + k] as i32 - a[j + k] as i32).abs()).max().unwrap();
                    worst = worst.max(d);
                }
            }
        }
        assert!(worst < 40, "q{q} hard edge at the explored border: {worst}");
        // The clouds reach in over explored ground, up to the hard limit, and leave the far inside clear.
        let reach_px = crate::engine::clouds::reach_px(p.reach, 3.0, w, h);
        let mut over = 0;
        let mut n = 0;
        for y in (0..h).step_by(3) {
            for x in (0..w).step_by(3) {
                if img_a[(y * w + x) * 4 + 3] == 0 {
                    continue;
                }
                let d = (22.0 * 12.0 - (x + y) as f32) / std::f32::consts::SQRT_2;
                if d < reach_px * 0.4 {
                    n += 1;
                    if c.mask_at(x, y).0 > 0.5 {
                        over += 1;
                    }
                }
                if d > reach_px + 1.0 {
                    let (cg, al) = c.mask_at(x, y);
                    assert!(cg == 0.0 && al == 0.0, "q{q} cloud far inside explored ground at {x},{y}");
                }
            }
        }
        assert!(over * 2 > n, "q{q} clouds do not come over the border: {over}/{n}");
    }
}

#[test]
fn clouds_have_no_row_seams() {
    use crate::engine::clouds::{Clouds, Params};
    let (w, h) = (256usize, 256usize);
    for (q, bpp) in [(0u8, 4.0), (1, 4.0), (2, 4.0), (3, 4.0), (1, 1.0), (2, 16.0)] {
        let v = View::new([100.0, -40.0], bpp);
        let p = Params { quality: q, ..Params::default() };
        let mut img = vec![0u8; w * h * 4];
        let mut c = Clouds::new();
        assert!(c.update(&img, w, h, &v, 7.0, &p));
        c.composite(&mut img, w, h, &p, 7.0);
        let lum = |x: usize, y: usize| img[(y * w + x) * 4..][..3].iter().map(|&v| v as f32).sum::<f32>();
        let mut rows: Vec<f32> = (1..h - 1)
            .map(|y| (0..w).map(|x| (lum(x, y) - 0.5 * (lum(x, y - 1) + lum(x, y + 1))).abs()).sum::<f32>() / w as f32)
            .collect();
        let max = rows.iter().cloned().fold(0.0, f32::max);
        rows.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med = rows[rows.len() / 2];
        assert!(max <= med * 4.0 + 3.0, "q{q} bpp{bpp}: row seam {max} vs median {med}");
    }
}

#[test]
fn weather_step_does_not_jump_cloud_phase() {
    use crate::engine::clouds::{wind_target, Clouds, Params, MAX_DT};
    let mut c = Clouds::new();
    let mut p = Params::default();
    let dt = 1.0 / 60.0;
    let cap = 12.0 * p.speed + 1e-3;
    let mut last_phase = c.advance(0.0, &p);
    let mut last_d = 0.0f32;
    let mut worst_jerk = 0.0f32;
    for k in 1..600 {
        if k == 120 {
            p.rain = 1.0;
            p.thunder = 1.0;
        }
        let t = k as f32 * dt;
        let ph = c.advance(t, &p);
        let d = ph - last_phase;
        assert!(d >= 0.0 && d <= cap * MAX_DT, "frame {k}: phase step {d}");
        if k > 1 {
            worst_jerk = worst_jerk.max((d - last_d).abs());
        }
        // The old `t * speed` would jump by t * (new - old speed) here: hundreds of blocks at t = 2 s.
        last_phase = ph;
        last_d = d;
    }
    // Eased: a frame's step changes by at most a small fraction of the full speed change.
    let full = (wind_target(&p) - wind_target(&Params::default())) * dt;
    assert!(worst_jerk <= full * 0.05 + 1e-4, "jerk {worst_jerk} vs full step change {full}");
    // A one-second hitch advances at most MAX_DT worth.
    let before = c.advance(10.0, &p);
    let after = c.advance(11.0, &p);
    assert!(after - before <= cap * MAX_DT + 1e-4);
}

#[test]
fn frame_is_one_fixed_opaque_grey() {
    use crate::engine::style::{frame_colors, ring_color};
    let base = frame_colors(100, 6000, 0.0, 0.0, 0);
    for op in [100u32, 85, 70, 50, 0] {
        for t in (0..24000u32).step_by(1500) {
            for (r, th) in [(0.0f32, 0.0f32), (1.0, 0.0), (1.0, 1.0)] {
                for theme in 0..3u8 {
                    assert_eq!(frame_colors(op, t, r, th, theme), base, "op {op} t {t} rain {r} theme {theme}");
                }
            }
        }
    }
    for c in [base.edge, base.light, base.dark, base.mid] {
        assert_eq!(c >> 24, 0xFF, "frame colour {c:08x} not opaque");
        let (r, g, b) = ((c >> 16) & 255, (c >> 8) & 255, c & 255);
        // Neutral (cool) grey: no channel strays far from the others.
        assert!(r.max(g).max(b) - r.min(g).min(b) <= 0x20, "{c:08x} is not grey");
    }
    for i in 0..72 {
        let c = ring_color(i as f32 / 72.0 * std::f32::consts::TAU);
        assert_eq!(c >> 24, 0xFF);
    }
}

#[test]
fn autozoom_picks_scale_from_known_radius_and_does_not_flap() {
    use crate::engine::autozoom::{known_radius, target_bpp, AutoZoom, DWELL};
    // A 256 px minimap: 128 px half side; 512 known blocks want 4 blocks per pixel.
    assert!((target_bpp(512.0, 256) - 4.0).abs() < 1e-9);
    let mut z = AutoZoom::default();
    let first = z.update(512.0, 256, 0.5, 8.0, 1.0, 0.016);
    assert!((first - 4.0).abs() < 1e-6, "snaps on the first frame: {first}");
    // Limits clamp (tiny known area -> max zoom in, huge -> max zoom out).
    let mut n = AutoZoom::default();
    assert!((n.update(10.0, 256, 0.5, 8.0, 1.0, 0.016) - 0.5).abs() < 1e-6);
    let mut f = AutoZoom::default();
    assert!((f.update(1e6, 256, 0.5, 8.0, 1.0, 0.016) - 8.0).abs() < 1e-6);
    // Walking along a data edge: the radius wobbles +-12 % around a level boundary every frame for 20 s.
    let mut w = AutoZoom::default();
    let base = 512.0 * 2f64.powf(0.25); // halfway between the 4.0 and 5.66 levels
    w.update(base, 256, 0.5, 8.0, 1.0, 0.016);
    let mut changes = 0;
    let mut last = w.level;
    for i in 0..1250 {
        let r = base * if i % 2 == 0 { 1.12 } else { 0.88 };
        w.update(r, 256, 0.5, 8.0, 1.0, 0.016);
        if w.level != last {
            changes += 1;
            last = w.level;
        }
    }
    assert_eq!(changes, 0, "no flapping on a +-12 % wobble");
    // A real change (4x more data) switches after the dwell, and the shown scale eases (no jump).
    let mut prev = w.bpp;
    let mut max_step = 0.0f64;
    for _ in 0..((DWELL + 3.0) / 0.016) as usize {
        let b = w.update(base * 4.0, 256, 0.5, 8.0, 1.0, 0.016);
        max_step = max_step.max((b / prev).log2().abs());
        prev = b;
    }
    assert!(w.bpp > 15.9 * 0.5 && w.bpp <= 8.0 + 1e-9, "followed to the limit: {}", w.bpp);
    assert!(max_step < 0.1, "eases: max per-frame change {max_step} octaves");
    // The manual multiplier shifts the scale.
    let mut m = AutoZoom::default();
    assert!((m.update(512.0, 256, 0.5, 8.0, 0.5, 0.016) - 2.0).abs() < 1e-6);
    // Ring scan: 5 known chunks around the player -> 5.5 chunks radius.
    assert_eq!(known_radius(32, 0.6, |dx, dz| dx.abs().max(dz.abs()) <= 5), 5.5 * 16.0);
    assert_eq!(known_radius(32, 0.6, |_, _| false), 8.0);
}

#[test]
fn icons_are_line_art_and_cover_the_toolbar() {
    use crate::engine::icons::{self, ink, runs, TOOLBAR};
    for (i, ic) in TOOLBAR.iter().enumerate() {
        let n = ink(ic);
        // Visible, but a line drawing, not a filled blob (at most ~45 % of the square).
        assert!(n >= 20 && n <= 115, "toolbar icon {i}: {n} px");
        let mut sum = 0;
        runs(ic, |x, y, len| {
            assert!(x + len <= 16 && y < 16);
            sum += len;
        });
        assert_eq!(sum, n);
    }
    // All different.
    for a in 0..TOOLBAR.len() {
        for b in a + 1..TOOLBAR.len() {
            assert_ne!(TOOLBAR[a], TOOLBAR[b], "{a} and {b}");
        }
    }
    assert!(ink(&icons::SETTINGS) > 0 && ink(&icons::AUTOZOOM) > 0);
}

#[test]
fn settings_categories_cover_everything_search_reset_and_migrate() {
    use crate::engine::settings::{self, Row, Settings, BUTTON_COUNT, CATEGORIES};
    // Every flat button is in some category; nav per tab.
    let mut seen = vec![false; BUTTON_COUNT];
    for c in CATEGORIES.iter() {
        for i in c.items() {
            seen[i] = true;
        }
    }
    assert!(seen.iter().all(|&s| s), "uncategorised: {:?}", seen.iter().enumerate().filter(|e| !e.1).map(|e| e.0).collect::<Vec<_>>());
    assert_eq!(settings::tab_categories(0).count(), 6);
    assert_eq!(settings::tab_categories(1).count(), 7);
    for i in 0..BUTTON_COUNT {
        assert!(!settings::hint(i).is_empty(), "hint {i}");
    }
    // Rows: headers, items, a reset row; search spans categories.
    let s = Settings::default();
    let labels = s.buttons();
    let mut rows = Vec::new();
    let en = Tr::new(Lang::En);
    let ru = Tr::new(Lang::Ru);
    settings::rows(3, "", &labels, &en, &[], &mut rows);
    assert_eq!(rows[0], Row::Header("megamap.group.autozoom"));
    assert_eq!(rows.last(), Some(&Row::ResetCategory(3)));
    settings::rows(0, "приближение", &s.buttons_in(&ru), &ru, &[], &mut rows);
    assert_eq!(rows, vec![Row::Item(46)]);
    settings::rows(0, "cloud", &labels, &en, &[], &mut rows);
    assert!(rows.len() >= 8 && rows.iter().all(|r| matches!(r, Row::Item(_))));
    // Changed counter and category reset.
    let d = s.buttons();
    let mut t = s.clone();
    t.click(45);
    t.click(47);
    t.click(21);
    assert_eq!(t.changed_in(3, &t.buttons(), &d), 2);
    t.reset_category(3);
    assert_eq!(t.changed_in(3, &t.buttons(), &d), 0);
    assert_eq!(t.relief, (s.relief + 1) % 4, "other categories untouched");
    // Migration: a 0.3.0 file keeps the old look (no auto-zoom, dots), a fresh file gets the new defaults, and
    // the new keys round-trip.
    let old = Settings::from_text("version=3\nzoom=2\n");
    assert_eq!((old.autozoom, old.ent_mode, old.zoom), (false, [0; 4], 2));
    let mut n = Settings::default();
    n.az_near = 2;
    n.az_far = 1;
    let back = Settings::from_text(&n.to_text());
    assert!(back.az_far >= back.az_near);
    n.ui_cat = 9;
    n.ent_mode = [2, 0, 1, 2];
    assert_eq!(Settings::from_text(&n.to_text()).ent_mode, [2, 0, 1, 2]);
    assert_eq!(Settings::from_text(&n.to_text()).ui_cat, 9);
}

#[test]
fn preview_weather_params_fog_edge_and_budget() {
    use crate::engine::clouds::{Clouds, Look, Params};
    use crate::engine::preview::{self, SIZE};
    use crate::engine::settings::Target;
    let base = Params { weather: false, rain: 0.7, density: 0.4, ..Params::default() };
    let clear = preview::weather_params(base, preview::CLEAR, 1.0, 1.0);
    assert_eq!((clear.rain, clear.thunder, clear.day_time, clear.look), (0.0, 0.0, 6000, Look::Overworld));
    assert!(clear.weather);
    assert_eq!(clear.density, 0.4, "the player's own look is kept");
    let storm = preview::weather_params(base, preview::STORM, 1.0, 1.0);
    assert_eq!((storm.rain, storm.thunder), (1.0, 1.0));
    assert_eq!(preview::weather_params(base, preview::RAIN, 1.0, 1.0).thunder, 0.0);
    assert!(preview::weather_params(base, preview::NIGHT, 1.0, 1.0).day_time > 13_000);
    let n = preview::weather_params(base, preview::NETHER, 1.4, 0.5);
    assert_eq!((n.look, n.intensity), (Look::Nether, 1.4));
    assert_eq!(preview::weather_params(base, preview::END, 1.4, 0.5).look, Look::End);

    assert!(preview::due(0.0, 0.05) && !preview::due(0.0, 0.02) && preview::due(5.0, 1.0));

    let mut img = vec![255u8; SIZE * SIZE * 4];
    let cleared = preview::fog_edge(&mut img, SIZE);
    assert!(cleared > SIZE * SIZE / 5 && cleared < SIZE * SIZE * 3 / 4, "{cleared}");
    let c = (SIZE / 2 * SIZE + SIZE / 2) * 4 + 3;
    assert_eq!(img[c], 255, "the centre stays explored");
    assert_eq!(img[3], 0, "the corner is fog");
    assert_eq!(preview::fog_edge(&mut img, SIZE), 0, "idempotent");

    let r = [10.0, 20.0, 116.0, 116.0];
    assert_eq!(preview::target_rect(Target::None, r, 12.0), None);
    let inner = preview::target_rect(Target::Entities, r, 12.0).unwrap();
    assert!(inner[0] > r[0] && inner[0] + inner[2] < r[0] + r[2]);
    assert!(preview::target_rect(Target::MiniInfo, r, 12.0).unwrap()[1] >= r[1] + r[3]);

    // Frame budget: a preview frame (clouds update + composite at 116 px, the heaviest quality) well under 4 ms.
    let v = View { center: [0.0, 0.0], bpp: 2.0, rotation: 0.0, round: false, fog: true, style: Default::default() };
    let mut cl = Clouds::new();
    let mut frame = vec![0u8; SIZE * SIZE * 4];
    let mut worst = 0.0f64;
    let mut total = 0.0;
    for q in 0..4u8 {
        let p = Params { quality: q, ..preview::weather_params(Params::default(), preview::STORM, 1.0, 1.0) };
        for f in 0..20 {
            frame.copy_from_slice(&img);
            let t0 = std::time::Instant::now();
            cl.update(&frame, SIZE, SIZE, &v, f as f32 * 0.05, &p);
            cl.composite(&mut frame, SIZE, SIZE, &p, f as f32 * 0.05);
            let ms = t0.elapsed().as_secs_f64() * 1e3;
            if f > 2 {
                worst = worst.max(ms);
                total += ms;
            }
        }
    }
    eprintln!("preview frame: mean {:.3} ms, worst {:.3} ms", total / 68.0, worst);
    // The budget is for the optimised build the game ships; an unoptimised test build runs ~10x slower.
    let budget = if cfg!(debug_assertions) { 16.0 } else { 4.0 };
    assert!(total / 68.0 < budget, "{} ms", total / 68.0);

    // The preview shows the map, not a sheet of clouds: in clear weather the explored middle (a third of the side)
    // keeps its ground (0.3.1 floored the feather at 40 px, which with the overlap veiled all 116 px).
    for bpp in [1.0, 2.0, 8.0] {
        let v = View { center: [0.0, 0.0], bpp, rotation: 0.0, round: false, fog: true, style: Default::default() };
        let p = preview::weather_params(Params::default(), preview::CLEAR, 1.0, 1.0);
        let mut cl = Clouds::new();
        let mut ground = vec![0u8; SIZE * SIZE * 4];
        for px in ground.chunks_exact_mut(4) {
            px.copy_from_slice(&[40, 120, 40, 255]);
        }
        preview::fog_edge(&mut ground, SIZE);
        let mut fr = ground.clone();
        for f in 0..10 {
            fr.copy_from_slice(&ground);
            cl.update(&fr, SIZE, SIZE, &v, f as f32 * 0.05, &p);
            cl.composite(&mut fr, SIZE, SIZE, &p, f as f32 * 0.05);
        }
        let (mut clear, mut tot) = (0, 0);
        for y in SIZE / 3..SIZE * 2 / 3 {
            for x in SIZE / 3..SIZE * 2 / 3 {
                let i = (y * SIZE + x) * 4;
                tot += 1;
                if (fr[i] as i32 - 40).abs() < 25 && (fr[i + 1] as i32 - 120).abs() < 25 {
                    clear += 1;
                }
            }
        }
        assert!(clear * 10 > tot * 7, "bpp {bpp}: clouds veil the preview middle: {clear}/{tot} clear");
        // and they still reach over the border
        let e = (SIZE / 2 * SIZE + (SIZE / 2 - SIZE * 38 / 100)) * 4;
        assert!(cl.mask_at(SIZE / 2 - SIZE * 38 / 100, SIZE / 2).1 > 0.0 || fr[e] > 100, "bpp {bpp}: no clouds at the edge");
    }
}

#[test]
fn portraits_faces_raster_and_budget() {
    use crate::engine::portrait::{self, Budget, Tex};
    // A 64x64 skin: face red, hat pixel blue at its top-left, everything else green.
    let mut skin = Tex { w: 64, h: 64, rgba: vec![0; 64 * 64 * 4] };
    for y in 0..64 {
        for x in 0..64 {
            let i = (y * 64 + x) * 4;
            let c = if (8..16).contains(&x) && (8..16).contains(&y) {
                [255, 0, 0, 255]
            } else if x == 40 && y == 8 {
                [0, 0, 255, 255]
            } else if (32..64).contains(&x) && y < 16 {
                [0, 0, 0, 0]
            } else {
                [0, 255, 0, 255]
            };
            skin.rgba[i..i + 4].copy_from_slice(&c);
        }
    }
    let f = portrait::player_face(&skin).unwrap();
    assert_eq!(f[0], 0xFF00_00FF, "hat over the face");
    assert_eq!(f[9], 0xFFFF_0000);
    // An HD skin (128) gives the same face.
    let mut hd = Tex { w: 128, h: 128, rgba: vec![0; 128 * 128 * 4] };
    for y in 0..128 {
        for x in 0..128 {
            let s = (y / 2 * 64 + x / 2) * 4;
            let d = (y * 128 + x) * 4;
            hd.rgba[d..d + 4].copy_from_slice(&skin.rgba[s..s + 4].to_vec());
        }
    }
    assert_eq!(portrait::player_face(&hd).unwrap(), f);
    // Mirrored crop flips columns.
    let m = portrait::crop_face(&skin, [16.0 / 64.0, 8.0 / 64.0, 8.0 / 64.0, 16.0 / 64.0]).unwrap();
    assert_eq!(m[7], 0xFFFF_0000);
    // Runs merge equal colours.
    let r = portrait::runs(8, 8, |x, y| f[y * 8 + x]);
    assert!(r.len() < 64 && r.len() >= 8, "{}", r.len());

    // The player model rasterises; the front face shows the skin face on the upper part of the image.
    let model = portrait::player_model(false, false);
    assert_eq!(model.len() % 4, 0);
    let t0 = std::time::Instant::now();
    let (img, covered) = portrait::raster(&model, std::slice::from_ref(&skin), 64, 0.0, 0.0);
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    assert!(covered > 300, "covered {covered}");
    // Centre column, near the top (head): red face.
    let i = ((64 * 10 / 100 + 3) * 64 + 32) * 4;
    assert!(img[i] > 200 && img[i + 1] < 60, "head front {:?}", &img[i..i + 4]);
    // 3/4 view for the budget numbers.
    let t1 = std::time::Instant::now();
    let mut n = 0;
    for k in 0..20 {
        n += portrait::raster(&model, std::slice::from_ref(&skin), 116, 35.0 + k as f32, 20.0).1;
    }
    let ms34 = t1.elapsed().as_secs_f64() * 1000.0 / 20.0;
    assert!(n > 0);
    println!("portrait raster: front 64px {ms:.3} ms, 3/4 116px {ms34:.3} ms/mob");
    assert!(ms34 < 15.0);
    // Flat card.
    let (_, c) = portrait::raster(&portrait::flat_card(), std::slice::from_ref(&skin), 32, 0.0, 0.0);
    assert!(c > 500);
    // A block-model element: a full cube is 6 quads, its top from slot 1 (the second texture).
    let mut cube = Vec::new();
    portrait::block_box(&mut cube, [0.0; 3], [16.0; 3], [0, 0, 0, 0, 1, 0], 0xFFFF_FFFF);
    assert_eq!(cube.len(), 24);
    assert!(cube[16..20].iter().all(|v| v.uv[0] >= 1.0 && v.uv[0] <= 2.0 && v.pos[1] == 1.0));
    let (_, c) = portrait::raster(&cube, &[skin.clone(), skin.clone()], 32, 35.0, 20.0);
    assert!(c > 300, "cube {c}");
    let mut b = Budget::new(2);
    assert!(b.take() && b.take() && !b.take());
}

// ---- 0.3.2 ----

#[test]
fn layers_hysteresis_dwell_and_window() {
    use crate::engine::layers::{self, Detector, Evidence, LayerMode, Probe};
    let open = Probe { y: 70, sky: Some(15), ground: Some(70) };
    let deep = Probe { y: 20, sky: Some(0), ground: Some(70) };
    let edge = Probe { y: 66, sky: Some(7), ground: Some(70) };
    assert_eq!(layers::evidence(open, 0), Evidence::Surface);
    assert_eq!(layers::evidence(deep, 0), Evidence::Cave);
    assert_eq!(layers::evidence(edge, 0), Evidence::Unsure);
    // Rule 0: any sign of open sky wins (sky light under an overhang, but above the height map).
    let overhang = Probe { y: 71, sky: Some(2), ground: Some(70) };
    assert_eq!(layers::evidence(overhang, 0), Evidence::Surface);
    assert_eq!(layers::evidence(overhang, 1), Evidence::Cave);
    assert_eq!(layers::evidence(overhang, 2), Evidence::Surface);
    // Unknown ground: the sky decides alone.
    assert_eq!(layers::evidence(Probe { y: 0, sky: Some(0), ground: None }, 0), Evidence::Cave);
    let mut d = Detector::default();
    // Enter needs ENTER_S of steady cave evidence.
    assert!(!d.update(0.0, deep, 0));
    assert!(!d.update(0.9, deep, 0));
    assert!(d.update(1.0, deep, 0) && d.cave);
    // Flicker in the dead band keeps the layer; a short surface blip does not flip back.
    for k in 0..20 {
        assert!(!d.update(1.1 + k as f32 * 0.1, if k % 2 == 0 { edge } else { deep }, 0));
    }
    assert!(!d.update(3.2, open, 0));
    assert!(!d.update(3.3, deep, 0));
    assert!(!d.update(3.4, open, 0));
    assert!(d.cave);
    assert!(d.update(3.9, open, 0) && !d.cave);
    // Slices: 8-block buckets, a window `depth` deep, fixed and surface modes.
    assert_eq!(layers::slice(LayerMode::Surface, true, 20, 64), None);
    assert_eq!(layers::slice(LayerMode::Auto, false, 20, 64), None);
    assert_eq!(layers::slice(LayerMode::Auto, true, 20, 64), Some(25));
    assert_eq!(layers::slice(LayerMode::Cave, false, 23, 64), Some(25));
    assert_eq!(layers::slice(LayerMode::Cave, false, -1, 64), Some(1));
    assert_eq!(layers::slice(LayerMode::FixedY, false, 20, 64), Some(64));
    assert_eq!(layers::cave_window(25, 16), (25, 9));
    assert_eq!(layers::cave_window(25, 0), (25, 24));
    for m in LayerMode::ALL {
        assert_eq!(LayerMode::from_key(m.key()), Some(m));
        assert_eq!(LayerMode::from_index(m.index()), m);
    }
    assert_eq!(LayerMode::FixedY.next(), LayerMode::Auto);
    // Cross-fade.
    assert_eq!(layers::fade(0.0), 0.0);
    assert_eq!(layers::fade(layers::FADE_S), 1.0);
    let mut img = vec![200u8; 8];
    layers::crossfade(&mut img, &[0u8; 8], layers::fade(layers::FADE_S / 2.0));
    assert!((95..=105).contains(&img[0]), "{}", img[0]);
}

#[test]
fn cave_window_cuts_ceiling_and_floor() {
    // The hollow is y 20..30 under hills at ~64: a slice topped at 28 cuts the ceiling and finds the floor at 19
    // when the window reaches it, and stops short of it when the window is too shallow.
    let w = World { r: 1, cave: true };
    let deep = bake_chunk(&w, &Plains, &table(), 0, 0, Layer::Cave(28, 10)).unwrap();
    assert!(deep.height.iter().all(|&h| h == 19), "{:?}", &deep.height[..4]);
    let shallow = bake_chunk(&w, &Plains, &table(), 0, 0, Layer::Cave(28, 22));
    assert!(shallow.map_or(true, |c| c.height.iter().all(|&h| h != 19)));
}

#[test]
fn palette_average_map_id_override_and_hash() {
    use crate::engine::baker::{ColorTable, MC_GRASS, MC_PLANT, MC_WATER};
    use crate::engine::palette::{self, AverageCache, Sample, Tint, MC_PLAIN};
    // Alpha-weighted: a clear pixel adds nothing; only the first frame of a strip counts.
    let mut px = vec![0u8; 2 * 4 * 4];
    px[..4].copy_from_slice(&[255, 0, 0, 255]);
    px[4..8].copy_from_slice(&[0, 0, 255, 0]);
    px[8..12].copy_from_slice(&[255, 0, 0, 255]);
    px[12..16].copy_from_slice(&[255, 0, 0, 255]);
    px[16..].iter_mut().for_each(|b| *b = 255); // second frame, white: ignored
    assert_eq!(palette::average(2, 4, &px), Some(0xFF0000));
    assert_eq!(palette::average(1, 1, &[1, 2, 3, 0]), None);
    assert_eq!(palette::map_id(MC_WATER, Tint::None), MC_WATER);
    assert_eq!(palette::map_id(MC_GRASS, Tint::Grass), MC_GRASS);
    assert_eq!(palette::map_id(MC_PLANT, Tint::None), MC_PLAIN);
    assert_eq!(palette::map_id(11, Tint::Foliage), MC_PLANT);
    assert_eq!(palette::map_id(11, Tint::None), 11);
    let mut vanilla = ColorTable::with_len(4);
    vanilla.set(0, 11, 0x707070);
    vanilla.set(1, MC_PLANT, 0x007C00);
    vanilla.set(2, MC_WATER, 0x4040FF);
    let s = [
        Sample { id: 0, rgb: 0x808080, tint: Tint::None },
        Sample { id: 1, rgb: 0xE0E020, tint: Tint::None },
        Sample { id: 2, rgb: 0x3355AA, tint: Tint::Water },
        Sample { id: 3, rgb: 0x123456, tint: Tint::None },
        Sample { id: 0, rgb: 0x808080, tint: Tint::Fixed(0xFF0000) },
    ];
    let t = palette::override_table(&vanilla, &s[..4], false);
    assert_eq!(t.get(0), Some((11, 0x808080)));
    assert_eq!(t.get(1), Some((MC_PLAIN, 0xE0E020)));
    assert_eq!(t.get(2), Some((MC_WATER, 0x3355AA)));
    assert_eq!(t.get(3), None, "a state missing from the vanilla table stays missing");
    assert_eq!(palette::override_table(&vanilla, &s[4..], false).get(0).unwrap().1 & 0x00FFFF, 0);
    let sh = palette::override_table(&vanilla, &s[..1], true).get(0).unwrap().1;
    assert!((sh >> 16) > (sh & 0xFF), "shader tint warms");
    assert!(!palette::use_pack(0, true) && palette::use_pack(1, false) && palette::use_pack(2, true) && !palette::use_pack(2, false));
    let a = palette::pack_hash(1, &["vanilla".into(), "file/x.zip".into()], "", 2);
    assert_ne!(a, palette::pack_hash(1, &["file/x.zip".into(), "vanilla".into()], "", 2), "order matters");
    assert_ne!(a, palette::pack_hash(2, &["vanilla".into(), "file/x.zip".into()], "", 2));
    assert_ne!(a, palette::pack_hash(1, &["vanilla".into(), "file/x.zip".into()], "bsl", 2));
    assert_ne!(a, palette::pack_hash(1, &["vanilla".into(), "file/x.zip".into()], "", 1));
    assert_eq!(a, palette::pack_hash(1, &["vanilla".into(), "file/x.zip".into()], "", 2));
    let mut c = AverageCache::default();
    let load = |_: &str| Some((1, 1, vec![10, 20, 30, 255]));
    assert_eq!(c.get("block/stone", load), Some(0x0A141E));
    assert_eq!(c.get("block/stone", |_| panic!("cached")), Some(0x0A141E));
    assert_eq!(c.decoded, 1);
}

#[test]
fn lang_names_search_and_json_tables() {
    use crate::engine::lang::{self, Lang};
    assert_eq!(Lang::from_code("ru_ru"), Lang::Ru);
    assert_eq!(Lang::from_code("en_GB"), Lang::En);
    assert_eq!(Lang::pick(0, "ru_ru"), Lang::Ru);
    assert_eq!(Lang::pick(1, "ru_ru"), Lang::En);
    assert_eq!(lang::key_of("entity", "minecraft:zombie"), "entity.minecraft.zombie");
    assert_eq!(lang::key_of("biome", "terralith:alpine/peaks"), "biome.terralith.alpine.peaks");
    let none = |_: &str| None;
    let game = |k: &str| (k == "entity.minecraft.zombie").then(|| "Zombi (game)".to_string());
    assert_eq!(lang::name(Lang::Ru, "entity", "minecraft:zombie", &game), "Zombi (game)");
    assert_eq!(lang::name(Lang::Ru, "entity", "minecraft:zombie", &none), "Зомби");
    assert_eq!(lang::name(Lang::En, "dimension", "minecraft:the_nether", &none), "Nether");
    assert_eq!(lang::name(Lang::Ru, "biome", "mymod:dark_oak_forest", &none), "Dark oak forest");
    assert!(lang::matches("зомб", "Зомби", "minecraft:zombie"));
    assert!(lang::matches("zombie", "Зомби", "minecraft:zombie"));
    assert!(lang::matches("dark oak", "Тёмный лес", "minecraft:dark_oak_forest"));
    assert!(!lang::matches("крипер", "Зомби", "minecraft:zombie"));
    assert_eq!(lang::weather(Lang::Ru, 1.0, 1.0), "Гроза");
    assert_eq!(lang::weather(Lang::En, 0.0, 0.0), "Clear");
    assert_eq!(lang::time_of_day(Lang::Ru, 18000), "Ночь");
    // The asset files are the table.
    // MEGAMAP_REGEN_LANG=1 rewrites them from the table (then build again).
    if std::env::var_os("MEGAMAP_REGEN_LANG").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/megamap/lang");
        std::fs::write(dir.join("en_us.json"), lang::to_json(Lang::En)).unwrap();
        std::fs::write(dir.join("ru_ru.json"), lang::to_json(Lang::Ru)).unwrap();
        return;
    }
    for (l, f) in [(Lang::En, include_str!("../../../../tachyon/content/megamap/assets/megamap/lang/en_us.json")), (Lang::Ru, include_str!("../../../../tachyon/content/megamap/assets/megamap/lang/ru_ru.json"))] {
        assert_eq!(f.replace("\r\n", "\n"), lang::to_json(l), "{:?} json out of step: regenerate from lang::TABLE", l);
    }
}

#[test]
fn settings_v5_migration_layers_frame_sliders() {
    use crate::engine::layers::LayerMode;
    use crate::engine::settings::{FACE_FRAME, VERSION};
    // 0.3.1 file: cave mode, default 1 px frame.
    let o = Settings::from_text("version=4\nmode=cave\nface_frame=1\n");
    assert_eq!((o.mode, o.layer_mode(), o.face_frame), (Mode::Shown, LayerMode::Cave, 0));
    let o = Settings::from_text("version=4\nmode=hidden\nface_frame=2\n");
    assert_eq!((o.mode, o.layer_mode(), o.face_frame), (Mode::Hidden, LayerMode::Auto, 8));
    assert_eq!(o.frame_px(), 2.0);
    // v5 keeps quarter px; bad values fall back to none.
    assert_eq!(Settings::from_text(&format!("version={VERSION}\nface_frame=3\n")).face_frame, 3);
    assert_eq!(Settings::from_text(&format!("version={VERSION}\nface_frame=5\n")).face_frame, 0);
    let d = Settings::default();
    assert!(d.ui_fixed && d.face_frame == 0 && d.layer_mode() == LayerMode::Auto && d.color_style == 2);
    // Sliders: the auto-zoom pair stays ordered.
    let mut s = d.clone();
    assert_eq!(s.slider(46), Some((0, 5)));
    s.set_slider(47, 1);
    s.set_slider(46, 4);
    assert_eq!((s.az_near, s.az_far), (4, 4));
    s.set_slider(47, 2);
    assert_eq!((s.az_near, s.az_far), (2, 2));
    s.set_slider(FACE_FRAME, 99);
    assert_eq!(s.frame_px(), 3.0);
    assert_eq!(Settings::from_text(&s.to_text()), s);
    assert!(s.buttons()[FACE_FRAME].contains("3 px"));
    assert_eq!(d.buttons()[FACE_FRAME], "Icon frame: none");
    assert_eq!(d.buttons_in(&crate::engine::lang::Tr::new(Lang::Ru))[FACE_FRAME], "Рамка иконок: нет");
}

#[test]
fn ui_colours_ignore_time_and_weather() {
    use crate::engine::{clouds, style};
    let s = Settings::default();
    let mut seen = std::collections::HashSet::new();
    for t in (0..24000).step_by(500) {
        for (rain, thunder) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)] {
            let day = clouds::daylight(t).0;
            seen.insert((
                style::ui_light(s.ui_theme, s.ui_fixed, day, rain, thunder),
                style::frame_colors(s.opacity, t, rain, thunder, s.ui_theme),
            ));
        }
    }
    assert_eq!(seen.len(), 1, "UI colours changed with time or weather");
    // Off: the auto theme follows the day again (but never the weather).
    let noon = clouds::daylight(6000).0;
    let night = clouds::daylight(18000).0;
    assert!(style::ui_light(0, false, noon, 0.0, 0.0) && !style::ui_light(0, false, night, 0.0, 0.0));
    assert_eq!(style::ui_light(0, false, noon, 0.0, 0.0), style::ui_light(0, false, noon, 1.0, 1.0));
}

#[test]
fn icon_frame_rings_anti_aliased() {
    assert!(crate::engine::style::frame_rings(0.0).is_empty());
    assert_eq!(crate::engine::style::frame_rings(0.25), vec![(1.0, 0.25)]);
    assert_eq!(crate::engine::style::frame_rings(1.0), vec![(1.0, 1.0)]);
    assert_eq!(crate::engine::style::frame_rings(1.5), vec![(1.0, 1.0), (2.0, 0.5)]);
    assert_eq!(crate::engine::style::frame_rings(3.0).len(), 3);
    assert_eq!(crate::engine::style::frame_rings(9.0).len(), 3);
    // Coverage sums to the width for every step the setting offers.
    for q in 0..=12u8 {
        let w = q as f32 / 4.0;
        let sum: f32 = crate::engine::style::frame_rings(w).iter().map(|r| r.1).sum();
        assert!((sum - w).abs() < 1e-4, "{w}");
    }
}

#[test]
fn settings_sliders_click_steps() {
    use crate::engine::settings::{self as st, Settings};
    assert_eq!(st::slider_step(0.0, 9), 0);
    assert_eq!(st::slider_step(1.0, 9), 8);
    assert_eq!(st::slider_step(0.5, 9), 4);
    assert_eq!(st::slider_step(-3.0, 9), 0);
    let mut s = Settings::default();
    for &i in &st::SLIDERS {
        let (_, n) = s.slider(i).expect("slider");
        s.set_slider(i, st::slider_step(1.0, n));
        assert_eq!(s.slider(i).unwrap().0, n - 1);
    }
    assert_eq!(s.frame_px(), 3.0);
    // Near never passes far.
    s.set_slider(47, 0);
    assert!(s.slider(46).unwrap().0 <= s.slider(47).unwrap().0);
}

#[test]
fn entity_search_by_shown_name_and_id() {
    use crate::engine::{entities, lang};
    let seen: Vec<String> = Vec::new();
    let mut ru = |k: &str| lang::name(lang::Lang::Ru, "entity", k, &|_| None);
    let r = entities::list_named(&seen, "зомби", None, &mut ru);
    assert!(r.iter().any(|k| k == "minecraft:zombie"), "{r:?}");
    let r = entities::list_named(&seen, "minecraft:creeper", None, &mut ru);
    assert!(r.iter().any(|k| k == "minecraft:creeper"));
    let r = entities::list_named(&seen, "крипер", None, &mut ru);
    assert_eq!(r, vec!["minecraft:creeper".to_string()]);
}

/// A synthetic map in world coordinates: explored ground is a checker of colours, two discs are unexplored.
fn world_image(v: &View, w: usize, h: usize) -> Vec<u8> {
    let mut img = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let [wx, wz] = v.to_world(w, h, x as f64, y as f64);
            let i = (y * w + x) * 4;
            let hole = |cx: f64, cz: f64, r: f64| (wx - cx).powi(2) + (wz - cz).powi(2) < r * r;
            if hole(40.0, 10.0, 60.0) || hole(-90.0, 50.0, 25.0) {
                continue;
            }
            let c = (((wx / 16.0).floor() + (wz / 16.0).floor()) as i64).rem_euclid(2) as u8;
            img[i..i + 4].copy_from_slice(&[60 + c * 80, 120, 40 + c * 40, 255]);
        }
    }
    img
}

/// Pan invariance: shifting the view by N pixels (any N, not only node multiples, also rotated) gives the same
/// clouds over the same ground in the overlap, and the drag frame reuses the cached noise instead of recomputing.
#[test]
fn clouds_pan_invariant() {
    let (w, h) = (320usize, 200usize);
    for (rot, q) in [(0.0f64, 1u8), (0.0, 2), (0.6, 2), (0.0, 3)] {
        let p = Params { quality: q, density: 0.8, ..Params::default() };
        let mut va = View::new([10.0, 20.0], 1.0);
        va.rotation = rot;
        let mut c = Clouds::new();
        let ia = world_image(&va, w, h);
        assert!(c.update(&ia, w, h, &va, 5.0, &p));
        let mut a = ia.clone();
        c.composite(&mut a, w, h, &p, 5.0);
        for (nx, ny) in [(5i64, 0i64), (13, 7), (-37, 3), (1, -1)] {
            // Image pixel (x, y) of B shows the ground of pixel (x + nx, y + ny) of A.
            let mut vb = va;
            let (s, cs) = rot.sin_cos();
            vb.center = [va.center[0] + (nx as f64 * cs - ny as f64 * s), va.center[1] + (nx as f64 * s + ny as f64 * cs)];
            let ib = world_image(&vb, w, h);
            assert!(c.update(&ib, w, h, &vb, 5.0, &p));
            assert!(c.reused > c.computed * 4, "drag frame recomputed {} nodes, reused {}", c.computed, c.reused);
            let mut b = ib.clone();
            c.composite(&mut b, w, h, &p, 5.0);
            let (mut worst, mut sum, mut n) = (0i32, 0i64, 0i64);
            let m = 24i64;
            for y in m..h as i64 - m {
                for x in m..w as i64 - m {
                    let (xa, ya) = (x + nx, y + ny);
                    if xa < m || ya < m || xa >= w as i64 - m || ya >= h as i64 - m {
                        continue;
                    }
                    let (ib_, ia_) = (((y * w as i64 + x) * 4) as usize, ((ya * w as i64 + xa) * 4) as usize);
                    for k in 0..3 {
                        let d = (b[ib_ + k] as i32 - a[ia_ + k] as i32).abs();
                        worst = worst.max(d);
                        sum += d as i64;
                        n += 1;
                    }
                }
            }
            let mean = sum as f64 / n as f64;
            assert!(worst <= 3 && mean < 0.05, "rot {rot} q {q} shift ({nx},{ny}): worst {worst}, mean {mean:.3}");
            // Back to A for the next shift (the cache follows).
            c.update(&ia, w, h, &va, 5.0, &p);
        }
    }
}

/// The hard reach: with reach = N blocks no pixel of the cloud mask farther than N blocks (Euclidean, in the world)
/// from the nearest unexplored pixel has any density, and the output there is the map itself - at every quality,
/// zoom, rotation and on the round minimap. Inside the limit the clouds do come over the border (reach > 0).
#[test]
fn clouds_reach_hard_limit_in_blocks() {
    use crate::engine::clouds::{reach_px, Clouds};
    const EPS: f32 = 1.0 / 255.0;
    let cases: [(usize, usize, f64, u8, f64, bool, f32); 9] = [
        (320, 200, 1.0, 1, 0.0, false, 10.0),
        (320, 200, 0.5, 2, 0.0, false, 10.0),
        (320, 200, 2.0, 3, 0.0, false, 10.0),
        (320, 200, 4.0, 0, 0.0, false, 32.0),
        (320, 200, 0.25, 2, 0.6, false, 8.0),
        (320, 200, 1.0, 2, 0.0, false, 64.0),
        (320, 200, 1.0, 1, 0.0, false, 0.0),
        (320, 200, 2.0, 2, 0.0, false, 16.0),
        (128, 128, 1.0, 1, 0.0, true, 10.0),
    ];
    for (w, h, bpp, q, rot, round, reach) in cases {
        let mut v = View::new([30.0, -20.0], bpp);
        v.rotation = rot;
        v.round = round;
        let img = world_image(&v, w, h);
        let p = Params { quality: q, density: 0.9, reach, ..Params::default() };
        let mut c = Clouds::new();
        c.update(&img, w, h, &v, 3.0, &p);
        let mut out = img.clone();
        c.composite(&mut out, w, h, &p, 3.0);
        let unk: Vec<(i32, i32)> = (0..w * h).filter(|&i| img[i * 4 + 3] == 0).map(|i| ((i % w) as i32, (i / w) as i32)).collect();
        assert!(!unk.is_empty());
        let lim = reach_px(reach, bpp, w, h);
        // The limit is in blocks while it fits the image cap.
        assert!(lim <= reach / bpp as f32 + 1e-4);
        let (mut beyond, mut near, mut near_cov) = (0, 0, 0);
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                if img[i * 4 + 3] == 0 {
                    continue;
                }
                let d2 = unk.iter().map(|&(ux, uy)| (ux - x as i32).pow(2) + (uy - y as i32).pow(2)).min().unwrap();
                let d = (d2 as f32).sqrt();
                let (cg, al) = c.mask_at(x, y);
                if d > lim {
                    beyond += 1;
                    assert!(cg <= EPS && al <= EPS, "reach {reach} bpp {bpp} q{q}: density {cg}/{al} at {x},{y}, {:.1} blocks in (limit {reach})", d as f64 * bpp);
                    assert_eq!(&out[i * 4..i * 4 + 4], &img[i * 4..i * 4 + 4], "reach {reach} bpp {bpp} q{q}: output changed at {x},{y}");
                } else if d <= (lim * 0.3).max(1.0) {
                    near += 1;
                    if cg > 0.3 || al > 0.3 {
                        near_cov += 1;
                    }
                }
            }
        }
        assert!(beyond > 0);
        if reach / bpp as f32 >= 4.0 {
            assert!(near_cov * 2 > near, "reach {reach} bpp {bpp} q{q}: clouds do not come over the border ({near_cov}/{near})");
        }
    }
}

/// The reach setting: blocks are the unit, chunk mode steps by 16 and only rounds the limit down, and the 0.3.2
/// value (chunks of feather width) migrates to blocks.
#[test]
fn settings_reach_blocks_chunks_migrate() {
    use crate::engine::settings::{self as st, Settings};
    let d = Settings::default();
    assert_eq!((d.cloud_reach, d.mini_reach, d.reach_chunks), (10, 10, false));
    assert_eq!(d.reach_blocks(false), 10);
    assert_eq!(d.cloud_params().reach, 10.0);
    let mut s = d.clone();
    s.set_slider(st::MAP_REACH, 23);
    assert_eq!(s.reach_blocks(false), 23);
    assert_eq!(s.slider(st::MAP_REACH), Some((23, 65)));
    s.click(st::REACH_UNIT);
    assert!(s.reach_chunks);
    // Never past the value in blocks.
    assert_eq!(s.reach_blocks(false), 16);
    assert_eq!(s.slider(st::MAP_REACH), Some((1, 5)));
    s.set_slider(st::MAP_REACH, 4);
    assert_eq!(s.reach_blocks(false), 64);
    s.click(st::MAP_REACH);
    assert_eq!(s.reach_blocks(false), 0);
    s.click(st::MAP_REACH);
    assert_eq!(s.reach_blocks(false), 16);
    // Round trip.
    s.set_slider(st::MINI_REACH, 2);
    let t = Settings::from_text(&s.to_text());
    assert_eq!((t.cloud_reach, t.mini_reach, t.reach_chunks), (16, 32, true));
    // Migration from 0.3.2 files.
    for (old, new) in [(3u32, 10u32), (1, 3), (2, 7), (4, 13), (6, 20)] {
        let t = Settings::from_text(&format!("cloud_reach={old}\n"));
        assert_eq!((t.cloud_reach, t.mini_reach), (new, new), "old {old}");
    }
    // New keys win over the old one whatever the order.
    let t = Settings::from_text("cloud_reach=6\ncloud_reach_blocks=5\n");
    assert_eq!(t.cloud_reach, 5);
    // Reset.
    let mut s = Settings::default();
    s.set_slider(st::MINI_REACH, 40);
    s.click(st::REACH_UNIT);
    s.reset_item(st::MINI_REACH);
    s.reset_item(st::REACH_UNIT);
    assert_eq!((s.mini_reach, s.reach_chunks), (10, false));
}

/// The user's 4 blocks/px scene: an explored island in the unexplored, with far (estimated) terrain in chunk and
/// region squares next to it and away from it. `far_alpha` = what those squares carry (FAR_ALPHA as the view marks
/// them, or 0 = plain unknown).
fn far_scene(v: &View, w: usize, h: usize, far_alpha: u8) -> Vec<u8> {
    let mut img = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let [wx, wz] = v.to_world(w, h, x as f64, y as f64);
            let i = (y * w + x) * 4;
            let (cx, cz) = ((wx / 16.0).floor() as i64, (wz / 16.0).floor() as i64);
            let island = (wx - 30.0).powi(2) + (wz + 10.0).powi(2) < 230.0f64.powi(2);
            // Far squares: a region-like block east of the island, a strip of chunks north, scattered chunks.
            let far = (16..34).contains(&cx) && (-12..6).contains(&cz)
                || (-6..9).contains(&cx) && (-26..-17).contains(&cz)
                || (cx * 7 + cz * 13).rem_euclid(23) == 0 && !(-40..40).contains(&cz);
            if island {
                let c = ((cx + cz).rem_euclid(2)) as u8;
                img[i..i + 4].copy_from_slice(&[60 + c * 80, 120, 40 + c * 40, 255]);
            } else if far {
                if far_alpha == 0 {
                    img[i..i + 4].copy_from_slice(&[0x1B, 0x1E, 0x24, 0]);
                } else {
                    img[i..i + 4].copy_from_slice(&[150, 150, 150, far_alpha]);
                }
            } else {
                img[i..i + 4].copy_from_slice(&[0x1B, 0x1E, 0x24, 0]);
            }
        }
    }
    img
}

/// Rectangular cloud patches (v0.3.3 user report) and fog over far terrain (0.3.5). (1) Default (`far_fog` off):
/// the far terrain shows through, deep inside its squares untouched, while the fog over ground with no terrain at
/// all is exactly what it was, and the fog comes back over the far coverage edge with a soft front, not a step.
/// (1b) `far_fog` on: far squares come out exactly as unknown ground does (the 0.3.3/0.3.4 behaviour). (2) Either
/// way the clouds are a pure function of the world point, the time and the masks: the image rendered whole and the
/// same image stitched from tiles (each rendered with a halo, by a fresh engine) agree, so no square shows.
#[test]
fn clouds_far_terrain_hidden_and_tiling_invariant() {
    use crate::engine::clouds::Clouds;
    let (w, h) = (384usize, 256usize);
    for (bpp, q, rot) in [(4.0f64, 1u8, 0.0f64), (4.0, 2, 0.0), (4.0, 3, 0.0), (1.0, 2, 0.0), (2.0, 2, 0.6), (8.0, 2, 0.0)] {
        for far_fog in [false, true] {
            let mut v = View::new([60.0, 0.0], bpp);
            v.rotation = rot;
            v.style.mark_unknown = true;
            let p = Params { quality: q, density: 0.8, far_fog, ..Params::default() };
            let render = |v: &View, w: usize, h: usize, far_alpha: u8| -> Vec<u8> {
                let img = far_scene(v, w, h, far_alpha);
                let mut c = Clouds::new();
                assert!(c.update(&img, w, h, v, 4.0, &p));
                let mut out = img.clone();
                c.composite(&mut out, w, h, &p, 4.0);
                out
            };
            let a = render(&v, w, h, view::FAR_ALPHA);
            let b = render(&v, w, h, 0);
            let src = far_scene(&v, w, h, view::FAR_ALPHA);
            let worst = a.iter().zip(&b).map(|(x, y)| (*x as i32 - *y as i32).abs()).max().unwrap();
            if far_fog {
                // (1b) Far squares = unknown, to the last bit of colour.
                assert!(worst <= 1, "bpp {bpp} q{q} far_fog: far terrain shows through the fog (worst {worst})");
            } else {
                // Distance (px) from each pixel to the nearest pixel of a kind, by brute force over a window.
                let lpx = crate::engine::clouds::lod_front_px(bpp, w, h);
                let near = |x: usize, y: usize, r: usize, f: &dyn Fn(usize) -> bool| -> bool {
                    let (x0, y0, x1, y1) = (x.saturating_sub(r), y.saturating_sub(r), (x + r).min(w - 1), (y + r).min(h - 1));
                    (y0..=y1).any(|yy| (x0..=x1).any(|xx| f(yy * w + xx)))
                };
                let is_far = |k: usize| src[k * 4 + 3] == view::FAR_ALPHA;
                let is_void = |k: usize| src[k * 4 + 3] == 0;
                let (mut deep, mut deep_clear, mut voids, mut void_same, mut step, mut void_max) = (0, 0, 0, 0, 0i32, 0i32);
                let r = (lpx * 2.5).ceil() as usize + 4;
                for y in 0..h {
                    for x in 0..w {
                        let k = y * w + x;
                        let d = (0..3).map(|c| (a[k * 4 + c] as i32 - src[k * 4 + c] as i32).abs()).max().unwrap();
                        if is_far(k) && !near(x, y, r, &is_void) {
                            deep += 1;
                            deep_clear += (d <= 2) as i32;
                        }
                        // Ground with no terrain far from any far square: the fog exactly as before.
                        if is_void(k) && !near(x, y, r, &is_far) {
                            voids += 1;
                            let e = (0..4).map(|c| (a[k * 4 + c] as i32 - b[k * 4 + c] as i32).abs()).max().unwrap();
                            // Exact below Volumetric; there the light march (7 steps towards the sun) reads the deck
                            // further out, so the shading of the clouds may move by a few levels next to the front.
                            void_same += (e <= if q >= 2 { 16 } else { 2 }) as i32;
                            void_max = void_max.max(e);
                        }
                        // The front: fog over far terrain changes by small steps between neighbouring pixels.
                        if x + 1 < w && is_far(k) && is_far(k + 1) {
                            let d1 = (0..3).map(|c| (a[k * 4 + 4 + c] as i32 - src[k * 4 + 4 + c] as i32).abs()).max().unwrap();
                            step = step.max((d1 - d).abs());
                        }
                    }
                }
                eprintln!("far terrain bpp {bpp} q{q} rot {rot}: front {lpx:.1} px, deep far {deep_clear}/{deep} clear, void {void_same}/{voids} unchanged (max diff {void_max}), max step {step}");
                assert!(deep_clear == deep, "bpp {bpp} q{q}: fog over deep far terrain ({deep_clear}/{deep})");
                assert!(void_same == voids, "bpp {bpp} q{q}: fog over plain unknown changed ({void_same}/{voids})");
                // The front fades over `reach_fade_px` (3.5 px at the narrowest): at most 1.5 / 3.5 of full fog a pixel.
                assert!(step <= 112, "bpp {bpp} q{q}: the far front is a step ({step})");
            }
            // (2) Tilings: 2 x 2 and 3 x 2 tiles, each rendered with a halo around its core.
            for (tx, ty) in [(2usize, 2usize), (3, 2)] {
                let (cw, ch) = (w / tx, h / ty);
                const HALO: usize = 96;
                let (tw, th) = (cw + 2 * HALO, ch + 2 * HALO);
                let mut stitched = vec![0u8; w * h * 4];
                for j in 0..ty {
                    for i in 0..tx {
                        // Tile pixel (HALO + x, HALO + y) shows full-image pixel (i * cw + x, j * ch + y).
                        let (ox, oy) = ((i * cw) as f64 - HALO as f64, (j * ch) as f64 - HALO as f64);
                        let c0 = v.to_world(w, h, w as f64 / 2.0, h as f64 / 2.0);
                        let c1 = v.to_world(w, h, ox + tw as f64 / 2.0, oy + th as f64 / 2.0);
                        let vt = View { center: [v.center[0] + c1[0] - c0[0], v.center[1] + c1[1] - c0[1]], ..v };
                        let t = render(&vt, tw, th, view::FAR_ALPHA);
                        for y in 0..ch {
                            let src = ((HALO + y) * tw + HALO) * 4;
                            let dst = ((j * ch + y) * w + i * cw) * 4;
                            stitched[dst..dst + cw * 4].copy_from_slice(&t[src..src + cw * 4]);
                        }
                    }
                }
                let (mut worst, mut sum, mut bad) = (0i32, 0i64, Vec::new());
                // Away from the outer border of the whole image (there the whole render guesses the ground past
                // its edge, the tiles know it); every tile seam is inside.
                const M: usize = 32;
                for (k, (x, y)) in a.iter().zip(&stitched).enumerate() {
                    let (px, py) = (k / 4 % w, k / 4 / w);
                    if px < M || py < M || px >= w - M || py >= h - M {
                        continue;
                    }
                    let d = (*x as i32 - *y as i32).abs();
                    worst = worst.max(d);
                    sum += d as i64;
                    if d > 3 && bad.len() < 12 {
                        bad.push((px, py, d));
                    }
                }
                let mean = sum as f64 / a.len() as f64;
                eprintln!("tiling bpp {bpp} q{q} rot {rot} far_fog {far_fog} {tx}x{ty}: worst {worst}, mean {mean:.4}");
                assert!(worst <= 3 && mean < 0.02, "bpp {bpp} q{q} rot {rot} far_fog {far_fog} tiles {tx}x{ty}: worst {worst}, mean {mean:.4}, at {bad:?}");
            }
        }
    }
}

// ---- 0.3.4: localisation completeness, no hardcoded words, settings rows that fit ----
use crate::engine::settings;

/// Minecraft's default font advance, GUI pixels (a model good enough to size labels: the real widths come from
/// the game at run time).
fn mc_width(t: &str) -> f32 {
    t.chars()
        .map(|c| match c {
            'i' | '!' | '.' | ',' | ':' | ';' | '|' | '\'' => 2.0,
            'l' | '`' => 3.0,
            't' | 'I' | '[' | ']' | ' ' | '(' | ')' | '"' | '*' => 4.0,
            'f' | 'k' | '<' | '>' | '{' | '}' => 5.0,
            'Ж' | 'Ш' | 'Щ' | 'Ю' | 'Ы' | 'ж' | 'ш' | 'щ' | 'ю' | 'ы' | 'М' | 'м' | 'W' | 'w' | 'M' | 'm' | '@' | '~' => 8.0,
            _ => 6.0,
        })
        .sum()
}

#[test]
fn lang_every_key_in_both_languages() {
    use crate::engine::lang;
    let mut seen = std::collections::HashSet::new();
    for (k, en, ru) in lang::TABLE.iter().chain(lang::UI.iter()) {
        assert!(seen.insert(*k), "duplicate lang key {k}");
        assert!(!en.trim().is_empty(), "{k}: empty en");
        assert!(!ru.trim().is_empty(), "{k}: empty ru");
        // Placeholders agree between the languages.
        for n in 0..6 {
            let p = format!("{{{n}}}");
            assert_eq!(en.contains(&p), ru.contains(&p), "{k}: {p} in one language only");
        }
        assert_eq!(lang::builtin(Lang::En, k), Some(*en));
        assert_eq!(lang::builtin(Lang::Ru, k), Some(*ru));
    }
    for (k, game) in lang::GAME {
        assert!(seen.contains(k), "game override for unknown key {k}");
        assert!(!game.is_empty());
    }
    // A key with a game translation takes it only when the game speaks the map's language.
    fn game(k: &str) -> Option<String> {
        (k == "options.on").then(|| "AN (game)".to_string())
    }
    let with = Tr { lang: Lang::En, game: Some(game) };
    assert_eq!(with.t("megamap.on"), "AN (game)");
    assert_eq!(Tr::new(Lang::Ru).t("megamap.on"), "вкл");
    assert_eq!(Tr::new(Lang::En).f("megamap.toast.death", &[&1, &-2, &3]), "Death point saved at 1 -2 3");
    assert_eq!(Tr::new(Lang::En).t("megamap.no.such.key"), "megamap.no.such.key");
}

/// The string literals of a line of Rust (escapes kept as written), raw strings left out.
fn literals(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    let mut prev = ' ';
    while let Some(c) = chars.next() {
        if c == '/' && chars.peek() == Some(&'/') {
            break;
        }
        if c == '\'' {
            // A char literal ('"' among them) or a lifetime.
            let rest: String = chars.clone().take(3).collect();
            if rest.starts_with("\\") {
                let n = rest.find('\'').map_or(0, |i| i + 1);
                for _ in 0..n.max(1) {
                    chars.next();
                }
            } else if rest.chars().nth(1) == Some('\'') {
                chars.next();
                chars.next();
            }
            prev = c;
            continue;
        }
        if c == '"' && prev != 'r' && prev != '#' {
            let mut s = String::new();
            while let Some(d) = chars.next() {
                match d {
                    '\\' => {
                        s.push(d);
                        if let Some(e) = chars.next() {
                            s.push(e);
                        }
                    }
                    '"' => break,
                    _ => s.push(d),
                }
            }
            out.push(s);
        }
        prev = c;
    }
    out
}

/// A literal that reads as words for the player: Cyrillic, or English words (a capitalised word with a space or
/// colon after it, or two lowercase words).
fn wordy(s: &str) -> bool {
    if s.starts_with("megamap.") || s.starts_with("megamap:") || s.starts_with("key.") || s.starts_with("minecraft:") {
        return false;
    }
    if s.chars().any(|c| ('\u{400}'..='\u{4FF}').contains(&c)) {
        return true;
    }
    let words: Vec<&str> = s.split(|c: char| !c.is_ascii_alphabetic()).filter(|w| !w.is_empty()).collect();
    let cap = words.iter().any(|w| w.len() >= 3 && w.chars().next().unwrap().is_ascii_uppercase() && w[1..].chars().all(|c| c.is_ascii_lowercase()));
    let lower_pair = s.split(' ').filter(|w| w.len() >= 3 && w.chars().all(|c| c.is_ascii_lowercase())).count() >= 2;
    (cap && (s.contains(' ') || s.contains(": "))) || lower_pair
}

#[test]
fn lang_no_hardcoded_words_in_ui_sources() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = manifest.join("src");
    let glue = manifest.join("../horizon-client/src/game/tachyon_mods/megamap_glue.rs");
    let files: Vec<(String, std::path::PathBuf)> = vec![
        ("megamap_glue.rs".into(), glue),
        ("settings.rs".into(), src.join("settings.rs")),
        ("ui.rs".into(), src.join("ui.rs")),
        ("entities.rs".into(), src.join("entities.rs")),
        ("waypoints.rs".into(), src.join("waypoints.rs")),
        ("layers.rs".into(), src.join("layers.rs")),
        ("color.rs".into(), src.join("color.rs")),
    ];
    let skip = ["eprintln!", "println!", "log::", "expect(", "panic!", "assert", "unreachable!", "debug_assert", "#[", "// lang-ok", "format!(\"{", "write!("];
    let mut bad = Vec::new();
    for (label, path) in files {
        let text = std::fs::read_to_string(&path).unwrap();
        let mut in_tests = false;
        for (n, line) in text.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("#[cfg(test)]") {
                in_tests = true;
            }
            if in_tests || t.starts_with("//") || skip.iter().any(|s| line.contains(s)) {
                continue;
            }
            for lit in literals(line) {
                if wordy(&lit) {
                    bad.push(format!("{label}:{}: \"{lit}\"", n + 1));
                }
            }
        }
    }
    assert!(bad.is_empty(), "words outside the lang table:\n{}", bad.join("\n"));
}

#[test]
fn lang_every_used_key_exists() {
    use crate::engine::lang;
    let known: std::collections::HashSet<&str> = lang::TABLE.iter().chain(lang::UI.iter()).map(|e| e.0).collect();
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = vec![manifest.join("../horizon-client/src/game/tachyon_mods/megamap_glue.rs")];
    for e in std::fs::read_dir(manifest.join("src")).unwrap() {
        let p = e.unwrap().path();
        if p.extension().and_then(|s| s.to_str()) == Some("rs") {
            files.push(p);
        }
    }
    let mut missing = Vec::new();
    for f in files {
        if f.ends_with("lang.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap();
        for line in text.lines() {
            for lit in literals(line) {
                if lit.starts_with("megamap.") && !lit.contains('{') && !lit.ends_with('.') && !known.contains(lit.as_str()) {
                    missing.push(format!("{}: {lit}", f.file_name().unwrap().to_string_lossy()));
                }
            }
        }
    }
    // Keys the code builds with format!.
    for l in ["surface", "cave", "fixed_y", "auto"] {
        let k = format!("megamap.layer.{l}");
        if !known.contains(k.as_str()) {
            missing.push(k);
        }
    }
    for n in 0..4 {
        for p in ["relief", "grid"] {
            let k = format!("megamap.{p}.{n}");
            if !known.contains(k.as_str()) {
                missing.push(k);
            }
        }
    }
    for i in 0..settings::BUTTON_COUNT {
        if !known.contains(settings::hint(i)) {
            missing.push(settings::hint(i).to_string());
        }
    }
    for c in settings::CATEGORIES.iter() {
        for k in std::iter::once(c.name).chain(c.groups.iter().map(|g| g.0)) {
            if !known.contains(k) {
                missing.push(k.to_string());
            }
        }
    }
    assert!(missing.is_empty(), "keys missing from the lang table:\n{}", missing.join("\n"));
}

#[test]
fn lang_settings_rows_have_no_raw_keys_in_any_state() {
    for l in [Lang::En, Lang::Ru] {
        let tr = Tr::new(l);
        for i in 0..settings::BUTTON_COUNT {
            if i == settings::RESET_ALL {
                continue;
            }
            let mut s = Settings::default();
            for _ in 0..9 {
                let (name, value) = s.item(i, &tr);
                assert!(!name.is_empty(), "{l:?} item {i} has no name");
                assert!(!name.contains("megamap.") && !value.contains("megamap."), "{l:?} item {i}: {name}: {value}");
                s.click(i);
            }
        }
    }
}

/// The settings rows as the glue lays them out: name and value never overlap and stay inside the row at the GUI
/// sizes of 1280x720 / 1920x1080 at GUI scales 2, 3 and 4, in both languages; at the default size (1920x1080 at
/// scale 3) no value is shortened.
#[test]
fn settings_rows_fit_at_gui_scales() {
    use crate::engine::ui::{self, PanelKind};
    let sizes = [(1920.0, 1080.0, 2.0), (1920.0, 1080.0, 3.0), (1920.0, 1080.0, 4.0), (1280.0, 720.0, 2.0), (1280.0, 720.0, 3.0)];
    for (sw, sh, scale) in sizes {
        for l in [Lang::En, Lang::Ru] {
            let tr = Tr::new(l);
            let mut ui = MapUi::default();
            ui.screen = Rect::new(0.0, 0.0, sw / scale, sh / scale);
            ui.tr = tr;
            ui.toggle_panel(PanelKind::Settings);
            let s = Settings::default();
            let (mut cut_names, mut cut_values, mut rows) = (0, 0, 0);
            for (c, cat) in settings::CATEGORIES.iter().enumerate() {
                // The nav entry: icon 20 px, the changed badge up to "99".
                if let Some(nr) = ui.settings_nav_rect(0) {
                    let (_, cut) = ui::fit(&tr.t(cat.name), nr.w - 21.0 - 14.0, &mc_width);
                    if scale <= 3.0 && sw >= 1920.0 {
                        assert!(!cut, "{l:?} {sw}@{scale}: category '{}' cut in the nav", tr.t(cat.name));
                    }
                }
                let rr = ui.panel_row_rect(0);
                for g in cat.groups {
                    let (_, cut) = ui::fit(&tr.t(g.0), rr.w - 4.0, &mc_width);
                    assert!(!cut || sw / scale < 500.0, "{l:?} {sw}@{scale}: group '{}' cut", tr.t(g.0));
                }
                for i in cat.items() {
                    let _ = c;
                    let (name, value) = s.item(i, &tr);
                    let boxed = (settings::INFO_FIRST..settings::INFO_FIRST + settings::Info::ALL.len()).contains(&i);
                    let right = match s.slider(i) {
                        Some(_) => ui.panel_slider_rect(0).x - 4.0,
                        None if boxed => rr.x + rr.w - 27.0,
                        None => rr.x + rr.w - 3.0,
                    };
                    let avail = right - rr.x - 9.0;
                    let row = ui::row_text(&name, &value, avail, &mc_width);
                    let used = mc_width(&row.name) + if row.value.is_empty() { 0.0 } else { ui::ROW_GAP + mc_width(&row.value) };
                    assert!(used <= avail + 0.01, "{l:?} {sw}@{scale}: '{name}: {value}' takes {used} of {avail}");
                    rows += 1;
                    cut_names += (row.name != name) as usize;
                    cut_values += (row.value != value) as usize;
                }
            }
            println!("{l:?} {sw}x{sh}@{scale}: panel {:.0} px, {rows} rows, names shortened {cut_names}, values shortened {cut_values}", ui.panel_rect().w);
            if sw >= 1920.0 && scale == 3.0 {
                assert_eq!(cut_values, 0, "{l:?}: a value is shortened at the default size");
                assert_eq!(cut_names, 0, "{l:?}: a name is shortened at the default size");
            }
        }
    }
}

/// 0.3.4 item 7: a synthetic strip of terrain walked at 4.3 blocks/s with a probe every 0.5 s, as the glue does.
/// x 0..40 plains, 40..80 a hill with a tunnel through it, 100..104 a short underpass, 140..150 a closed house,
/// 156..166 a tree, 170..180 a stone bridge, 190..230 a lake the player dives into, 242..278 a tall cavern
/// (ceiling 60 blocks over the floor, past the roof reach).
mod scene {
    use crate::engine::baker::{ChunkSource, ColorTable, MC_PLANT, MC_WATER};
    pub const AIR: u32 = 0;
    pub const STONE: u32 = 1;
    pub const GRASS: u32 = 2;
    pub const WATER: u32 = 3;
    pub const PLANKS: u32 = 4;
    pub const LOG: u32 = 5;
    pub const LEAVES: u32 = 6;
    pub const GLASS: u32 = 7;
    pub fn table() -> ColorTable {
        let mut t = ColorTable::with_len(8);
        t.set(AIR, 0, 0);
        t.set(STONE, 11, 0x707070);
        t.set(GRASS, 1, 0x7FB238);
        t.set(WATER, MC_WATER, 0x4040FF);
        t.set(PLANKS, 13, 0x8F7748);
        t.set(LOG, 13, 0x8F7748);
        t.set(LEAVES, MC_PLANT, 0x007C00);
        t.set(GLASS, 0, 0);
        t
    }
    pub struct Scene;
    impl Scene {
        fn ground(&self, _x: i32, y: i32) -> u32 {
            if y < 60 { STONE } else if y < 64 { GRASS } else { AIR }
        }
    }
    impl ChunkSource for Scene {
        fn block(&self, [x, y, z]: [i32; 3]) -> u32 {
            let zin = |w: i32| z.abs() <= w;
            match x {
                40..80 => {
                    if zin(2) && (64..67).contains(&y) { AIR } else if y < 100 { STONE } else { AIR }
                }
                100..104 => {
                    if zin(2) && (64..67).contains(&y) { AIR } else if y < 70 && zin(6) { STONE } else { self.ground(x, y) }
                }
                140..150 if zin(4) => {
                    let wall = x == 140 || x == 149 || z.abs() == 4;
                    if y == 68 || (wall && (64..68).contains(&y)) { PLANKS } else if y == 66 && z.abs() == 4 { GLASS } else { self.ground(x, y) }
                }
                156..166 if zin(4) => {
                    if x == 160 && z == 0 && (64..71).contains(&y) { LOG } else if (68..74).contains(&y) { LEAVES } else { self.ground(x, y) }
                }
                170..180 if zin(3) && (70..72).contains(&y) => STONE,
                190..230 => {
                    if y < 48 { STONE } else if y < 63 { WATER } else { AIR }
                }
                240..280 => {
                    if (242..278).contains(&x) && (20..80).contains(&y) { AIR } else if y < 120 { STONE } else { AIR }
                }
                _ => self.ground(x, y),
            }
        }
        fn biome(&self, _: [i32; 3]) -> Option<u16> {
            Some(1)
        }
        fn section_top(&self, [x, _z]: [i32; 2]) -> i32 {
            // A chunk-wide value, as the game's: the hill and the cavern raise their chunks.
            match x >> 4 {
                2..=4 => 112,
                15..=17 => 128,
                _ => 80,
            }
        }
        fn loaded(&self, _: [i32; 2]) -> bool {
            true
        }
        fn min_y(&self) -> i32 {
            -64
        }
    }
    /// The player's feet height and sky light at `x` on the walk (the light as the game would have it at night
    /// inside the house: 0, and fading in the tunnel's first blocks).
    pub fn at(x: i32) -> (i32, u8) {
        match x {
            40..80 => (64, [12u8, 9, 6, 3].get((x - 40) as usize).copied().unwrap_or(0).max([12u8, 9, 6, 3].get((79 - x) as usize).copied().unwrap_or(0))),
            100..104 => (64, 11),
            141..149 => (64, 0),
            157..164 => (64, 13),
            171..179 => (64, 14),
            190..200 | 220..230 => (61, 14),
            200..220 => (50, 2),
            242..278 => (21, 0),
            _ => (64, 15),
        }
    }
}

#[test]
fn auto_layer_roof_check_on_a_synthetic_scene() {
    use crate::engine::baker::ChunkSource;
    use crate::engine::layers::{self, Detector, Evidence, LayerMode, Probe, SliceTracker};
    use scene::Scene;
    let t = scene::table();
    let ev = |x: i32, size: u8| {
        let (y, sky) = scene::at(x);
        layers::roof_evidence(layers::roof_probe(&Scene, &t, [x, y, 0], size, 24), Some(sky))
    };
    // Single spots.
    assert_eq!(ev(20, 3), Evidence::Surface, "plains");
    assert_eq!(ev(60, 3), Evidence::Cave, "tunnel under the hill");
    assert_eq!(ev(40, 3), Evidence::Surface, "tunnel mouth, bright");
    assert_eq!(ev(145, 3), Evidence::Surface, "closed house, dark: a thin roof is a building");
    assert_eq!(ev(161, 3), Evidence::Surface, "under the tree next to the trunk");
    assert_eq!(ev(160, 1), Evidence::Surface, "leaves are no roof");
    assert_eq!(ev(175, 3), Evidence::Surface, "under the bridge");
    assert_eq!(ev(210, 3), Evidence::Surface, "under 12 blocks of water, sky light 2");
    assert_eq!(ev(260, 3), Evidence::Cave, "tall cavern: no roof in reach, dark");
    let r = layers::roof_probe(&Scene, &t, [145, 64, 0], 3, 24);
    assert_eq!((r.columns, r.thin, r.thick, r.lowest), (9, 9, 0, Some(68)));
    assert_eq!(layers::roof_probe(&Scene, &t, [60, 64, 0], 5, 24).thick, 25);
    assert_eq!(layers::roof_probe(&Scene, &t, [60, 64, 0], 0, 24), layers::Roof::default());

    // The walk: every 0.5 s, 4.3 blocks/s, 0..240; the old rule (sky light or height) next to the roof check.
    let (enter, exit) = layers::dwell(10);
    assert_eq!((enter, exit), (1.0, 0.5));
    let (mut d, mut old) = (Detector::default(), Detector::default());
    let (mut flips, mut old_flips) = (Vec::new(), Vec::new());
    let mut cave_xs = Vec::new();
    let mut tick = 0;
    loop {
        let tt = tick as f32 * 0.5;
        let x = (tt * 4.3) as i32;
        if x >= 240 {
            break;
        }
        let (y, sky) = scene::at(x);
        if d.step(tt, ev(x, 3), enter, exit) {
            flips.push((x, d.cave));
        }
        // The surface map's top block of the column, as the glue reads it (the roof, the leaves, the water top).
        let ground = (y..140).rev().find(|&yy| scene::table().get(Scene.block([x, yy, 0])).is_some_and(|c| c.0 != 0)).map(|g| g + 1);
        if old.update(tt, Probe { y, sky: Some(sky), ground }, 0) {
            old_flips.push((x, old.cave));
        }
        if d.cave {
            cave_xs.push(x);
        }
        tick += 1;
    }
    println!("roof check flips (x, cave): {flips:?}");
    println!("sky/height flips (x, cave): {old_flips:?}");
    // Exactly one cave stretch: into the tunnel after the delay, out at its far mouth; the short underpass, the
    // house, the tree, the bridge and the lake keep the surface.
    assert_eq!(flips.len(), 2, "{flips:?}");
    assert!(flips[0].1 && (44..=52).contains(&flips[0].0), "{flips:?}");
    assert!(!flips[1].1 && (76..=84).contains(&flips[1].0), "{flips:?}");
    assert!(cave_xs.iter().all(|x| (40..86).contains(x)), "{cave_xs:?}");
    // The old rule flipped in the house and the lake.
    assert!(old_flips.len() > flips.len(), "{old_flips:?}");

    // Flicker: standing at the tunnel mouth where the light swings 9..11 and the roof comes and goes every probe.
    let mut d = Detector::default();
    let mut n = 0;
    for i in 0..60 {
        let x = if i % 2 == 0 { 40 } else { 42 };
        n += d.step(i as f32 * 0.5, ev(x, 3), enter, exit) as usize;
    }
    assert_eq!(n, 0, "no flips at the cave mouth");
    // Walking in and out of the mouth with 1 s on each side flips each time it holds the delay, never faster.
    let mut d = Detector::default();
    let mut times = Vec::new();
    for i in 0..40 {
        let x = if (i / 4) % 2 == 0 { 30 } else { 60 };
        if d.step(i as f32 * 0.5, ev(x, 3), enter, exit) {
            times.push(i as f32 * 0.5);
        }
    }
    assert!(times.windows(2).all(|w| w[1] - w[0] >= 1.0), "{times:?}");

    // The tall cavern: cave after the delay, from a fresh start.
    let mut d = Detector::default();
    let c = (0..4).map(|i| d.step(i as f32 * 0.5, ev(260, 3), enter, exit)).filter(|&f| f).count();
    assert!(c == 1 && d.cave);

    // The slice height: steps of 8 with 2 blocks of margin, following the player down a staircase and back up.
    let mut st = SliceTracker::default();
    let mut tops = Vec::new();
    for y in (0..=40).rev().chain(0..=40) {
        let top = st.top(y, 8);
        if tops.last() != Some(&top) {
            tops.push(top);
        }
        // Never under the feet, never more than a step and the margin over the head.
        assert!(top >= y && top <= y + 8 + 1 + layers::SLICE_HYST, "y {y} top {top}");
    }
    println!("cave slice tops down and up: {tops:?}");
    assert_eq!(tops, [49, 41, 33, 25, 17, 9, 17, 25, 33, 41]);
    // At the step's edge, a block up and down does not move it.
    let mut st = SliceTracker::default();
    let first = st.top(16, 8);
    assert!((0..20).all(|i| st.top(if i % 2 == 0 { 15 } else { 16 }, 8) == first));
    assert_eq!(SliceTracker::default().top(20, 16), 33);
    // Fixed Y and manual cave are kept.
    assert_eq!(layers::slice_at(LayerMode::FixedY, true, 33, 64), Some(64));
    assert_eq!(layers::slice_at(LayerMode::Cave, false, 33, 64), Some(33));
    assert_eq!(layers::slice_at(LayerMode::Auto, false, 33, 64), None);
    assert_eq!(layers::slice_at(LayerMode::Surface, true, 33, 64), None);
}

// ---- 0.3.4: indoors, the ceiling cut and the minimap zoom ----

#[test]
fn indoor_room_cut_and_zoom() {
    use crate::engine::baker::{bake_chunk, Layer};
    use crate::engine::layers::{self, Detector, Evidence};
    use crate::fake::Plains;
    use scene::Scene;
    let t = scene::table();
    let room = |x: i32| {
        let (y, _) = scene::at(x);
        let r = layers::roof_probe(&Scene, &t, [x, y, 0], 3, 24);
        (layers::is_room(r, y), r.walls)
    };
    assert_eq!(room(145), (true, 4), "inside the house");
    assert!(!room(175).0, "under the bridge: no walls");
    assert!(!room(161).0, "under the tree");
    assert!(!room(60).0, "in the tunnel: a cave, not a room");
    assert!(!room(20).0, "on the plains");
    assert!(!room(210).0, "under the lake");

    // The cut: head height, kept a block up or down, never into the ceiling.
    assert_eq!(layers::indoor_cut(None, 64, Some(68)), 65);
    assert_eq!(layers::indoor_cut(Some(65), 65, Some(68)), 65, "a step up keeps it");
    assert_eq!(layers::indoor_cut(Some(65), 63, Some(68)), 65, "a step down keeps it");
    assert_eq!(layers::indoor_cut(Some(65), 67, Some(72)), 68, "a storey up moves it");
    assert_eq!(layers::indoor_cut(Some(66), 64, Some(66)), 65, "never at the ceiling");

    // Baked indoors: the floor and the walls instead of the roof.
    let surf = bake_chunk(&Scene, &Plains, &t, 9, 0, Layer::Surface).unwrap();
    let cut = bake_chunk(&Scene, &Plains, &t, 9, 0, Layer::Indoor(65)).unwrap();
    let (inside, wall) = (1usize, 5usize); // x 145 and 149 at z 0
    assert_eq!(surf.height[inside], 68, "the surface shows the roof");
    assert_eq!(cut.height[inside], 63, "indoors shows the floor");
    assert_eq!(cut.height[wall], 65, "and the walls at the cut");
    assert_ne!(cut.color[inside], surf.color[inside]);

    // Into and out of the house: the room view after two probes, no flicker on a doorstep that alternates.
    let mut d = Detector::default();
    let ev = |x: i32| if room(x).0 { Evidence::Cave } else { Evidence::Surface };
    let walk: Vec<bool> = (0..12)
        .map(|i| {
            let x = if (2..6).contains(&i) { 145 } else { 130 };
            d.step(i as f32 * 0.5, ev(x), layers::INDOOR_ENTER_S, layers::INDOOR_EXIT_S);
            d.cave
        })
        .collect();
    // Out 1.2 s after the last "room" probe (0.3.10; 2 s before): the third half-second step outside.
    assert_eq!(walk, [false, false, false, true, true, true, true, true, true, false, false, false]);
    // One odd probe indoors (a window, a door) does not leave the room view.
    let mut d = Detector::default();
    d.reset(true);
    let odd: Vec<bool> = (0..6)
        .map(|i| {
            let e = if i == 2 { Evidence::Surface } else { Evidence::Cave };
            d.step(i as f32 * 0.5, e, layers::INDOOR_ENTER_S, layers::INDOOR_EXIT_S);
            d.cave
        })
        .collect();
    assert!(odd.iter().all(|&c| c), "one odd probe: still a room");
    let mut d = Detector::default();
    d.reset(true);
    d.step(0.0, Evidence::Unsure, layers::INDOOR_ENTER_S, layers::INDOOR_EXIT_S);
    d.step(9.0, Evidence::Unsure, layers::INDOOR_ENTER_S, layers::INDOOR_EXIT_S);
    assert!(d.cave, "no roof data: unsure keeps the view");
    // Walking past a window: the looser check keeps the room where the strict one lets go.
    let (y, _) = scene::at(145);
    let r = layers::roof_probe(&Scene, &t, [145, y, 0], 3, 24);
    assert!(layers::is_room_keep(r, y), "inside the house, the keeping check holds");

    // The zoom: x2 by default, eased in about a third of a second, none with the cut only or off.
    assert_eq!(layers::indoor_zoom(2), 0.5);
    assert_eq!(layers::indoor_zoom(1), 1.0);
    assert_eq!(layers::indoor_zoom(0), 1.0);
    let mut k = 1.0;
    let mut frames = 0;
    while k != 0.5 {
        k = layers::ease_zoom(k, 0.5, 1.0 / 60.0);
        frames += 1;
        assert!(frames < 60, "eases within a second");
    }
    assert!(frames > 10, "not a jump: {frames} frames");
}

// ---- 0.3.4: a minimum feather of the explored edge in pixels, inside the hard reach ----

/// At 4 blocks a pixel the fill used to end in a 1.25 px step that traced the chunk staircase. The fade is now
/// at least `FEATHER_MIN_PX` (or the whole reach when that is narrower), and still exactly 0 past the reach.
#[test]
fn clouds_min_feather_at_far_zoom() {
    use crate::engine::clouds::{reach_fade_px, reach_px, Clouds, Params, FEATHER_MIN_PX};
    let (w, h) = (320usize, 200usize);
    assert_eq!(reach_fade_px(40.0), 20.0);
    assert_eq!(reach_fade_px(6.0), FEATHER_MIN_PX);
    assert_eq!(reach_fade_px(2.5), 2.5);
    for (bpp, reach) in [(4.0f64, 10.0f32), (4.0, 16.0), (4.0, 24.0), (2.0, 10.0)] {
        let v = View::new([30.0, -20.0], bpp);
        let img = world_image(&v, w, h);
        let p = Params { quality: 2, density: 0.9, reach, ..Params::default() };
        let mut c = Clouds::new();
        c.update(&img, w, h, &v, 3.0, &p);
        c.composite(&mut img.clone(), w, h, &p, 3.0);
        let unk: Vec<(i32, i32)> = (0..w * h).filter(|&i| img[i * 4 + 3] == 0).map(|i| ((i % w) as i32, (i / w) as i32)).collect();
        let lim = reach_px(reach, bpp, w, h);
        // Per distance band (px): the smallest fill factor seen, and the widest band still soft (0.02..0.98).
        let (mut soft_w, mut jump) = (0f32, 0f32);
        let mut dist = vec![f32::MAX; w * h];
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                if img[i * 4 + 3] == 0 {
                    dist[i] = 0.0;
                    continue;
                }
                let d2 = unk.iter().map(|&(ux, uy)| (ux - x as i32).pow(2) + (uy - y as i32).pow(2)).min().unwrap();
                dist[i] = (d2 as f32).sqrt();
                let (g, _) = c.mask_at(x, y);
                if dist[i] > lim {
                    assert!(g <= 1.0 / 255.0, "bpp {bpp} reach {reach}: fill {g} past the limit at {x},{y}");
                }
            }
        }
        for y in 0..h - 1 {
            for x in 0..w - 1 {
                let i = y * w + x;
                let k = |x: usize, y: usize| c.kmask_at(x, y);
                if dist[i] > 0.0 && dist[i] <= lim {
                    if k(x, y) < 0.98 {
                        soft_w = soft_w.max(dist[i]);
                    }
                }
                for (x2, y2) in [(x + 1, y), (x, y + 1)] {
                    if dist[y2 * w + x2] > 0.0 || dist[i] > 0.0 {
                        jump = jump.max((k(x, y) - k(x2, y2)).abs());
                    }
                }
            }
        }
        let fade = reach_fade_px(lim);
        eprintln!("min feather: bpp {bpp} reach {reach}: limit {lim:.2} px, fade {fade:.2} px, soft to {soft_w:.2} px, max step {jump:.3}");
        assert!(fade >= FEATHER_MIN_PX.min(lim) - 1e-4);
        assert!(jump <= 1.5 / fade + 0.05, "bpp {bpp} reach {reach}: step {jump} past {}", 1.5 / fade);
    }
}

// ---- fog fade, block light, textures, smoothing ----

#[test]
fn light_encoding_and_average4() {
    use crate::engine::color;
    for l in 0..=15 {
        let c = color::land(0x123456, l);
        assert_eq!(color::block_light(c), l);
        assert_eq!(color::water_depth(c), 0);
        assert_eq!(c & 0xFF_FFFF, 0x123456);
    }
    assert_eq!(color::land(0x123456, 0) >> 24, 0xFF, "unlit land stays opaque");
    let a = color::average4([color::land(0x101010, 0), color::land(0x101010, 12), color::land(0x101010, 3), 0]);
    assert_eq!(color::block_light(a), 12, "the brightest light of the four");
}

struct Torch(World);
impl crate::engine::baker::ChunkSource for Torch {
    fn block(&self, p: [i32; 3]) -> u32 {
        if p[0] == 8 && p[2] == 8 && p[1] == self.0.height(8, 8) - 1 {
            6
        } else {
            self.0.block(p)
        }
    }
    fn biome(&self, p: [i32; 3]) -> Option<u16> {
        self.0.biome(p)
    }
    fn section_top(&self, p: [i32; 2]) -> i32 {
        self.0.section_top(p)
    }
    fn loaded(&self, p: [i32; 2]) -> bool {
        self.0.loaded(p)
    }
    fn min_y(&self) -> i32 {
        self.0.min_y()
    }
}

#[test]
fn bake_block_light_around_an_emitter() {
    use crate::engine::color::block_light;
    let w = Torch(World { r: 2, cave: false });
    let mut t = table();
    t.set(6, 11, 0xFFE080);
    let dark = bake_chunk(&w, &Plains, &t, 0, 0, Layer::Surface).unwrap();
    assert!(dark.color.iter().all(|&c| block_light(c) == 0), "no emitters known: no light");
    t.set_emit(6, 15);
    let c = bake_chunk(&w, &Plains, &t, 0, 0, Layer::Surface).unwrap();
    let at = |x: usize, z: usize| block_light(c.color[z * 16 + x]);
    assert!(at(8, 8) >= 13, "the emitter's column is bright: {}", at(8, 8));
    assert!(at(10, 8) < at(8, 8) && at(10, 8) > 0, "falls off");
    assert!(at(0, 0) < at(6, 6));
    // The neighbour chunk is lit across the border.
    let n = bake_chunk(&w, &Plains, &t, 1, 0, Layer::Surface).unwrap();
    assert!(block_light(n.color[8 * 16]) > 0);
    let far = bake_chunk(&w, &Plains, &t, -2, -2, Layer::Surface).unwrap();
    assert!(far.color.iter().all(|&c| block_light(c) == 0));
}

#[test]
fn fresh_marking_feeds_the_front() {
    let w = World { r: 2, cave: false };
    let t = table();
    let c = bake_chunk(&w, &Plains, &t, 0, 0, Layer::Surface).unwrap();
    // Not tracking (the far layer, a settling cave slice): never fresh.
    let mut s = Store::new(64);
    s.tried.insert((0, 0));
    s.put(&c);
    assert!(s.front.is_empty());
    // Tracking but the region file has not answered: maybe ground from an earlier session, not news.
    let mut s = Store::new(64);
    s.track_fresh = true;
    s.put(&c);
    assert!(s.front.is_empty());
    // The cave slice has no files: news at once.
    let mut s = Store::new(64);
    s.track_fresh = true;
    s.memory_only = true;
    s.put(&c);
    assert_eq!(s.front.len(), 1);
    // Answered: the first put is news (hidden until the front passes), a repeat is not.
    let mut s = Store::new(64);
    s.track_fresh = true;
    s.tried.insert((0, 0));
    s.put(&c);
    assert_eq!(s.front.len(), 1);
    assert_eq!(s.front.amount(5, 5, reveal::SOFT), 0);
    assert_eq!(s.front.amount(20, 5, reveal::SOFT), 256);
    assert!(s.chunk_explored(0, 0) && !s.chunk_explored(1, 0));
    s.front.clear();
    s.put(&c);
    assert!(s.front.is_empty(), "explored before");
}

/// A walk east at sprint speed through unexplored ground, the loaded square following the player: chunks are
/// explored as they come into range and step into the front every frame.
struct Walk {
    seen: std::collections::HashSet<(i32, i32)>,
    front: reveal::Front,
    pos: [f64; 2],
}

impl Walk {
    const RADIUS: i32 = 6;
    fn new(pos: [f64; 2]) -> Self {
        let mut w = Walk { seen: Default::default(), front: Default::default(), pos };
        // Ground explored in an earlier session around the start.
        w.load(false);
        w
    }
    fn load(&mut self, news: bool) {
        let (pcx, pcz) = ((self.pos[0].floor() as i32) >> 4, (self.pos[1].floor() as i32) >> 4);
        for cz in pcz - Self::RADIUS..=pcz + Self::RADIUS {
            for cx in pcx - Self::RADIUS..=pcx + Self::RADIUS {
                if self.seen.insert((cx, cz)) && news {
                    self.front.add(cx, cz);
                }
            }
        }
    }
    fn tick(&mut self, dt: f32, sp: reveal::Speed) -> bool {
        self.load(true);
        let seen = &self.seen;
        self.front.step(dt, self.pos, sp, reveal::SOFT, |cx, cz| seen.contains(&(cx, cz)))
    }
    /// What the map shows at block (`x`, `z`): `None` unexplored (fog whatever the front does), else the amount.
    fn shown(&self, x: i32, z: i32) -> Option<u32> {
        self.seen.contains(&(x >> 4, z >> 4)).then(|| self.front.amount(x, z, reveal::SOFT))
    }
}

#[test]
fn reveal_front_walk_is_monotonic_and_continuous() {
    let sp = reveal::speed(2).unwrap();
    let mut w = Walk::new([8.0, 8.0]);
    assert!(w.front.is_empty());
    let dt = 1.0 / 60.0;
    // Blocks along a row through the strip the walk explores, and a few rows across it.
    let rows = [-40, -17, 0, 8, 23, 61];
    let xs: Vec<i32> = (96..420).collect();
    let mut last = vec![vec![None::<u32>; xs.len()]; rows.len()];
    let mut edge_x = Vec::new();
    for f in 0..60 * 50 {
        if f < 60 * 40 {
            w.pos[0] += 5.6 * dt as f64;
        }
        w.tick(dt, sp);
        for (ri, &z) in rows.iter().enumerate() {
            for (xi, &x) in xs.iter().enumerate() {
                let now = w.shown(x, z);
                match (last[ri][xi], now) {
                    // Unexplored stays fogged; once explored a block only ever comes further out of the fog.
                    (Some(a), Some(b)) => assert!(b >= a, "block ({x}, {z}) went back into the fog: {a} -> {b}, frame {f}"),
                    (Some(_), None) => panic!("explored block forgotten"),
                    _ => {}
                }
                last[ri][xi] = now;
            }
        }
        // Along the fog edge (x at fixed z), no chunk-sized steps: neighbours differ by at most a ramp's share,
        // also across chunk borders (x = 16 k - 1 | 16 k).
        for &z in &rows {
            for x in 96..419 {
                if let (Some(a), Some(b)) = (w.shown(x, z), w.shown(x + 1, z)) {
                    let (a, b) = (a as i32, b as i32);
                    let lim = if x & 15 == 15 { 256 } else { 120 };
                    assert!((a - b).abs() <= lim, "step at x {x}, z {z}: {a} vs {b}, frame {f}");
                }
            }
        }
        // Across chunk borders along z (the direction the front runs parallel to): continuous too.
        for x in (100..400).step_by(7) {
            for z in -60..60 {
                if let (Some(a), Some(b)) = (w.shown(x, z), w.shown(x, z + 1)) {
                    assert!((a as i32 - b as i32).abs() <= 120, "step across z at ({x}, {z}): {a} vs {b}, frame {f}");
                }
            }
        }
        if f % 30 == 0 {
            // The front on row z = 8: the farthest explored block that is half out.
            let e = (96..420).filter(|&x| w.shown(x, 8).is_some_and(|a| a >= 128)).max();
            edge_x.push(e);
        }
    }
    // The front moved east with the player, never back.
    let e: Vec<i32> = edge_x.iter().flatten().copied().collect();
    assert!(e.len() > 20);
    assert!(e.windows(2).all(|p| p[1] >= p[0]), "front went back: {e:?}");
    assert!(*e.last().unwrap() > 300, "front kept up: {e:?}");
    // Everything explored is out once the walk stops and the front catches up.
    assert!(w.front.is_empty(), "{} chunks still in the fog", w.front.len());
}

#[test]
fn reveal_front_grows_from_the_player_after_teleport() {
    let sp = reveal::speed(2).unwrap();
    let mut w = Walk::new([8.0, 8.0]);
    let dt = 1.0 / 60.0;
    // Far away, nothing seen there before: the loaded square comes in over a few frames.
    w.pos = [100_000.5, 100_000.5];
    w.tick(dt, sp);
    let (px, pz) = (100_000, 100_000);
    let ring = |w: &Walk, r: i32| -> Vec<u32> {
        (0..32).map(|k| {
            let a = k as f32 / 32.0 * std::f32::consts::TAU;
            w.shown(px + (a.cos() * r as f32) as i32, pz + (a.sin() * r as f32) as i32).unwrap()
        }).collect()
    };
    // Right after: all of it hidden, then it opens up around the player, nearer ground first.
    assert!(ring(&w, 3).iter().all(|&a| a == 0));
    let mut opened = [None::<usize>; 4];
    let radii = [4, 24, 48, 80];
    for f in 0..60 * 20 {
        w.tick(dt, sp);
        for (i, &r) in radii.iter().enumerate() {
            let a = ring(&w, r);
            // Round: every direction comes out together (no square, no chunk order).
            let (lo, hi) = (*a.iter().min().unwrap(), *a.iter().max().unwrap());
            assert!(hi - lo <= 140, "ring {r} uneven at frame {f}: {lo}..{hi}");
            if opened[i].is_none() && lo >= 128 {
                opened[i] = Some(f);
            }
        }
    }
    let o: Vec<usize> = opened.iter().map(|o| o.expect("ring never opened")).collect();
    assert!(o.windows(2).all(|p| p[1] > p[0]), "nearer first: {o:?}");
    // Not a pop: the 80-block ring takes a visible while (over half a second) but is out within seconds.
    assert!(o[3] > 30 && o[3] < 60 * 8, "{o:?}");
    // The place left behind is not in the field (out at once, it was explored before).
    assert_eq!(w.shown(8, 8), Some(256));
}

#[test]
fn reveal_speed_setting() {
    assert!(reveal::speed(0).is_none());
    let (s1, s2, s3) = (reveal::speed(1).unwrap(), reveal::speed(2).unwrap(), reveal::speed(3).unwrap());
    assert!(s1.base < s2.base && s2.base < s3.base && s1.tau > s3.tau);
    assert!(reveal::soft_for(0.5) == reveal::SOFT && reveal::soft_for(4.0) == 12.0 && reveal::soft_for(100.0) == reveal::SOFT_MAX);
    let mut s = Settings::default();
    assert_eq!(s.reveal, 2);
    for want in [3, 0, 1, 2] {
        s.click(settings::REVEAL);
        assert_eq!(s.reveal, want);
    }
    s.reveal = 1;
    assert_eq!(Settings::from_text(&s.to_text()).reveal, 1);
    assert_eq!(Settings::from_text("reveal=9\n").reveal, 2);
}

struct Revealing<'a>(&'a Store, u32);
impl Lookup for Revealing<'_> {
    fn region(&self, rx: i32, rz: i32) -> Option<&Region> {
        self.0.get(rx, rz)
    }
    fn reveal(&self, x: i32, _z: i32) -> u32 {
        if x >> 4 == 0 { self.1 } else { 256 }
    }
}

#[test]
fn view_reveal_alpha_and_fog_mix() {
    assert_eq!(view::revealing(255), None);
    assert_eq!(view::revealing(view::FAR_ALPHA), None);
    assert_eq!(view::revealing(0), None);
    assert_eq!(view::revealing(view::REVEAL_ALPHA), Some(0));
    assert!(view::revealing(254).unwrap() >= 250);
    let s = baked_store(2);
    let (w, h) = (32usize, 32usize);
    let mut v = View::new([16.0, 16.0], 1.0);
    v.style.mark_unknown = true;
    let a_at = |buf: &[u8], x: usize, y: usize| buf[(y * w + x) * 4 + 3];
    let mut img = vec![0u8; w * h * 4];
    // Image x 8 is block x 8 (chunk 0), x 24 block 24 (chunk 1).
    view::render(&mut img, w, h, &v, &Revealing(&s, 128));
    let a = a_at(&img, 8, 16);
    assert!(view::revealing(a).is_some_and(|k| (110..=146).contains(&k)), "half way: {a}");
    assert_eq!(a_at(&img, 24, 16), 255);
    view::render(&mut img, w, h, &v, &Revealing(&s, 256));
    assert_eq!(a_at(&img, 8, 16), 255);
    // Without the cloud pass the fade mixes the fog tone in.
    v.style.mark_unknown = false;
    let mut done = vec![0u8; w * h * 4];
    view::render(&mut done, w, h, &v, &Revealing(&s, 256));
    view::render(&mut img, w, h, &v, &Revealing(&s, 0));
    let i = (16 * w + 8) * 4;
    assert_ne!(img[i..i + 3], done[i..i + 3]);
    assert_eq!(img[i + 3], 255);
}

/// New ground over far terrain, `reveal` of the way out; `live` false: only the far terrain is known.
struct OverFar<'a>(&'a Store, &'a Store, u32, bool);

impl Lookup for OverFar<'_> {
    fn region(&self, rx: i32, rz: i32) -> Option<&Region> {
        if self.3 { self.0.get(rx, rz) } else { None }
    }
    fn far_region(&self, rx: i32, rz: i32) -> Option<&Region> {
        self.1.get(rx, rz)
    }
    fn reveal(&self, _x: i32, _z: i32) -> u32 {
        self.2
    }
}

/// 0.3.7: a chunk loading over far (LOD) terrain replaces it with a cross-fade from the far colour; no fog comes
/// up in between (the cloud pass takes it for explored), unless the far terrain is fogged too (`far_fog`).
#[test]
fn reveal_over_far_terrain_fades_from_it() {
    let s = baked_store(2);
    let (w, h) = (32usize, 32usize);
    let mut v = View::new([16.0, 16.0], 1.0);
    for mark in [false, true] {
        v.style.mark_unknown = mark;
        let mut far_only = vec![0u8; w * h * 4];
        view::render(&mut far_only, w, h, &v, &OverFar(&s, &s, 256, false));
        let mut fresh = vec![0u8; w * h * 4];
        view::render(&mut fresh, w, h, &v, &OverFar(&s, &s, 0, true));
        let mut done = vec![0u8; w * h * 4];
        view::render(&mut done, w, h, &v, &OverFar(&s, &s, 256, true));
        let i = (16 * w + 8) * 4;
        assert_eq!(fresh[i..i + 3], far_only[i..i + 3], "mark {mark}: starts as the far terrain");
        assert_eq!(fresh[i + 3], 255, "mark {mark}: explored, no fog over it");
        let mut half = vec![0u8; w * h * 4];
        view::render(&mut half, w, h, &v, &OverFar(&s, &s, 128, true));
        for c in 0..3 {
            let (a, b) = (far_only[i + c].min(done[i + c]), far_only[i + c].max(done[i + c]));
            assert!((a.saturating_sub(1)..=b.saturating_add(1)).contains(&half[i + c]), "between far and live");
        }
    }
    // Fog over far terrain on: the new ground comes out of the fog as before.
    v.style.mark_unknown = true;
    v.style.far_fog = true;
    let mut fresh = vec![0u8; w * h * 4];
    view::render(&mut fresh, w, h, &v, &OverFar(&s, &s, 0, true));
    assert_eq!(view::revealing(fresh[(16 * w + 8) * 4 + 3]), Some(0));
}

/// 0.3.7: the cloud fog over a reveal ramp has one edge. Going from the fog to the ground the colour leaves the fog
/// once: no clear strip inside the ramp followed by a second fog rim past it (the Nether minimap of 0.3.6).
#[test]
fn fog_edge_over_reveal_ramp_is_single() {
    use crate::engine::clouds::{self, Clouds};
    let (w, h) = (192usize, 192usize);
    let st = Settings::default();
    for dim in ["minecraft:the_nether", "minecraft:overworld"] {
        let p = clouds::Params { reach: st.reach_blocks(true) as f32, ..st.cloud_params_for(dim) };
        for (ramp_at, ramp) in [(70usize, 12usize), (90, 6), (100, 24)] {
            let mut base = vec![0u8; w * h * 4];
            for y in 0..h {
                let a = if y < ramp_at {
                    0
                } else if y < ramp_at + ramp {
                    let t = (y - ramp_at) as f32 / ramp as f32;
                    view::REVEAL_ALPHA + ((t * t * (3.0 - 2.0 * t) * 256.0) as u32 * 125 / 256) as u8
                } else {
                    255
                };
                for x in 0..w {
                    base[(y * w + x) * 4..][..4].copy_from_slice(&[100, 100, 100, a]);
                }
            }
            view::settle_revealing(&mut base);
            let v = View::new([0.0, 0.0], 0.5);
            let mut c = Clouds::new();
            let mut img = base.clone();
            for t in [19.0f32, 20.0] {
                img.copy_from_slice(&base);
                assert!(c.update(&base, w, h, &v, t, &p));
                c.composite(&mut img, w, h, &p, t);
            }
            for x in [w / 4, w / 2, 3 * w / 4] {
                let diff = |y: usize| {
                    let o = &img[(y * w + x) * 4..];
                    (0..3).map(|i| (o[i] as i32 - 100).abs()).max().unwrap()
                };
                let clear = (ramp_at / 2..h).find(|&y| diff(y) < 12).expect("ground shows");
                let back = (clear..h).map(diff).max().unwrap();
                assert!(back < 40, "{dim} ramp {ramp_at}+{ramp} x {x}: fog again after the ground at {clear} ({back})");
            }
        }
    }
}

#[test]
fn view_night_glow_brightens_lit_ground() {
    use crate::engine::color;
    let mut s = Store::new(64);
    let w = World { r: 1, cave: false };
    let t = table();
    let mut c = bake_chunk(&w, &Plains, &t, 0, 0, Layer::Surface).unwrap();
    for z in 0..16 {
        for x in 0..8 {
            c.color[z * 16 + x] = color::land(c.color[z * 16 + x], 15);
        }
    }
    s.put(&c);
    let (iw, ih) = (16usize, 16usize);
    let mut v = View::new([8.0, 8.0], 1.0);
    v.fog = false;
    v.style.relief = 0;
    v.style.light = 40;
    let lum = |b: &[u8], x: usize| {
        let i = (8 * iw + x) * 4;
        b[i] as u32 + b[i + 1] as u32 + b[i + 2] as u32
    };
    let mut off = vec![0u8; iw * ih * 4];
    view::render(&mut off, iw, ih, &v, &StoreLookup(&s));
    v.style.glow = 256;
    let mut on = vec![0u8; iw * ih * 4];
    view::render(&mut on, iw, ih, &v, &StoreLookup(&s));
    assert!(lum(&on, 3) > lum(&off, 3) + 30, "lit half glows at night: {} vs {}", lum(&on, 3), lum(&off, 3));
    assert_eq!(on[(8 * iw + 12) * 4..][..4], off[(8 * iw + 12) * 4..][..4], "unlit half unchanged");
}

#[test]
fn detail_build_sample_apply() {
    use crate::engine::detail::{self, Detail};
    assert!(Detail::build(16, 16, &[0u8; 16 * 16 * 4]).is_none(), "fully clear");
    assert!(Detail::build(0, 0, &[]).is_none());
    // Checker 0x40 / 0xC0, a 16 x 64 animated strip.
    let mut rgba = Vec::new();
    for y in 0..64 {
        for x in 0..16 {
            let v = if (x + y) % 2 == 0 { 0x40 } else { 0xC0 };
            rgba.extend_from_slice(&[v, v, v, 255]);
        }
    }
    let d = Detail::build(16, 64, &rgba).unwrap();
    let a = d.sample(0.01, 0.01, 16.0).unwrap();
    let b = d.sample(1.0 / 16.0 + 0.01, 0.01, 16.0).unwrap();
    assert!(a[0] < 128 && b[0] > 128, "{a:?} {b:?}");
    // Coarser than the texture: the mip averages the checker out.
    let m = d.sample(0.3, 0.3, 2.5).unwrap();
    assert!((m[0] as i32 - 128).abs() <= 4, "{m:?}");
    assert!(d.sample(0.5, 0.5, 1.5).is_none());
    assert_eq!(detail::strength(1.0), 0);
    assert_eq!(detail::strength(detail::DETAIL_FULL), 256);
    let c = 0xFF80_8080;
    assert_eq!(detail::apply(c, [128; 3], 256), c, "ratio 1");
    assert_eq!(detail::apply(c, [64; 3], 0), c, "no strength");
    assert_eq!(detail::apply(c, [64; 3], 256) & 0xFF, 0x40);
    assert_eq!(detail::apply(0xF380_8080, [64; 3], 256) >> 24, 0xF3, "alpha (light) kept");
}

struct Textured<'a>(&'a Store, crate::engine::detail::Detail);
impl Lookup for Textured<'_> {
    fn region(&self, rx: i32, rz: i32) -> Option<&Region> {
        self.0.get(rx, rz)
    }
    fn detail(&self, _x: i32, _y: i32, _z: i32) -> Option<&crate::engine::detail::Detail> {
        Some(&self.1)
    }
}

#[test]
fn view_textures_only_when_zoomed_in() {
    let s = baked_store(2);
    let mut rgba = Vec::new();
    for y in 0..16 {
        for x in 0..16 {
            let v = if (x / 4 + y / 4) % 2 == 0 { 0x30 } else { 0xD0 };
            rgba.extend_from_slice(&[v, v, v, 255]);
        }
    }
    let lk = Textured(&s, crate::engine::detail::Detail::build(16, 16, &rgba).unwrap());
    let (w, h) = (64usize, 64usize);
    for (bpp, differs) in [(1.0 / 8.0, true), (1.0, false)] {
        let mut v = View::new([5.0, 5.0], bpp);
        v.fog = false;
        let mut plain = vec![0u8; w * h * 4];
        view::render(&mut plain, w, h, &v, &lk);
        v.style.textures = true;
        let mut tex = vec![0u8; w * h * 4];
        view::render(&mut tex, w, h, &v, &lk);
        assert_eq!(plain != tex, differs, "bpp {bpp}");
    }
}

#[test]
fn view_plain_style_is_nearest() {
    // Smooth off (the default) keeps the nearest texel: every pixel equals a baked colour.
    let s = baked_store(2);
    let lk = StoreLookup(&s);
    let (w, h) = (32usize, 32usize);
    let mut v = View::new([0.3, 0.6], 0.37);
    v.fog = false;
    v.style.relief = 0;
    let mut img = vec![0u8; w * h * 4];
    view::render(&mut img, w, h, &v, &lk);
    let r = s.get(0, 0).unwrap();
    let known: std::collections::HashSet<[u8; 4]> =
        r.colors.iter().map(|&c| crate::engine::color::to_rgba(c)).collect();
    let px = |x: usize, y: usize| -> [u8; 4] { img[(y * w + x) * 4..][..4].try_into().unwrap() };
    for y in 20..32 {
        for x in 20..32 {
            assert!(known.contains(&px(x, y)), "pixel {x},{y} is a baked colour");
        }
    }
}

#[test]
fn view_smooth_glides_under_sub_pixel_moves() {
    let s = baked_store(4);
    let lk = StoreLookup(&s);
    let (w, h) = (48usize, 48usize);
    let render = |cx: f64, smooth: bool| {
        let mut v = View::new([cx, 3.0], 0.25);
        v.fog = false;
        v.style.smooth = smooth;
        let mut img = vec![0u8; w * h * 4];
        view::render(&mut img, w, h, &v, &lk);
        img
    };
    // Steps of a tenth of a pixel: the largest jump of any pixel from one step to the next.
    let worst = |smooth: bool| {
        let mut worst = 0i32;
        let mut prev = render(0.0, smooth);
        for i in 1..=10 {
            let cur = render(i as f64 * 0.025, smooth);
            for (a, b) in prev.chunks_exact(4).zip(cur.chunks_exact(4)) {
                for c in 0..3 {
                    worst = worst.max((a[c] as i32 - b[c] as i32).abs());
                }
            }
            prev = cur;
        }
        worst
    };
    let (sharp, smooth) = (worst(false), worst(true));
    assert!(smooth < sharp, "smooth {smooth} vs nearest {sharp}");
}

#[test]
fn follow_glides_and_snaps() {
    use std::time::{Duration, Instant};
    let mut f = view::Follow::default();
    let t = Instant::now();
    assert_eq!(f.update([0.0, 0.0], t), [0.0, 0.0]);
    // A tick step of 0.2 blocks is reached over one period, not at once.
    let p = f.update([0.2, 0.0], t + Duration::from_millis(50));
    assert!(p[0] < 0.05, "{p:?}");
    let mid = f.at(t + Duration::from_millis(75));
    assert!(mid[0] > 0.05 && mid[0] < 0.2, "{mid:?}");
    let end = f.at(t + Duration::from_millis(200));
    assert!((end[0] - 0.2).abs() < 1e-9);
    // Monotonic while walking at a steady pace.
    let mut last = end[0];
    for i in 0..40 {
        let now = t + Duration::from_millis(200 + i * 10);
        let raw = 0.2 + ((i / 5) + 1) as f64 * 0.2;
        let p = if i % 5 == 0 { f.update([raw, 0.0], now) } else { f.at(now) };
        assert!(p[0] >= last - 1e-9, "step {i}: {} < {last}", p[0]);
        last = p[0];
    }
    // A teleport is taken at once.
    let p = f.update([500.0, -40.0], t + Duration::from_secs(1));
    assert_eq!(p, [500.0, -40.0]);
    f.reset();
    assert_eq!(f.update([1.0, 1.0], t + Duration::from_secs(2)), [1.0, 1.0]);
}

#[test]
fn settings_new_flags_roundtrip() {
    let s = Settings { smooth: false, textures: false, block_light: false, ..Settings::default() };
    assert_eq!(Settings::from_text(&s.to_text()), s);
    let d = Settings::default();
    assert!(d.smooth && d.textures && d.block_light);
    let st = d.style(256, false);
    assert!(st.smooth && st.textures && st.glow == 256);
    let st = s.style(256, false);
    assert!(!st.smooth && !st.textures && st.glow == 0);
}

// ---- 0.3.5: no zoom step blurs (user: "1px = 2 blocks" looked soft-focus) ----

/// Block-detailed ground (every block its own colour) in the four regions around the origin.
struct Busy(Vec<Region>);
impl Busy {
    fn new() -> Busy {
        let mut v = Vec::new();
        for (rx, rz) in [(-1, -1), (0, -1), (-1, 0), (0, 0)] {
            let mut r = Region::new(rx, rz);
            for z in 0..SIZE {
                for x in 0..SIZE {
                    let (wx, wz) = (rx * SIZE as i32 + x as i32, rz * SIZE as i32 + z as i32);
                    let hsh = (wx as u32).wrapping_mul(0x9E37_79B1) ^ (wz as u32).wrapping_mul(0x85EB_CA77);
                    let hsh = (hsh ^ hsh >> 15).wrapping_mul(0x2C1B_3C6D);
                    // Patches of 3 blocks with per-block noise on top: structure at several scales.
                    let p = ((wx.div_euclid(3) * 31 + wz.div_euclid(3) * 17) & 7) as u32 * 24;
                    let n = (hsh >> 24) & 63;
                    r.colors[z * SIZE + x] = 0xFF00_0000 | (p + n) << 16 | (200 - p / 2 + n / 2) << 8 | (40 + n);
                    r.heights[z * SIZE + x] = 64;
                }
            }
            r.explored = [!0; 16];
            r.generation += 1;
            r.ensure_mips();
            v.push(r);
        }
        Busy(v)
    }
    fn block(&self, x: i32, z: i32) -> u32 {
        let (rx, rz) = (x.div_euclid(SIZE as i32), z.div_euclid(SIZE as i32));
        let r = self.0.iter().find(|r| (r.rx, r.rz) == (rx, rz)).unwrap();
        r.colors[z.rem_euclid(SIZE as i32) as usize * SIZE + x.rem_euclid(SIZE as i32) as usize]
    }
}
impl Lookup for Busy {
    fn region(&self, rx: i32, rz: i32) -> Option<&Region> {
        self.0.iter().find(|r| (r.rx, r.rz) == (rx, rz))
    }
}

/// Sharpness of the map at every zoom step (`MapUi::zoom_at` multiplies by 1.25): the mean gradient of the render
/// against the mean gradient of the ideal picture (each pixel the exact area average of the blocks under it,
/// 8 x 8 supersampled), and the mean error against that ideal. No step may be softer than its neighbours by more
/// than a margin, nor much softer than ideal.
#[test]
fn zoom_steps_are_equally_sharp() {
    let lk = Busy::new();
    let (w, h) = (96usize, 96usize);
    let grad = |img: &[u8]| -> f64 {
        let mut s = 0u64;
        for y in 0..h - 1 {
            for x in 0..w - 1 {
                let i = (y * w + x) * 4;
                for c in 0..3 {
                    s += (img[i + c] as i32 - img[i + 4 + c] as i32).unsigned_abs() as u64;
                    s += (img[i + c] as i32 - img[i + w * 4 + c] as i32).unsigned_abs() as u64;
                }
            }
        }
        s as f64 / ((w - 1) * (h - 1) * 6) as f64
    };
    let mut rows = Vec::new();
    for k in 0..14 {
        let bpp = 0.32 * 1.25f64.powi(k);
        let mut ratios = Vec::new();
        let mut errs = Vec::new();
        // A few sub-pixel centres: the still map sits at any fraction of a block.
        for (cx, cz) in [(0.0, 0.0), (0.37, 0.81), (0.5, 0.5), (0.13, 0.66)] {
            let mut v = View::new([cx, cz], bpp);
            v.fog = false;
            v.style.smooth = true;
            v.style.relief = 0;
            let mut img = vec![0u8; w * h * 4];
            view::render(&mut img, w, h, &v, &lk);
            let mut ideal = vec![0u8; w * h * 4];
            for y in 0..h {
                for x in 0..w {
                    let mut acc = [0u32; 3];
                    for sy in 0..8 {
                        for sx in 0..8 {
                            let p = v.to_world(w, h, x as f64 + (sx as f64 + 0.5) / 8.0, y as f64 + (sy as f64 + 0.5) / 8.0);
                            let c = lk.block(p[0].floor() as i32, p[1].floor() as i32);
                            acc[0] += (c >> 16) & 255;
                            acc[1] += (c >> 8) & 255;
                            acc[2] += c & 255;
                        }
                    }
                    let i = (y * w + x) * 4;
                    for c in 0..3 {
                        ideal[i + c] = ((acc[c] + 32) / 64) as u8;
                    }
                    ideal[i + 3] = 255;
                }
            }
            ratios.push(grad(&img) / grad(&ideal));
            let e: u64 = img.iter().zip(&ideal).map(|(a, b)| (*a as i32 - *b as i32).unsigned_abs() as u64).sum();
            errs.push(e as f64 / (w * h * 3) as f64);
        }
        let r = ratios.iter().sum::<f64>() / ratios.len() as f64;
        let e = errs.iter().sum::<f64>() / errs.len() as f64;
        rows.push((bpp, r, e));
    }
    eprintln!("image blocks/px | sharpness (render/ideal gradient) | mean error vs ideal");
    for (b, r, e) in &rows {
        eprintln!("{b:7.3} | {r:5.3} | {e:5.2}");
    }
    for i in 0..rows.len() {
        let (b, r, _) = rows[i];
        let nb: Vec<f64> = [i.wrapping_sub(1), i + 1].iter().filter_map(|&j| rows.get(j)).map(|x| x.1).collect();
        let lo = nb.iter().cloned().fold(f64::MAX, f64::min);
        assert!(r >= lo - 0.12, "{b:.3} blocks/px is softer than its neighbours: {r:.3} vs {nb:?}");
        assert!(r >= 0.75, "{b:.3} blocks/px is soft: {r:.3} of ideal");
    }
}

/// A synthetic far terrain source on disk for the load tests: every region of a wide square has a file, some
/// fail to read a number of times first; the worker answers a few reads per frame, in order.
struct SlowDisk {
    fail: std::collections::HashMap<(i32, i32), u32>,
    queue: std::collections::VecDeque<(i32, i32)>,
    per_frame: usize,
}

/// One frame of the map as glue's `prepare` runs it (every frame, zoom, pan or still), then the worker's answers,
/// an eviction every `evict_every` frames.
fn load_frame(s: &mut Store, d: &mut SlowDisk, v: &View, w: usize, h: usize, now: std::time::Instant, evict: bool) -> Vec<(i32, i32)> {
    let mut vis = Vec::new();
    let shown = Store::plan(v, w, h, &mut vis);
    s.fit_visible(vis.len());
    s.shown = Some(shown);
    for &k in &vis {
        if s.want_load(k, now) {
            d.queue.push_back(k);
        }
        if s.regions.contains_key(&k) {
            s.touch(k);
        }
    }
    for _ in 0..d.per_frame {
        let Some(k) = d.queue.pop_front() else { break };
        match d.fail.get_mut(&k) {
            Some(n) if *n > 0 => {
                *n -= 1;
                s.load_failed(k, now);
            }
            _ => s.loaded(Region::new(k.0, k.1)),
        }
    }
    if evict {
        let _ = s.evict_regions(&vis);
    }
    vis
}

#[test]
fn lod_loading_follows_the_view_without_a_pan() {
    let t0 = std::time::Instant::now();
    let (w, h) = (1280usize, 720usize);
    let mut s = Store::new(48);
    let mut d = SlowDisk { fail: Default::default(), queue: Default::default(), per_frame: 4 };
    let mut v = View::new([100.0, 100.0], 1.0);
    let mut frame = 0u64;
    let mut run = |s: &mut Store, d: &mut SlowDisk, v: &View, frames: u64| -> (usize, usize, u64) {
        let mut first_full = None;
        let mut vis = Vec::new();
        for f in 0..frames {
            frame += 1;
            let now = t0 + std::time::Duration::from_millis(frame * 16);
            vis = load_frame(s, d, v, w, h, now, frame % 600 == 0 || f % 37 == 0);
            if first_full.is_none() && vis.iter().all(|k| s.regions.contains_key(k)) {
                first_full = Some(f);
            }
        }
        let loaded = vis.iter().filter(|k| s.regions.contains_key(k)).count();
        (vis.len(), loaded, first_full.map_or(u64::MAX, |f| f))
    };
    // Still at a close zoom, then zoomed out in steps with the view standing still: everything it shows loads.
    run(&mut s, &mut d, &v, 60);
    for bpp in [2.0, 4.0, 5.0, 6.0, 8.0] {
        v.bpp = bpp;
        let (n, loaded, at) = run(&mut s, &mut d, &v, 120);
        let mut all = Vec::new();
        view::visible_regions(&v, w, h, &mut all);
        println!(
            "zoom out to {bpp} blocks/px, still: {} regions on screen, {n} kept (cap {}), {loaded} loaded, full after {at} \
             frames, limit {}, evicted while on screen {}",
            all.len(),
            store::VISIBLE_CAP,
            s.limit,
            s.stats.evicted_visible
        );
        assert_eq!(loaded, n, "every kept visible region loads without a pan");
        if all.len() <= store::VISIBLE_CAP {
            assert_eq!(s.stats.evicted_visible, 0, "within the cap nothing on screen is evicted");
        }
    }
    let beyond_cap = s.stats.evicted_visible;
    // In and out a dozen times, evicting all the while: nothing on screen goes.
    for i in 0..12 {
        v.bpp = if i % 2 == 0 { 1.0 } else { 5.0 };
        let (n, loaded, _) = run(&mut s, &mut d, &v, 90);
        assert_eq!(loaded, n, "step {i}: {loaded}/{n} visible loaded");
        let mut vis = Vec::new();
        view::visible_regions(&v, w, h, &mut vis);
        assert!(vis.iter().all(|k| s.regions.contains_key(k)), "step {i}: a visible region is missing");
    }
    println!("stats after 12 zooms: {:?}, {} in memory", s.stats, s.regions.len());
    assert_eq!(s.stats.evicted_visible, beyond_cap, "evicted while visible");
    assert!(s.regions.len() <= s.limit.max(store::VISIBLE_CAP + 8));
}

#[test]
fn lod_loading_retries_a_failed_read() {
    let t0 = std::time::Instant::now();
    let mut s = Store::new(48);
    let mut d = SlowDisk { fail: [((0, 0), 2u32)].into_iter().collect(), queue: Default::default(), per_frame: 8 };
    let v = View::new([100.0, 100.0], 1.0);
    let mut got = None;
    for f in 0..1200u64 {
        let now = t0 + std::time::Duration::from_millis(f * 16);
        load_frame(&mut s, &mut d, &v, 256, 256, now, f % 60 == 0);
        if s.regions.contains_key(&(0, 0)) {
            got = Some(f);
            break;
        }
    }
    println!("failed twice, loaded after {:?} frames (16 ms), stats {:?}", got, s.stats);
    let f = got.expect("a region whose read failed loads on a later try");
    // 3 s, then 6 s more.
    assert!(f * 16 >= 9000 && f * 16 < 9500, "{f}");
    assert_eq!(s.stats.failed, 2);
    assert!(s.pending_load.is_empty());
    // A region that never reads gives up after a few tries and stops being asked.
    let mut s = Store::new(48);
    let mut d = SlowDisk { fail: [((0, 0), 1000u32)].into_iter().collect(), queue: Default::default(), per_frame: 8 };
    for f in 0..6000u64 {
        let now = t0 + std::time::Duration::from_millis(f * 16);
        load_frame(&mut s, &mut d, &v, 256, 256, now, false);
    }
    println!("never readable: {} tries in 96 s", s.stats.failed);
    assert_eq!(s.stats.failed as u8, store::RETRY_TRIES + 1);
}

/// Icon jitter: a player and a mob walk at constant speed; the minimap glides with the player (view::Follow on
/// 20 Hz ticks, centre quantised to 1/8 image px) and the mob icon is drawn over it. The icon, measured from a
/// landmark painted in the image, must glide: small residual from a straight line, no backward or double steps.
#[test]
fn icon_glides_with_the_map() {
    use crate::engine::view::{Follow, View};
    use std::time::{Duration, Instant};
    let snap_half = |v: f64| (v * 2.0).round() / 2.0;
    // Minimap: image is 2x GUI px; 0.5 blocks per image px (zoomed in, where steps show most).
    let (bpp, w, h) = (0.5f64, 256usize, 256usize);
    let landmark = [12.3f64, -7.9f64];
    let base = Instant::now();
    let mut table = Vec::new();
    for fps in [60.0f64, 144.0] {
        for (name, ve) in [("mob walks", [0.0f64, 4.3]), ("mob follows", [5.6f64, 0.0])] {
            for new in [false, true] {
                let vp = [5.6f64, 0.0];
                let tick = |t: f64| (t / 0.05).floor() * 0.05;
                let (mut fp, mut fe) = (Follow::default(), Follow::default());
                let mut radar_at = -1.0f64;
                let mut radar_pos = [0.0f64; 2];
                let mut rel = Vec::new();
                // Warm up 0.5 s so the followers have learned the tick period, then measure 60 frames.
                let frames = (0.5 * fps) as usize + 60;
                for f in 0..frames {
                    let t = f as f64 / fps + 0.0123;
                    let now = base + Duration::from_secs_f64(t);
                    let tt = tick(t);
                    let p = [vp[0] * tt, vp[1] * tt];
                    let e = [3.0 + ve[0] * tt, 2.0 + ve[1] * tt];
                    let c = fp.update(p, now);
                    let q = bpp / 8.0;
                    let c = [(c[0] / q).round() * q, (c[1] / q).round() * q];
                    let v = View { center: c, bpp, rotation: 0.0, round: false, fog: false, style: Default::default() };
                    let lm = v.to_image(w, h, landmark[0], landmark[1]);
                    let ic = if new {
                        let pe = fe.update(e, now);
                        let i = v.to_image(w, h, pe[0], pe[1]);
                        [snap_half(i[0] / 2.0), snap_half(i[1] / 2.0)]
                    } else {
                        if t - radar_at >= 0.25 {
                            radar_at = t;
                            radar_pos = e;
                        }
                        let i = v.to_image(w, h, radar_pos[0], radar_pos[1]);
                        [(i[0] / 2.0).round(), (i[1] / 2.0).round()]
                    };
                    if f >= frames - 60 {
                        rel.push([ic[0] - lm[0] / 2.0, ic[1] - lm[1] / 2.0]);
                    }
                }
                // Residual from the least-squares line, per axis; steps = largest single-frame move vs the mean.
                let mut worst = 0.0f64;
                let mut back = 0;
                let mut jump = 0.0f64;
                let n = rel.len() as f64;
                for a in 0..2 {
                    let ys: Vec<f64> = rel.iter().map(|r| r[a]).collect();
                    let mx = (n - 1.0) / 2.0;
                    let my = ys.iter().sum::<f64>() / n;
                    let sxy: f64 = ys.iter().enumerate().map(|(i, y)| (i as f64 - mx) * (y - my)).sum();
                    let sxx: f64 = (0..ys.len()).map(|i| (i as f64 - mx).powi(2)).sum();
                    let k = sxy / sxx;
                    for (i, y) in ys.iter().enumerate() {
                        worst = worst.max((y - (my + k * (i as f64 - mx))).abs());
                    }
                    for d in ys.windows(2).map(|p| p[1] - p[0]) {
                        // An axis the icon does not travel along (under half a px in 60 frames) has no direction.
                        if d * k.signum() < -1e-9 && k.abs() * n > 0.5 {
                            back += 1;
                        }
                        jump = jump.max((d - k).abs());
                    }
                }
                println!(
                    "icon {} fps {:>3} {:<11}: max off line {:.2} px, backward steps {}, worst frame step off mean {:.2} px",
                    if new { "new" } else { "old" }, fps, name, worst, back, jump
                );
                table.push((new, worst, back));
            }
        }
    }
    for (new, worst, back) in table {
        if new {
            assert!(worst <= 0.5 + 1e-9, "icon strays {worst:.2} px from a straight glide");
            assert_eq!(back, 0, "icon steps backwards");
        }
    }
}

/// The info block (0.3.5): one panel as wide as the widest line, centred or flush with the chosen edge, every row
/// inside the panel and the screen, words wrapped not clipped. en/ru, 4 corners, 3 sizes, 1-6 lines.
#[test]
fn info_block_fits_and_aligns() {
    use crate::engine::infoblock::{self, Line};
    // Roughly the game font: 6 px a letter or digit, 4 a space, 2 for '.' and ':' (Cyrillic as wide as Latin).
    let width = |s: &str| -> f32 { s.chars().map(|c| match c { ' ' => 4.0, '.' | ':' | ',' | '·' => 2.0, _ => 6.0 }).sum::<f32>() - if s.is_empty() { 0.0 } else { 1.0 } };
    let sets: [(&str, [&str; 6]); 2] = [
        ("en", ["-1234 64 5678", "Old Growth Spruce Taiga", "Day 2480 · 14:21 · Night", "Light 7 (sky 15)", "Facing North-west (135)", "The Overworld"]),
        ("ru", ["-1234 64 5678", "Старовозрастная еловая тайга", "День 2480 · 14:21 · Ночь", "Свет 7 (небо 15)", "Взгляд Северо-запад (135)", "Верхний мир"]),
    ];
    let screen_w = 427.0f32;
    println!("lang size corner lines | panel w  rows | widest line");
    for (lang, texts) in sets {
        for frame_w in [68.0f32, 100.0, 132.0] {
            for (corner, right, below) in [("TL", false, true), ("TR", true, true), ("BL", false, true), ("BR", true, true)] {
                for n in 1..=6usize {
                    let mut lines: Vec<Line> = texts[..n].iter().map(|t| Line { text: t.to_string(), icon: false, color: 0xFFFFFFFF }).collect();
                    lines.push(Line { text: infoblock::hostile_line(2, 21.4, if lang == "ru" { "м" } else { "m" }, None), icon: true, color: 0xFFFF5555 });
                    let fx = if right { screen_w - 2.0 - frame_w } else { 2.0 };
                    let fy = if corner.starts_with('T') { 2.0 } else { 200.0 };
                    let frame = [fx, fy, frame_w, frame_w];
                  for align in [infoblock::Align::Center, infoblock::Align::Left, infoblock::Align::Right] {
                    let b = infoblock::layout(&lines, frame, align, below, screen_w, &width);
                    let [bx, by, bw, bh] = b.rect;
                    assert!(bx >= 0.0 && bx + bw <= screen_w, "{lang} {corner} {frame_w}: panel off screen {:?}", b.rect);
                    // Flush with the chosen edge, or centred, unless the screen edge pushed it in.
                    let pushed = bx <= 1.0 + 0.01 || bx + bw >= screen_w - 1.0 - 0.01;
                    match align {
                        infoblock::Align::Right if !pushed => assert!((bx + bw - (fx + frame_w)).abs() < 0.01, "{lang} {corner}: not flush right"),
                        infoblock::Align::Left if !pushed => assert!((bx - fx).abs() < 0.01, "{lang} {corner}: not flush left"),
                        infoblock::Align::Center if !pushed => assert!((bx + bw / 2.0 - (fx + frame_w / 2.0)).abs() <= 0.5, "{lang} {corner}: not centred"),
                        _ => {}
                    }
                    assert_eq!(bh, b.rows.len() as f32 * infoblock::ROW_H);
                    assert!((by - (fy + frame_w + 1.0)).abs() < 0.01);
                    let mut widest = 0.0f32;
                    for r in &b.rows {
                        let lead = if r.icon { infoblock::ICON_W } else { 0.0 };
                        assert!(r.x - lead >= bx + infoblock::PAD - 0.01 && r.x + r.w <= bx + bw - infoblock::PAD + 0.01, "{lang} {corner}: row '{}' out of the panel", r.text);
                        assert!(r.y >= by && r.y + 9.0 <= by + bh + 0.01);
                        widest = widest.max(r.w + lead);
                    }
                    // The panel is exactly as wide as its widest row.
                    assert!((bw - (widest + 2.0 * infoblock::PAD)).abs() < 0.01, "{lang} {corner}: panel {bw} vs widest {widest}");
                    // No word lost: the rows say the same as the lines, unless a single word was shortened.
                    let said: Vec<String> = b.rows.iter().flat_map(|r| r.text.split(' ').map(str::to_string).collect::<Vec<_>>()).collect();
                    let want: Vec<String> = lines.iter().flat_map(|l| l.text.split(' ').map(str::to_string).collect::<Vec<_>>()).collect();
                    if !said.iter().any(|w| w.ends_with("..")) {
                        assert_eq!(said, want, "{lang} {corner}: words lost");
                    }
                    if align == infoblock::Align::Center {
                        // Centred rows: equal room on both sides (within the whole pixel it is snapped to).
                        for r in &b.rows {
                            let lead = if r.icon { infoblock::ICON_W } else { 0.0 };
                            let (l, rr) = (r.x - lead - bx, bx + bw - (r.x + r.w));
                            assert!((l - rr).abs() <= 1.01, "{lang} {corner}: row '{}' not centred ({l} vs {rr})", r.text);
                        }
                    }
                    if align == infoblock::Align::Center && (n == 6 && corner == "TR" || n == 6 && corner == "TL" && frame_w == 68.0) {
                        println!("{lang}   {frame_w:>4} {corner}     {n}+1 | {bw:>6.0} {:>5} | {widest:.0}", b.rows.len());
                    }
                  }
                }
            }
        }
    }
    // The time line: no doubled "День"; the period word only at morning, evening or night.
    assert_eq!(infoblock::time_line("День", 2480, 8350, None), "День 2480 · 14:21");
    assert_eq!(infoblock::time_line("Day", 3, 15000, Some("Night")), "Day 3 · 21:00 · Night");
    assert_eq!(infoblock::hostile_line(2, 21.4, "м", None), "2 · 21 м");
    assert_eq!(infoblock::hostile_line(1, 5.0, "m", Some("Hostile")), "Hostile: 1 · 5 m");
    assert_eq!(infoblock::shorten("Abcdefghijklmnop", 30.0, &width), "Abcd..");
}

// ---- 0.3.5: block ids saved with the regions (textures where no chunk is loaded) ----

/// The block stamp the tests run under (one value for the whole process: the stamp is a global).
const TEST_STAMP: u32 = 8;

#[test]
fn region_blocks_roundtrip_and_old_data_loads() {
    use crate::engine::region;
    use crate::fake::{GRASS, SAND, STONE};
    region::set_block_stamp(TEST_STAMP);
    let w = World { r: 2, cave: false };
    let t = table();
    let mut r = Region::new(0, 0);
    for cz in 0..2 {
        for cx in 0..2 {
            let c = bake_chunk(&w, &Plains, &t, cx, cz, Layer::Surface).unwrap();
            assert!(c.block.iter().any(|&b| b != 0), "baker fills the column blocks");
            for i in 0..256 {
                let (x, z) = (cx * 16 + (i % 16) as i32, cz * 16 + (i / 16) as i32);
                if c.height[i] != i16::MIN && crate::engine::color::water_depth(c.color[i]) == 0 {
                    use crate::engine::baker::ChunkSource;
                    assert_eq!(c.block[i], w.block([x, c.height[i] as i32, z]), "shown block at {x},{z}");
                }
            }
            r.put_chunk(&c);
        }
    }
    // Hand-placed ids too, so the palette has several entries.
    let mut c = crate::engine::baker::BakedChunk { cx: 5, cz: 7, color: [0xFF30_6030; 256], height: [64; 256], block: [0; 256] };
    for (i, b) in c.block.iter_mut().enumerate() {
        *b = [STONE, GRASS, SAND, 0][i % 4];
    }
    r.put_chunk(&c);
    let bytes = r.encode();
    assert_eq!(bytes[4], 3, "written as v3 when it has block ids");
    let d = Region::decode(&bytes).unwrap();
    assert_eq!(d.colors, r.colors);
    assert_eq!(d.heights, r.heights);
    assert_eq!(d.block_stamp, TEST_STAMP);
    for z in 0..SIZE as i32 {
        for x in 0..SIZE as i32 {
            assert_eq!(d.block_at(x, z), r.block_at(x, z), "block at {x},{z}");
        }
    }
    assert_eq!(d.block_at(5 * 16 + 1, 7 * 16), Some(GRASS));
    assert_eq!(d.block_at(5 * 16 + 3, 7 * 16), None, "unknown stays unknown");
    assert_eq!(d.encode(), bytes, "encode is stable");
    // Another registry: the ids are not read back (they would name other blocks).
    let mut other = d.clone_for_test();
    other.block_stamp = TEST_STAMP + 1;
    assert_eq!(other.block_at(5 * 16 + 1, 7 * 16), None);
    // Old data: a region without block ids is still v2 and a v2 file (0.3.4 and older) loads, without ids.
    let mut old = Region::new(1, 1);
    let mut c2 = c.clone();
    c2.block = [0; 256];
    c2.cx = 32 + 1;
    c2.cz = 32 + 1;
    old.put_chunk(&c2);
    let v2 = old.encode();
    assert_eq!(v2[4], 2, "no ids: still v2");
    let dv2 = Region::decode(&v2).unwrap();
    assert!(dv2.blocks.is_none());
    assert_eq!(dv2.colors, old.colors);
    // A v2 region learns ids from a later bake and then writes v3; the v3 file decodes with both.
    let mut grow = dv2;
    let mut c3 = c.clone();
    c3.cx = 32 + 2;
    c3.cz = 32 + 1;
    grow.put_chunk(&c3);
    let v3 = grow.encode();
    assert_eq!(v3[4], 3);
    let g = Region::decode(&v3).unwrap();
    assert_eq!(g.block_at(34 * 16 + 1, 33 * 16), Some(GRASS));
    assert_eq!(g.block_at(33 * 16 + 1, 33 * 16), None, "old chunk: no id");
    assert!(g.chunk_explored(1, 1) && g.chunk_explored(2, 1));
    // Corrupt block runs are refused, not read past the end.
    let mut bad = v3.clone();
    let n = bad.len();
    bad[n - 6] ^= 0x55;
    assert!(Region::decode(&bad).is_err());
}

#[test]
fn far_columns_textured_without_a_snapshot() {
    use crate::engine::detail::Detail;
    use crate::engine::region;
    use crate::fake::{GRASS, STONE};
    region::set_block_stamp(TEST_STAMP);
    let t = table();
    let tints = TintTable { tints: vec![(0x91BD59, 0x77AB2F, 0x3F76E4)] };
    // Far columns only (no chunk loaded, nothing explored): grass and stone halves.
    let cols: Vec<Option<Surface>> = (0..256)
        .map(|i| Some(Surface { height: 65, state: if i % 16 < 8 { GRASS } else { STONE }, biome: 0, water: None }))
        .collect();
    let c = bake_surfaces(0, 0, &cols, &t, &tints).unwrap();
    assert_eq!(c.block[0], GRASS);
    assert_eq!(c.block[15], STONE);
    let mut far = Region::new(0, 0);
    far.put_chunk(&c);
    // Survives the disk too (the far store saves its regions like the explored ones).
    let far = Region::decode(&far.encode()).unwrap();
    assert_eq!(region::saved_block(None, Some(&far), 3, 3), Some(GRASS));
    assert_eq!(region::saved_block(None, Some(&far), 12, 3), Some(STONE));
    // Explored ground wins over far even without an id (its colour is not the far one's).
    let mut explored = Region::new(0, 0);
    let mut e = c.clone();
    e.block = [0; 256];
    explored.put_chunk(&e);
    // ... unless the far column is the same surface (0.3.10): then its block textures the explored pixel.
    assert_eq!(region::saved_block(Some(&explored), Some(&far), 3, 3), Some(GRASS));
    let mut other = Region::new(0, 0);
    e.height = [e.height[0] + 9; 256];
    other.put_chunk(&e);
    assert_eq!(region::saved_block(Some(&other), Some(&far), 3, 3), None);
    assert_eq!(region::saved_block(Some(&Region::new(0, 0)), Some(&far), 3, 3), Some(GRASS), "unexplored: far");
    // The lookup the glue uses (`Look::detail` without a snapshot) textures the far pixels.
    let mut rgba = Vec::new();
    for y in 0..16 {
        for x in 0..16 {
            let v = if (x / 4 + y / 4) % 2 == 0 { 0x30 } else { 0xD0 };
            rgba.extend_from_slice(&[v, v, v, 255]);
        }
    }
    struct FarTex(Region, Detail);
    impl Lookup for FarTex {
        fn region(&self, _: i32, _: i32) -> Option<&Region> {
            None
        }
        fn far_region(&self, rx: i32, rz: i32) -> Option<&Region> {
            ((rx, rz) == (0, 0)).then_some(&self.0)
        }
        fn detail(&self, x: i32, _y: i32, z: i32) -> Option<&Detail> {
            (region::saved_block(None, self.far_region(x >> 9, z >> 9), x, z)? == GRASS).then_some(&self.1)
        }
    }
    let lk = FarTex(far, Detail::build(16, 16, &rgba).unwrap());
    let (w, h) = (64usize, 64usize);
    let mut v = View::new([8.0, 8.0], 1.0 / 8.0);
    v.fog = false;
    let mut plain = vec![0u8; w * h * 4];
    view::render(&mut plain, w, h, &v, &lk);
    v.style.textures = true;
    let mut tex = vec![0u8; w * h * 4];
    view::render(&mut tex, w, h, &v, &lk);
    // World x 8 is the image centre: the left half (grass, x < 8) is textured, the right half (stone) is not.
    let diff = |x0: usize, x1: usize| {
        (0..h).flat_map(|y| (x0..x1).map(move |x| (y * w + x) * 4)).filter(|&o| plain[o..o + 3] != tex[o..o + 3]).count()
    };
    let (left, right) = (diff(0, w / 2 - 2), diff(w / 2 + 2, w));
    eprintln!("far textures: {left} grass px changed, {right} stone px changed");
    assert!(left > (w / 2 - 2) * h / 3, "grass far columns textured: {left}");
    assert_eq!(right, 0, "stone has no pattern here");
}

/// Disk and bake cost of the block ids: the real Horizon LOD cache (real block states per column) when present,
/// else a synthetic mix; prints v2 vs v3 sizes and the encode time.
#[test]
fn region_blocks_disk_cost() {
    use crate::engine::baker::BakedChunk;
    use crate::engine::region;
    region::set_block_stamp(TEST_STAMP);
    let mut regions: Vec<Region> = Vec::new();
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let saves = home.map(|h| h.join(".local/share/horizon/saves"));
    let (mut buf, mut raw) = (Vec::new(), Vec::new());
    let mut out = [None; 256];
    for w in saves.and_then(|s| std::fs::read_dir(s).ok()).into_iter().flatten().flatten() {
        let dir = hlod::dir_of(&w.path().join("dimensions/minecraft/overworld"));
        let Ok(list) = std::fs::read_dir(&dir) else { continue };
        for f in list.flatten() {
            if regions.len() >= 6 {
                break;
            }
            let Ok(file) = std::fs::File::open(f.path()) else { continue };
            let Some(mut hr) = HlodRegion::open(std::io::BufReader::new(file)) else { continue };
            let keys: Vec<_> = hr.chunks().collect();
            if keys.len() < 900 {
                continue;
            }
            let (rx, rz) = Region::of_chunk(keys[0].0, keys[0].1);
            let mut reg = Region::new(rx, rz);
            for (cx, cz) in keys {
                if Region::of_chunk(cx, cz) != (rx, rz) || !hr.read(cx, cz, inflate, &mut buf, &mut raw, &mut out) {
                    continue;
                }
                let mut c = BakedChunk { cx, cz, color: [0; 256], height: [i16::MIN; 256], block: [0; 256] };
                for (i, s) in out.iter().enumerate() {
                    let Some(s) = s else { continue };
                    let st = s.water.map_or(s.state, |w| w.1);
                    // A colour per block with the usual per-block jitter, as the baker gives.
                    let (x, z) = (cx * 16 + (i % 16) as i32, cz * 16 + (i / 16) as i32);
                    let base = 0xFF00_0000 | (st.wrapping_mul(2_654_435_761) & 0x00FF_FFFF);
                    c.color[i] = color::shade(base, color::jitter(x, z, 10));
                    c.height[i] = (s.height - 1) as i16;
                    c.block[i] = if s.water.is_some() { 0 } else { s.state };
                }
                reg.put_chunk(&c);
            }
            regions.push(reg);
        }
    }
    let real = !regions.is_empty();
    if !real {
        let w = World { r: 16, cave: false };
        let t = table();
        let mut reg = Region::new(-1, -1);
        for cz in -16..0 {
            for cx in -16..0 {
                reg.put_chunk(&bake_chunk(&w, &Plains, &t, cx, cz, Layer::Surface).unwrap());
            }
        }
        regions.push(reg);
    }
    let (mut v2, mut v3, mut pal, mut t_enc) = (0usize, 0usize, 0usize, 0f64);
    for r in &regions {
        let mut plain = Region::decode(&r.encode()).unwrap();
        plain.blocks = None;
        v2 += plain.encode().len();
        let t0 = std::time::Instant::now();
        let b = r.encode();
        t_enc += t0.elapsed().as_secs_f64();
        v3 += b.len();
        pal += r.block_pal.len() - 1;
        let d = Region::decode(&b).unwrap();
        let (x0, z0) = (r.rx * SIZE as i32, r.rz * SIZE as i32);
        for i in (0..SIZE * SIZE).step_by(7) {
            let (x, z) = (x0 + (i % SIZE) as i32, z0 + (i / SIZE) as i32);
            assert_eq!(d.block_at(x, z), r.block_at(x, z));
        }
    }
    let n = regions.len();
    eprintln!(
        "block ids ({}): {n} regions, v2 {} KB/region, v3 {} KB/region (+{:.1}%), palette {} ids/region, encode {:.1} ms/region",
        if real { "real LOD cache" } else { "synthetic" },
        v2 / n / 1024,
        v3 / n / 1024,
        (v3 as f64 / v2 as f64 - 1.0) * 100.0,
        pal / n,
        t_enc * 1000.0 / n as f64
    );
    assert!(v3 < v2 * 2, "block ids cost less than the rest");
}

#[test]
fn view_render_same_on_any_threads() {
    // Row bands on several threads (0.3.7): bit for bit the single-threaded image, at every mip level, turned,
    // round, smooth or not, with textures and grid, and with the band count not dividing the height.
    let s = baked_store(6);
    let mut rgba = Vec::new();
    for i in 0..256u32 {
        let v = (i * 37 % 200) as u8 + 30;
        rgba.extend_from_slice(&[v, v, v, 255]);
    }
    let lk = Textured(&s, crate::engine::detail::Detail::build(16, 16, &rgba).unwrap());
    let (w, h) = (301usize, 203usize);
    for (bpp, rot, round, smooth) in [(0.2, 0.0, false, true), (1.0, 0.4, true, true), (3.1, 0.0, false, false), (9.0, 1.3, false, true), (60.0, 0.0, false, true), (17.0, 2.0, true, false)] {
        let mut v = View::new([13.7, -21.2], bpp);
        v.rotation = rot;
        v.round = round;
        v.style.smooth = smooth;
        v.style.textures = true;
        v.style.grid = 3;
        v.style.mark_unknown = bpp > 5.0;
        let mut one = vec![0u8; w * h * 4];
        let mut many = vec![1u8; w * h * 4];
        view::set_render_threads(1);
        view::render(&mut one, w, h, &v, &lk);
        for n in [2, 3, 7] {
            view::set_render_threads(n);
            view::render(&mut many, w, h, &v, &lk);
            assert!(one == many, "bpp {bpp} rot {rot} threads {n}");
        }
        view::set_render_threads(0);
    }
}

/// Mips rebuilt chunk by chunk after `put_chunk` / `merge_missing` equal a full rebuild of the same pixels,
/// on every level, including re-bakes that change only the heights and generation bumps made from outside.
#[test]
fn incremental_mips_same_as_full_rebuild() {
    use crate::engine::baker::BakedChunk;
    use crate::engine::region::MIP_LEVELS;
    let mut seed = 0x1234_5678u32;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let check = |r: &mut Region, what: &str| {
        r.ensure_mips();
        let mut full = r.copy_pixels();
        full.ensure_mips();
        for l in 1..MIP_LEVELS {
            assert!(r.level(l) == full.level(l), "{what}: colour mip {l}");
            assert!(r.height_level(l) == full.height_level(l), "{what}: height mip {l}");
        }
    };
    let mut r = Region::new(2, -3);
    check(&mut r, "new");
    let mut last: Option<BakedChunk> = None;
    for step in 0..60 {
        if step % 9 == 4 {
            // A chunk of the last step (its mips are built) re-baked with the same colours and new heights: no
            // generation bump, but the rebuild after this step's writes must see them.
            if let Some(c) = last.as_mut() {
                for h in c.height.iter_mut() {
                    *h = h.wrapping_add(3).max(-60);
                }
                assert!(!r.put_chunk(c));
            }
        }
        let k = 1 + rnd() % 5;
        for _ in 0..k {
            let (lx, lz) = ((rnd() % 32) as i32, (rnd() % 32) as i32);
            let mut c = BakedChunk { cx: 64 + lx, cz: -96 + lz, color: [0; 256], height: [0; 256], block: [0; 256] };
            for i in 0..256 {
                let v = rnd();
                c.color[i] = if v % 7 == 0 { crate::engine::color::UNKNOWN } else { 0xFF00_0000 | (v >> 8) };
                c.height[i] = if v % 11 == 0 { i16::MIN } else { (v % 300) as i16 - 60 };
            }
            r.put_chunk(&c);
            last = Some(c);
        }
        if step % 13 == 7 {
            // A loaded region filling unexplored chunks.
            let mut o = Region::new(2, -3);
            for i in 0..SIZE * SIZE {
                o.colors[i] = 0xFF00_0000 | rnd() >> 8;
                o.heights[i] = (rnd() % 200) as i16;
            }
            o.explored = [rnd() as u64 | (rnd() as u64) << 32; 16];
            r.merge_missing(&o);
        }
        if step % 17 == 11 {
            // A direct write with its own bump, mixed with a chunk write: must fall back to a full rebuild.
            r.colors[rnd() as usize % (SIZE * SIZE)] = 0xFF12_3456;
            r.generation = r.generation.wrapping_add(1);
        }
        check(&mut r, &format!("step {step}"));
    }
}

/// The optimisations of 0.3.9 (empty-span fill, region table, fast floor, threaded refine) leave every frame of
/// the golden scenes as 0.3.8 drew it (the hash was taken from the 0.3.8 code).
#[test]
#[cfg_attr(debug_assertions, ignore = "slow without optimisation")]
fn golden_frames_unchanged() {
    // 0.3.10 moved the hash on purpose: the cloud frames with far terrain in them changed (the fog keeps off it,
    // `Clouds::kfar`); the map frames and the other cloud frames hash as in 0.3.8 (e528fb44 before).
    assert_eq!(crate::golden::run(|_| ()), 0x294f_3c98);
}

struct Bounded<'a>(&'a Store, &'a Store);
impl Lookup for Bounded<'_> {
    fn region(&self, rx: i32, rz: i32) -> Option<&Region> {
        self.0.get(rx, rz)
    }
    fn far_region(&self, rx: i32, rz: i32) -> Option<&Region> {
        self.1.get(rx, rz)
    }
    fn has_coarse_far(&self) -> bool {
        false
    }
    fn bounds(&self) -> Option<[i32; 4]> {
        match (self.0.bounds(), self.1.bounds()) {
            (Some(a), Some(b)) => Some([a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]),
            (a, b) => a.or(b),
        }
    }
}
struct Unbounded<'a>(&'a Store, &'a Store);
impl Lookup for Unbounded<'_> {
    fn region(&self, rx: i32, rz: i32) -> Option<&Region> {
        self.0.get(rx, rz)
    }
    fn far_region(&self, rx: i32, rz: i32) -> Option<&Region> {
        self.1.get(rx, rz)
    }
}

#[test]
fn render_with_bounds_same_as_without() {
    // The glue's lookup tells the render where its regions end and that it has no coarse far terrain (0.3.9):
    // the image is the one a lookup without those hints gives, zoomed in or all the way out, with empty stores too.
    let live = baked_store(3);
    let mut far = Store::new(64);
    let w = World { r: 40, cave: false };
    let t = table();
    for (cx, cz) in [(-40, 3), (-39, 3), (20, -30), (21, 35), (0, 0)] {
        far.put(&bake_chunk(&w, &Plains, &t, cx, cz, Layer::Surface).unwrap());
    }
    let empty = Store::new(4);
    let mut live = live;
    live.regions.values_mut().for_each(|r| r.ensure_mips());
    far.regions.values_mut().for_each(|r| r.ensure_mips());
    let (iw, ih) = (240usize, 135usize);
    for (a, b) in [(&live, &far), (&empty, &far), (&empty, &empty)] {
        for bpp in [0.5, 2.0, 7.0, 16.0, 40.0, 64.0] {
            for (mark, smooth, fog, round, rot) in [(true, true, true, false, 0.0), (false, false, true, false, 0.0), (false, true, false, true, 0.0), (true, true, true, false, 0.7)] {
                let mut v = View::new([37.5, -90.25], bpp);
                (v.style.mark_unknown, v.style.smooth, v.fog, v.round, v.rotation) = (mark, smooth, fog, round, rot);
                let mut x = vec![3u8; iw * ih * 4];
                let mut y = vec![9u8; iw * ih * 4];
                view::render(&mut x, iw, ih, &v, &Bounded(a, b));
                view::render(&mut y, iw, ih, &v, &Unbounded(a, b));
                assert!(x == y, "bpp {bpp} mark {mark} smooth {smooth} fog {fog} round {round} rot {rot}");
            }
        }
    }
}

#[test]
fn plan_picks_the_nearest_regions_without_listing_them() {
    // `Store::plan` (0.3.9) against listing the whole rectangle and sorting it: the same distances, nearest
    // first, at every zoom, for wide, tall and turned views, and the same rectangle.
    for (w, h) in [(1024usize, 576usize), (1024, 40), (30, 1024), (256, 256)] {
        for bpp in [0.25, 1.0, 4.0, 13.0, 32.0, 64.0] {
            for (center, rot) in [([0.0, 0.0], 0.0), ([-70000.3, 1234.5], 0.0), ([511.9, -512.1], 0.8)] {
                let mut v = View::new(center, bpp);
                v.rotation = rot;
                let mut all = Vec::new();
                view::visible_regions(&v, w, h, &mut all);
                let n_all = all.len();
                let c = ((center[0].floor() as i32) >> 9, (center[1].floor() as i32) >> 9);
                Store::nearest_visible(&mut all, c);
                let mut got = Vec::new();
                let rect = Store::plan(&v, w, h, &mut got);
                assert_eq!(rect, view::visible_rect(&v, w, h));
                let d = |k: &(i32, i32)| ((k.0 - c.0) as i64).pow(2) + ((k.1 - c.1) as i64).pow(2);
                assert_eq!(got.iter().map(d).collect::<Vec<_>>(), all.iter().map(d).collect::<Vec<_>>(), "{w}x{h} bpp {bpp} {center:?}");
                assert!(got.iter().all(|k| k.0 >= rect[0] && k.0 <= rect[2] && k.1 >= rect[1] && k.1 <= rect[3]));
                let mut s = Store::new(4);
                s.shown = Some(rect);
                assert_eq!(s.shown_count(), n_all as u64);
            }
        }
    }
}

#[test]
fn region_mips_built_before_the_store_takes_it() {
    // The worker builds a read region's mips (0.3.9): the store keeps them as they are, and a merge into chunks
    // baked meanwhile still ends with the mips of the merged pixels.
    use crate::engine::region::MIP_LEVELS;
    let src = baked_store(2);
    let mut disk = Region::decode(&src.get(0, 0).unwrap().encode()).unwrap();
    disk.ensure_mips();
    let mut s = Store::new(8);
    s.loaded(disk);
    let mut whole = Region::decode(&src.get(0, 0).unwrap().encode()).unwrap();
    whole.ensure_mips();
    for l in 0..MIP_LEVELS {
        assert!(s.get(0, 0).unwrap().level(l) == whole.level(l) && s.get(0, 0).unwrap().height_level(l) == whole.height_level(l));
    }
    // A chunk baked first, then the file arrives with its mips built.
    let mut s = Store::new(8);
    let w = World { r: 2, cave: true };
    s.put(&bake_chunk(&w, &Plains, &table(), 0, 0, Layer::Surface).unwrap());
    let mut disk = Region::decode(&src.get(0, 0).unwrap().encode()).unwrap();
    disk.ensure_mips();
    s.loaded(disk);
    let r = s.regions.get_mut(&(0, 0)).unwrap();
    r.ensure_mips();
    let mut again = r.clone_for_test();
    again.ensure_mips();
    for l in 0..MIP_LEVELS {
        assert!(r.level(l) == again.level(l) && r.height_level(l) == again.height_level(l), "level {l}");
    }
}

#[test]
fn settings_groups_fold_and_waypoint_menu() {
    // 0.3.10: a click on a group header folds the group; a right click on a waypoint opens its own menu.
    let en = Tr::new(Lang::En);
    let s = Settings::default();
    let labels = s.buttons_in(&en);
    let (mut open, mut folded) = (Vec::new(), Vec::new());
    settings::rows(0, "", &labels, &en, &[], &mut open);
    let Some(&settings::Row::Header(h)) = open.first() else { panic!("a category starts with a header") };
    settings::rows(0, "", &labels, &en, &[h], &mut folded);
    assert_eq!(folded.len(), open.len() - settings::group_len(0, h));
    assert_eq!(folded[0], settings::Row::Header(h));
    assert!(settings::group_len(0, h) > 0);

    let mut ui = MapUi::new();
    ui.screen = Rect::new(0.0, 0.0, 480.0, 270.0);
    ui.open([0.0, 64.0, 0.0], "minecraft:overworld", false);
    let mut wps = Waypoints::default();
    wps.list.push(Waypoint { x: 20, z: 0, dimension: "minecraft:overworld".into(), ..Waypoint::new("Base", [20, 70, 0], "minecraft:overworld") });
    let at = ui.screen_of(20.5, 0.5);
    ui.press([at[0] + 3.0, at[1] - 2.0], 1, &wps);
    let m = ui.menu.unwrap();
    assert_eq!(m.wp, Some(0));
    assert_eq!(m.items().len(), crate::engine::ui::MENU_WAYPOINT.len());
    // "Hide" saves the waypoint hidden; "Delete" deletes it.
    let r = ui.menu_item_rect(&m, 1);
    match ui.press([r.x + 2.0, r.y + 2.0], 0, &wps) {
        Action::SaveWaypoint(Some(0), w) => assert!(!w.visible),
        other => panic!("{other:?}"),
    }
    ui.press([at[0], at[1]], 1, &wps);
    let m = ui.menu.unwrap();
    let r = ui.menu_item_rect(&m, 5);
    assert_eq!(ui.press([r.x + 2.0, r.y + 2.0], 0, &wps), Action::DeleteWaypoint(0));
    // Away from any waypoint: the map's menu.
    ui.press([100.0, 200.0], 1, &wps);
    assert_eq!(ui.menu.unwrap().wp, None);
    // The editor: one click picks a colour and an icon.
    ui.menu = None;
    ui.new_waypoint(&wps, [1, 2, 3]);
    let (c, i) = (ui.dialog_swatch(5), ui.dialog_icon(4));
    ui.press([c.x + 2.0, c.y + 2.0], 0, &wps);
    ui.press([i.x + 2.0, i.y + 2.0], 0, &wps);
    let e = ui.edit.as_ref().unwrap();
    assert_eq!((e.draft.color, e.draft.icon), (5, 4));
}
