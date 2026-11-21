//! The info block under the minimap (0.3.5): one panel as wide as its widest line, centred under the frame or
//! flush with one of its edges, lines wrapped at spaces (or shortened with "..") so nothing is clipped by the screen edge.

/// Row height and the panel's padding, GUI px.
pub const ROW_H: f32 = 10.0;
pub const PAD: f32 = 2.0;
/// The warning icon's advance (8 px glyph + 2 px gap).
pub const ICON_W: f32 = 10.0;
/// The panel may grow past the frame's width up to this, GUI px.
pub const MAX_W: f32 = 160.0;

/// One line to show: its text, an icon in front (the hostile warning), its colour.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    pub icon: bool,
    pub color: u32,
}

/// One drawn row: the text at (x, y) GUI px, the icon (if any) at x - ICON_W.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub text: String,
    pub icon: bool,
    pub color: u32,
    pub x: f32,
    pub y: f32,
    pub w: f32,
}

/// The laid-out block: the panel rect [x, y, w, h] and its rows.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub rect: [f32; 4],
    pub rows: Vec<Row>,
}

/// Splits `text` into pieces no wider than `max` px: at spaces first, a single over-long word shortened with "..".
pub fn wrap(text: &str, max: f32, width: &dyn Fn(&str) -> f32) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in text.split(' ').filter(|w| !w.is_empty()) {
        let try_ = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if width(&try_) <= max {
            cur = try_;
            continue;
        }
        if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        if width(word) <= max {
            cur = word.to_string();
        } else {
            out.push(shorten(word, max, width));
        }
    }
    if !cur.is_empty() || out.is_empty() {
        out.push(cur);
    }
    out
}

/// `s` cut to fit `max` px with ".." at the end (just ".." when even one letter does not fit).
pub fn shorten(s: &str, max: f32, width: &dyn Fn(&str) -> f32) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    let mut chars: Vec<char> = s.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let t: String = chars.iter().collect::<String>() + "..";
        if width(&t) <= max {
            return t;
        }
    }
    "..".to_string()
}

/// The panel width the lines need (capped), before placing: what `Corners::place` must reserve with the rows.
pub fn measure(lines: &[Line], frame_w: f32, screen_w: f32, width: &dyn Fn(&str) -> f32) -> (f32, usize) {
    let cap = frame_w.max(MAX_W).min(screen_w - 2.0 * PAD - 2.0).max(16.0);
    let inner = cap - 2.0 * PAD;
    let mut widest = 0.0f32;
    let mut rows = 0;
    for l in lines {
        let iw = if l.icon { ICON_W } else { 0.0 };
        for piece in wrap(&l.text, inner - iw, width) {
            widest = widest.max(width(&piece) + iw);
            rows += 1;
        }
    }
    ((widest + 2.0 * PAD).min(cap), rows)
}

/// How the block lines up under the minimap (the `info_align` setting).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    /// The panel centred under the frame, each row centred in it (the default).
    Center,
    /// Flush with the frame's left edge, rows left-aligned.
    Left,
    /// Flush with the frame's right edge, rows right-aligned.
    Right,
}

impl Align {
    /// The setting value: 0 centre, 1 left, 2 right.
    pub fn of(mode: u8) -> Align {
        match mode {
            1 => Align::Left,
            2 => Align::Right,
            _ => Align::Center,
        }
    }
}

/// Lays the lines out under (or over) the minimap frame `frame` = [x, y, w, h]: centred under it or flush with one
/// of its edges (`align`), each row aligned the same way, the panel kept inside the screen.
pub fn layout(lines: &[Line], frame: [f32; 4], align: Align, below: bool, screen_w: f32, width: &dyn Fn(&str) -> f32) -> Block {
    let (w, n) = measure(lines, frame[2], screen_w, width);
    let h = n as f32 * ROW_H;
    let mut x = match align {
        Align::Left => frame[0],
        Align::Right => frame[0] + frame[2] - w,
        // Whole pixels, so the text does not land between them.
        Align::Center => (frame[0] + (frame[2] - w) / 2.0).round(),
    };
    x = x.min(screen_w - w - 1.0).max(1.0);
    let y = if below { frame[1] + frame[3] + 1.0 } else { frame[1] - h - 1.0 };
    let inner = w - 2.0 * PAD;
    let mut rows = Vec::with_capacity(n);
    let mut ry = y;
    for l in lines {
        let iw = if l.icon { ICON_W } else { 0.0 };
        for (k, piece) in wrap(&l.text, inner - iw, width).into_iter().enumerate() {
            let tw = width(&piece);
            // The icon sits in front of the first row only; the rest of the line lines up under the text.
            let tx = match align {
                Align::Left => x + PAD + iw,
                Align::Right => x + w - PAD - tw,
                Align::Center => x + PAD + iw + ((inner - iw - tw) / 2.0).floor(),
            };
            rows.push(Row { text: piece, icon: l.icon && k == 0, color: l.color, x: tx, y: ry + 1.0, w: tw });
            ry += ROW_H;
        }
    }
    Block { rect: [x, y, w, h], rows }
}

/// "Day 2480 · 14:21", with the time-of-day word only when it adds something (dawn, dusk, night): the day word
/// and the "day" period are the same word in Russian ("День ... День").
pub fn time_line(day_word: &str, day: i64, tick: i64, period: Option<&str>) -> String {
    let t = tick.rem_euclid(24000);
    // Tick 0 is 06:00.
    let minutes = ((t + 6000) % 24000) * 1440 / 24000;
    let mut s = format!("{day_word} {day} · {:02}:{:02}", minutes / 60, minutes % 60);
    if let Some(p) = period.filter(|p| !p.is_empty()) {
        s += " · ";
        s += p;
    }
    s
}

/// The hostile warning: "2 · 21 m" after the icon, or "hostile: 2 · 21 m" as text.
pub fn hostile_line(count: u32, nearest: f64, unit: &str, word: Option<&str>) -> String {
    let core = format!("{count} · {:.0} {unit}", nearest.max(0.0));
    match word {
        Some(w) => format!("{w}: {core}"),
        None => core,
    }
}
