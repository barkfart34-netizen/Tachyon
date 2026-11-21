//! Threat radar: hostile mobs near the player, as minimap blips plus a count, from the client's entity list
//! (what the player's client already tracks; nothing a server hides is revealed).

pub const HOSTILE: &[&str] = &[
    "minecraft:zombie", "minecraft:zombie_villager", "minecraft:husk", "minecraft:drowned",
    "minecraft:skeleton", "minecraft:stray", "minecraft:bogged", "minecraft:wither_skeleton",
    "minecraft:creeper", "minecraft:spider", "minecraft:cave_spider", "minecraft:enderman",
    "minecraft:witch", "minecraft:slime", "minecraft:magma_cube", "minecraft:phantom",
    "minecraft:blaze", "minecraft:ghast", "minecraft:piglin_brute", "minecraft:hoglin",
    "minecraft:zoglin", "minecraft:pillager", "minecraft:vindicator", "minecraft:evoker",
    "minecraft:ravager", "minecraft:vex", "minecraft:silverfish", "minecraft:endermite",
    "minecraft:guardian", "minecraft:elder_guardian", "minecraft:shulker", "minecraft:warden",
    "minecraft:breeze", "minecraft:creaking",
];

pub fn is_hostile(kind: &str) -> bool {
    HOSTILE.contains(&kind)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Blip {
    /// Entity kind (`minecraft:zombie`), for the per-kind style.
    pub kind: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub hostile: bool,
    /// A creeper within 6 blocks: drawn blinking.
    pub urgent: bool,
    /// Index of the entity in the scanned list (the glue looks its texture up by it).
    pub idx: usize,
}

pub struct Radar {
    pub range: f64,
    pub blips: Vec<Blip>,
    pub hostile_count: u32,
    pub nearest: Option<f64>,
}

impl Radar {
    pub fn new(range: f64) -> Self {
        Radar { range, blips: Vec::with_capacity(64), hostile_count: 0, nearest: None }
    }

    /// Refills the blips (buffer reused) from `(kind, position, alive)` entities around `me`.
    pub fn scan<'a>(&mut self, me: [f64; 3], entities: impl Iterator<Item = (&'a str, [f64; 3], bool)>, passive: bool) {
        self.blips.clear();
        self.hostile_count = 0;
        self.nearest = None;
        let r2 = self.range * self.range;
        for (idx, (kind, p, alive)) in entities.enumerate() {
            if !alive || kind == "minecraft:player" && p == me {
                continue;
            }
            let (dx, dy, dz) = (p[0] - me[0], p[1] - me[1], p[2] - me[2]);
            let d2 = dx * dx + dz * dz;
            if d2 > r2 || dy.abs() > 32.0 {
                continue;
            }
            let hostile = is_hostile(kind);
            if !hostile && !passive {
                continue;
            }
            if hostile {
                self.hostile_count += 1;
                let d = (d2 + dy * dy).sqrt();
                self.nearest = Some(self.nearest.map_or(d, |n: f64| n.min(d)));
            }
            let urgent = kind == "minecraft:creeper" && d2 < 36.0;
            if self.blips.len() < 256 {
                self.blips.push(Blip { kind: kind.to_string(), x: p[0], y: p[1], z: p[2], hostile, urgent, idx });
            }
        }
    }
}
