//! Waypoints for one world, every dimension in one list; saved as tab-separated lines with escaping.

use super::color;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Normal,
    Death,
}

/// Icon names; the art is `assets/megamap/textures/gui/wp_<name>.png`, the letter is the fallback.
/// Waypoint icons: the name's lang key and the letter drawn when the waypoint has no name.
pub const ICONS: [(&str, char); 6] = [
    ("megamap.wp_icon.diamond", 'D'),
    ("megamap.wp_icon.star", 'S'),
    ("megamap.wp_icon.home", 'H'),
    ("megamap.wp_icon.skull", 'X'),
    ("megamap.wp_icon.flag", 'F'),
    ("megamap.wp_icon.cross", '+'),
];
pub const DEATH_ICON: usize = 3;

#[derive(Clone, Debug, PartialEq)]
pub struct Waypoint {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub dimension: String,
    /// Index into `color::PALETTE`.
    pub color: usize,
    /// Index into `ICONS`.
    pub icon: usize,
    pub kind: Kind,
    pub visible: bool,
}

impl Waypoint {
    pub fn new(name: &str, pos: [i32; 3], dimension: &str) -> Self {
        Waypoint {
            name: name.into(),
            x: pos[0],
            y: pos[1],
            z: pos[2],
            dimension: dimension.into(),
            color: 0,
            icon: 0,
            kind: Kind::Normal,
            visible: true,
        }
    }
    pub fn argb(&self) -> u32 {
        color::PALETTE[self.color % color::PALETTE.len()]
    }
    pub fn letter(&self) -> char {
        self.name.chars().next().map(|c| c.to_ascii_uppercase()).unwrap_or(ICONS[self.icon % ICONS.len()].1)
    }
    /// The chat line another megamap user's `/megamap wp import` (or a human) can read.
    pub fn share_text(&self) -> String {
        format!("[megamap] {} @ {} {} {} ({})", self.name, self.x, self.y, self.z, short_dim(&self.dimension))
    }
}

pub fn short_dim(d: &str) -> &str {
    d.strip_prefix("minecraft:").unwrap_or(d)
}

/// Parses `share_text` back: `(name, pos, dimension)`.
pub fn parse_share(s: &str) -> Option<(String, [i32; 3], String)> {
    let rest = &s[s.find("[megamap] ")? + 10..];
    let (name, rest) = rest.rsplit_once(" @ ")?;
    let (coords, dim) = rest.split_once(" (")?;
    let dim = dim.strip_suffix(')')?;
    let v: Vec<i32> = coords.split_whitespace().map(|t| t.parse().ok()).collect::<Option<_>>()?;
    if v.len() != 3 {
        return None;
    }
    let dim = if dim.contains(':') { dim.to_string() } else { format!("minecraft:{dim}") };
    Some((name.to_string(), [v[0], v[1], v[2]], dim))
}

#[derive(Default, Clone, Debug, PartialEq)]
pub struct Waypoints {
    pub list: Vec<Waypoint>,
    pub generation: u64,
}

fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '\t' => o.push_str("\\t"),
            '\n' => o.push_str("\\n"),
            c => o.push(c),
        }
    }
    o
}

fn unescape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('t') => o.push('\t'),
                Some('n') => o.push('\n'),
                Some(c) => o.push(c),
                None => {}
            }
        } else {
            o.push(c);
        }
    }
    o
}

impl Waypoints {
    pub fn add(&mut self, w: Waypoint) -> usize {
        self.list.push(w);
        self.generation += 1;
        self.list.len() - 1
    }

    pub fn remove(&mut self, i: usize) -> Option<Waypoint> {
        if i < self.list.len() {
            self.generation += 1;
            Some(self.list.remove(i))
        } else {
            None
        }
    }

    pub fn replace(&mut self, i: usize, w: Waypoint) {
        if let Some(slot) = self.list.get_mut(i) {
            *slot = w;
            self.generation += 1;
        }
    }

    /// A unique default name: "Waypoint 3".
    pub fn next_name(&self, base: &str) -> String {
        (1..).map(|n| format!("{base} {n}")).find(|n| !self.list.iter().any(|w| &w.name == n)).unwrap()
    }

    /// Adds a death point, dropping the oldest beyond `keep` (0 keeps none).
    pub fn add_death(&mut self, pos: [i32; 3], dimension: &str, keep: usize, name: &str) {
        if keep == 0 {
            return;
        }
        let mut w = Waypoint::new(name, pos, dimension);
        w.kind = Kind::Death;
        w.icon = DEATH_ICON;
        w.color = 0;
        self.add(w);
        while self.list.iter().filter(|w| w.kind == Kind::Death).count() > keep {
            let first = self.list.iter().position(|w| w.kind == Kind::Death).unwrap();
            self.list.remove(first);
        }
    }

    /// Indices of the waypoints in `dimension` (all when `None`) whose name contains `query` (case-insensitive),
    /// nearest to `from` first.
    pub fn filter(&self, query: &str, dimension: Option<&str>, from: [f64; 3]) -> Vec<usize> {
        let q = query.to_lowercase();
        let mut v: Vec<usize> = (0..self.list.len())
            .filter(|&i| {
                let w = &self.list[i];
                dimension.is_none_or(|d| w.dimension == d) && (q.is_empty() || w.name.to_lowercase().contains(&q))
            })
            .collect();
        let d2 = |w: &Waypoint| (w.x as f64 - from[0]).powi(2) + (w.z as f64 - from[2]).powi(2);
        v.sort_by(|&a, &b| d2(&self.list[a]).total_cmp(&d2(&self.list[b])));
        v
    }

    pub fn in_dimension<'a>(&'a self, dim: &'a str) -> impl Iterator<Item = &'a Waypoint> + 'a {
        self.list.iter().filter(move |w| w.visible && w.dimension == dim)
    }

    pub fn to_text(&self) -> String {
        let mut s = String::from("# megamap waypoints v1: name x y z dimension color icon kind visible\n"); // lang-ok: file header
        for w in &self.list {
            s.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                escape(&w.name),
                w.x,
                w.y,
                w.z,
                escape(&w.dimension),
                color::PALETTE_NAMES[w.color % color::PALETTE.len()],
                ICONS[w.icon % ICONS.len()].0,
                if w.kind == Kind::Death { "death" } else { "normal" },
                w.visible,
            ));
        }
        s
    }

    pub fn from_text(text: &str) -> Waypoints {
        let mut out = Waypoints::default();
        for line in text.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 5 {
                continue;
            }
            let (Ok(x), Ok(y), Ok(z)) = (f[1].parse(), f[2].parse(), f[3].parse()) else { continue };
            let mut w = Waypoint::new(&unescape(f[0]), [x, y, z], &unescape(f[4]));
            if let Some(c) = f.get(5) {
                w.color = color::PALETTE_NAMES.iter().position(|n| n == c).unwrap_or(0);
            }
            if let Some(i) = f.get(6) {
                w.icon = ICONS.iter().position(|n| n.0 == *i).unwrap_or(0);
            }
            if f.get(7) == Some(&"death") {
                w.kind = Kind::Death;
            }
            if f.get(8) == Some(&"false") {
                w.visible = false;
            }
            out.list.push(w);
        }
        out
    }
}
