//! The entity catalog for the radar and the map: every known kind with a category and a default look (colour,
//! icon, size, where it shows). The player overrides any of it per kind in the entities window; overrides are
//! kept in `Settings::entities` and saved as `ent.<kind>=...` lines.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Hostile,
    Passive,
    Players,
    Other,
}

impl Category {
    pub const ALL: [Category; 4] = [Category::Hostile, Category::Passive, Category::Players, Category::Other];
    /// Its translation key.
    pub fn name_key(self) -> &'static str {
        match self {
            Category::Hostile => "megamap.ent_cat.hostile",
            Category::Passive => "megamap.ent_cat.passive",
            Category::Players => "megamap.ent_cat.players",
            Category::Other => "megamap.ent_cat.other",
        }
    }
    /// The frame colour of face icons / sprites (ARGB): players blue, hostile red, passive green, other amber.
    pub fn frame_color(self) -> u32 {
        match self {
            Category::Hostile => 0xFFE5_3935,
            Category::Passive => 0xFF43_A047,
            Category::Players => 0xFF42_A5F5,
            Category::Other => 0xFFFF_B300,
        }
    }
    pub fn index(self) -> usize {
        Category::ALL.iter().position(|&c| c == self).unwrap_or(0)
    }
}

/// Icon shapes, drawn from quads by the glue (`icon_quads`).
/// Keys of the icon names.
pub const ICONS: [&str; 6] = ["megamap.ent_icon.0", "megamap.ent_icon.1", "megamap.ent_icon.2", "megamap.ent_icon.3", "megamap.ent_icon.4", "megamap.ent_icon.5"];
/// Icon side in GUI pixels for size 0..=4.
pub const ICON_PX: [f32; 5] = [2.0, 3.0, 4.0, 5.0, 7.0];
/// Colour choices the window cycles through (0xRRGGBB).
pub const COLORS: [u32; 12] = [
    0xFF4040, 0xFF9030, 0xFFE040, 0x60E060, 0x40C0C0, 0x4090FF, 0xA070FF, 0xFF70C0, 0xFFFFFF, 0xA0A0A0, 0x8B5A2B,
    0x202020,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    /// Shown on the minimap radar.
    pub mini: bool,
    /// Shown on the world map.
    pub map: bool,
    pub color: u32,
    /// Index into `ICONS`.
    pub icon: u8,
    /// Index into `ICON_PX`.
    pub size: u8,
}

impl Style {
    pub fn to_text(&self) -> String {
        format!("{},{},{:06x},{},{}", self.mini as u8, self.map as u8, self.color & 0xFFFFFF, self.icon, self.size)
    }
    pub fn from_text(v: &str, d: Style) -> Style {
        let p: Vec<&str> = v.split(',').map(str::trim).collect();
        if p.len() != 5 {
            return d;
        }
        let flag = |s: &str, d: bool| match s {
            "1" | "true" | "on" => true,
            "0" | "false" | "off" => false,
            _ => d,
        };
        Style {
            mini: flag(p[0], d.mini),
            map: flag(p[1], d.map),
            color: u32::from_str_radix(p[2], 16).map(|c| c & 0xFFFFFF).unwrap_or(d.color),
            icon: p[3].parse::<u8>().ok().filter(|&i| (i as usize) < ICONS.len()).unwrap_or(d.icon),
            size: p[4].parse::<u8>().ok().filter(|&i| (i as usize) < ICON_PX.len()).unwrap_or(d.size),
        }
    }
    pub fn px(&self) -> f32 {
        ICON_PX[(self.size as usize).min(ICON_PX.len() - 1)]
    }
}

/// (kind, category, default colour). Hostile list matches `radar::HOSTILE`.
pub const CATALOG: &[(&str, Category, u32)] = &[
    ("minecraft:player", Category::Players, 0xFFFFFF),
    ("minecraft:zombie", Category::Hostile, 0x4F9A3A),
    ("minecraft:zombie_villager", Category::Hostile, 0x4F9A3A),
    ("minecraft:husk", Category::Hostile, 0xB59A5A),
    ("minecraft:drowned", Category::Hostile, 0x3AA0A0),
    ("minecraft:skeleton", Category::Hostile, 0xD0D0D0),
    ("minecraft:stray", Category::Hostile, 0x9CB8C0),
    ("minecraft:bogged", Category::Hostile, 0x7A8A40),
    ("minecraft:wither_skeleton", Category::Hostile, 0x303030),
    ("minecraft:creeper", Category::Hostile, 0x40E040),
    ("minecraft:spider", Category::Hostile, 0x6A4A3A),
    ("minecraft:cave_spider", Category::Hostile, 0x1F5A6A),
    ("minecraft:enderman", Category::Hostile, 0xB050E0),
    ("minecraft:witch", Category::Hostile, 0x6A2A8A),
    ("minecraft:slime", Category::Hostile, 0x70C050),
    ("minecraft:magma_cube", Category::Hostile, 0xE05010),
    ("minecraft:phantom", Category::Hostile, 0x4A5AA0),
    ("minecraft:blaze", Category::Hostile, 0xF0B020),
    ("minecraft:ghast", Category::Hostile, 0xF0F0F0),
    ("minecraft:piglin_brute", Category::Hostile, 0xC08040),
    ("minecraft:hoglin", Category::Hostile, 0xC07060),
    ("minecraft:zoglin", Category::Hostile, 0xD08080),
    ("minecraft:pillager", Category::Hostile, 0x5A5A5A),
    ("minecraft:vindicator", Category::Hostile, 0x4A6A6A),
    ("minecraft:evoker", Category::Hostile, 0x8A8A8A),
    ("minecraft:ravager", Category::Hostile, 0x5A4A40),
    ("minecraft:vex", Category::Hostile, 0xA0B0D0),
    ("minecraft:silverfish", Category::Hostile, 0x8A8A9A),
    ("minecraft:endermite", Category::Hostile, 0x6A3A8A),
    ("minecraft:guardian", Category::Hostile, 0x5AA090),
    ("minecraft:elder_guardian", Category::Hostile, 0xB0B090),
    ("minecraft:shulker", Category::Hostile, 0x9A6A9A),
    ("minecraft:warden", Category::Hostile, 0x0A3A4A),
    ("minecraft:breeze", Category::Hostile, 0xA0C0F0),
    ("minecraft:creaking", Category::Hostile, 0x6A5040),
    ("minecraft:cow", Category::Passive, 0x5A3A2A),
    ("minecraft:pig", Category::Passive, 0xF0A0A0),
    ("minecraft:sheep", Category::Passive, 0xE8E8E8),
    ("minecraft:chicken", Category::Passive, 0xF0F0E0),
    ("minecraft:horse", Category::Passive, 0x9A6A40),
    ("minecraft:donkey", Category::Passive, 0x7A6A5A),
    ("minecraft:mule", Category::Passive, 0x5A4030),
    ("minecraft:llama", Category::Passive, 0xD0C0A0),
    ("minecraft:rabbit", Category::Passive, 0xB09070),
    ("minecraft:wolf", Category::Passive, 0xD0D0D0),
    ("minecraft:cat", Category::Passive, 0xE0B060),
    ("minecraft:fox", Category::Passive, 0xE07020),
    ("minecraft:villager", Category::Passive, 0x8A6A4A),
    ("minecraft:wandering_trader", Category::Passive, 0x3A5AA0),
    ("minecraft:iron_golem", Category::Passive, 0xC0C0B0),
    ("minecraft:bee", Category::Passive, 0xF0C020),
    ("minecraft:turtle", Category::Passive, 0x40A040),
    ("minecraft:axolotl", Category::Passive, 0xF090C0),
    ("minecraft:goat", Category::Passive, 0xD8D0C0),
    ("minecraft:frog", Category::Passive, 0xC08040),
    ("minecraft:camel", Category::Passive, 0xD0A060),
    ("minecraft:sniffer", Category::Passive, 0xA04030),
    ("minecraft:armadillo", Category::Passive, 0xA06A5A),
    ("minecraft:squid", Category::Passive, 0x2A3A6A),
    ("minecraft:glow_squid", Category::Passive, 0x30C0B0),
    ("minecraft:dolphin", Category::Passive, 0x7090B0),
    ("minecraft:strider", Category::Passive, 0xA03030),
    ("minecraft:piglin", Category::Passive, 0xE0A060),
    ("minecraft:zombified_piglin", Category::Passive, 0xD08A70),
    ("minecraft:item", Category::Other, 0xF0E080),
    ("minecraft:experience_orb", Category::Other, 0x90F040),
    ("minecraft:boat", Category::Other, 0x9A6A30),
    ("minecraft:minecart", Category::Other, 0x8A8A8A),
    ("minecraft:arrow", Category::Other, 0xC0C0C0),
    ("minecraft:armor_stand", Category::Other, 0xB09060),
    ("minecraft:item_frame", Category::Other, 0x9A6A30),
    ("minecraft:tnt", Category::Other, 0xE02020),
];

pub fn category(kind: &str) -> Category {
    match CATALOG.iter().find(|e| e.0 == kind) {
        Some(e) => e.1,
        None if super::radar::is_hostile(kind) => Category::Hostile,
        None => Category::Other,
    }
}

pub fn default_style(kind: &str) -> Style {
    let cat = category(kind);
    let color = CATALOG.iter().find(|e| e.0 == kind).map_or(0xA0A0A0, |e| e.2);
    match cat {
        // Hostile mobs: red-ish diamonds everywhere, by their own colour so a creeper reads differently.
        Category::Hostile => Style { mini: true, map: true, color, icon: 1, size: 2 },
        // Passive mobs crowd the minimap; on the map only.
        Category::Passive => Style { mini: false, map: true, color, icon: 0, size: 1 },
        Category::Players => Style { mini: true, map: true, color, icon: 5, size: 3 },
        Category::Other => Style { mini: false, map: false, color, icon: 0, size: 0 },
    }
}

/// A readable name: `minecraft:cave_spider` -> `Cave spider`, other namespaces keep theirs in brackets.
pub fn display_name(kind: &str) -> String {
    let (ns, path) = kind.split_once(':').unwrap_or(("minecraft", kind));
    let mut s: String = path.replace('_', " ");
    if let Some(f) = s.get(..1) {
        let up = f.to_ascii_uppercase();
        s.replace_range(..1, &up);
    }
    if ns != "minecraft" {
        s = format!("{s} ({ns})");
    }
    s
}

/// Catalog kinds plus `seen` ones, filtered by a case-insensitive search over kind and name, optionally by
/// category, sorted by category then name.
pub fn list(seen: &[String], search: &str, cat: Option<Category>) -> Vec<String> {
    list_named(seen, search, cat, &mut |k| display_name(k))
}

/// `list` with the shown names from `name` (the game's or the built-in translation): the search matches the shown
/// name or the id (`lang::matches`), and the rows sort by category, then shown name.
pub fn list_named(seen: &[String], search: &str, cat: Option<Category>, name: &mut dyn FnMut(&str) -> String) -> Vec<String> {
    let mut out: Vec<String> = CATALOG.iter().map(|e| e.0.to_string()).collect();
    for k in seen {
        if !out.contains(k) {
            out.push(k.clone());
        }
    }
    let mut named: Vec<(String, String)> = out.into_iter().map(|k| (name(&k), k)).collect();
    named.retain(|(n, k)| cat.is_none_or(|c| category(k) == c) && super::lang::matches(search, n, k));
    named.sort_by(|a, b| (category(&a.1), a.0.to_lowercase()).cmp(&(category(&b.1), b.0.to_lowercase())));
    named.into_iter().map(|(_, k)| k).collect()
}

/// The icon as axis-aligned rectangles `[x, y, w, h]` around a centre, in GUI pixels (outline first, then
/// fill), so the glue draws it with `fill` and no sprites. `outline` is true for the dark border rects.
pub fn icon_rects(icon: u8, c: [f32; 2], px: f32) -> Vec<([f32; 4], bool)> {
    let h = (px / 2.0).max(1.0);
    let (x, y) = (c[0], c[1]);
    let mut v = Vec::new();
    match icon {
        // Diamond: stacked bars of shrinking width.
        1 => {
            let n = (px.round() as i32).max(2);
            v.push(([x - h - 1.0, y - 0.5, px + 2.0, 1.0], true));
            v.push(([x - 0.5, y - h - 1.0, 1.0, px + 2.0], true));
            for i in 0..n {
                let t = (i as f32 + 0.5) / n as f32 * 2.0 - 1.0;
                let w = (px * (1.0 - t.abs())).max(1.0);
                v.push(([x - w / 2.0, y - h + i as f32 * px / n as f32, w, px / n as f32], false));
            }
        }
        // Triangle pointing up.
        2 => {
            let n = (px.round() as i32).max(2);
            v.push(([x - h - 1.0, y + h - 0.5, px + 2.0, 1.5], true));
            for i in 0..n {
                let w = (px * (i as f32 + 1.0) / n as f32).max(1.0);
                v.push(([x - w / 2.0, y - h + i as f32 * px / n as f32, w, px / n as f32], false));
            }
        }
        // Cross.
        3 => {
            let t = (px / 3.0).max(1.0);
            v.push(([x - h - 0.5, y - t / 2.0 - 0.5, px + 1.0, t + 1.0], true));
            v.push(([x - t / 2.0 - 0.5, y - h - 0.5, t + 1.0, px + 1.0], true));
            v.push(([x - h, y - t / 2.0, px, t], false));
            v.push(([x - t / 2.0, y - h, t, px], false));
        }
        // Ring: four sides.
        4 => {
            let t = (px / 4.0).max(1.0);
            v.push(([x - h - 0.5, y - h - 0.5, px + 1.0, px + 1.0], true));
            v.push(([x - h, y - h, px, t], false));
            v.push(([x - h, y + h - t, px, t], false));
            v.push(([x - h, y - h, t, px], false));
            v.push(([x + h - t, y - h, t, px], false));
        }
        // Head: a square with a dark border.
        5 => {
            v.push(([x - h - 1.0, y - h - 1.0, px + 2.0, px + 2.0], true));
            v.push(([x - h, y - h, px, px], false));
        }
        _ => {
            v.push(([x - h - 0.5, y - h - 0.5, px + 1.0, px + 1.0], true));
            v.push(([x - h, y - h, px, px], false));
        }
    }
    v
}
