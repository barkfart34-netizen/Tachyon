//! Client custom GUIs for Create display & threshold blocks:
//! - Display Link (`create:display_link`) -> `create:dl_open` / `create:dl_save`
//! - Threshold Switch (`create:stockpile_switch`) -> `create:ts_open` / `create:ts_save`

use std::sync::{Mutex, PoisonError};

use create_kinetics::rules::trains::station_screen::TextField;
use crate::compat::{
    ClientTickPostEvent, Flow, Hooks, Host, NetFromServerEvent, ScreenCharTypedPreEvent,
    ScreenClosingEvent, ScreenKeyPressedPreEvent, ScreenMouseButtonPressedPreEvent,
    ScreenRenderPostEvent,
};

use super::create_gui::{
    self as gui, clean_wire, BtnStyle, UiRect, FRAME_BRASS, PANEL_BRASS, SLOT_BG, TEXT_GOLD,
    TEXT_GREEN, TEXT_LIGHT, TEXT_MUTED, TEXT_RED,
};

pub const DL_OPEN_CHANNEL: &str = "create:dl_open";
pub const DL_SAVE_CHANNEL: &str = "create:dl_save";
pub const TS_OPEN_CHANNEL: &str = "create:ts_open";
pub const TS_SAVE_CHANNEL: &str = "create:ts_save";

const TAG: &str = "redstone_ui";
const SEP: char = '\u{1f}';
const SUB_SEP: char = '\u{1e}';
const PANEL_W: i32 = 348;
const PANEL_H: i32 = 212;

struct Screen;

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Screen)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayLinkOpenPayload {
    pub pos: [i32; 3],
    pub source_block: String,
    pub target_block: String,
    pub sources: Vec<String>,
    pub source_idx: usize,
    pub option_label: String,
    pub options: Vec<String>,
    pub option_idx: usize,
    pub label: String,
    pub has_filter: bool,
    pub filter: String,
    pub target_line: usize,
    pub target_lines: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayLinkSavePayload {
    pub pos: [i32; 3],
    pub source_idx: usize,
    pub option_idx: usize,
    pub label: String,
    pub has_filter: bool,
    pub filter: String,
    pub target_line: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThresholdOpenPayload {
    pub pos: [i32; 3],
    /// 0 = Unsupported, 1 = Items, 2 = Fluids (mB).
    pub kind: u8,
    pub current: i32,
    pub min_val: i32,
    pub max_val: i32,
    pub on_above: i32,
    pub off_below: i32,
    pub inverted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThresholdSavePayload {
    pub pos: [i32; 3],
    pub on_above: i32,
    pub off_below: i32,
    pub inverted: bool,
}

fn enc_list(v: &[String]) -> String {
    v.iter().map(|s| clean_wire(s)).collect::<Vec<_>>().join(&SUB_SEP.to_string())
}

fn dec_list(s: &str) -> Vec<String> {
    if s.is_empty() {
        Vec::new()
    } else {
        s.split(SUB_SEP).map(str::to_owned).collect()
    }
}

pub fn encode_dl_open(p: &DisplayLinkOpenPayload) -> String {
    format!(
        "dl1{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}",
        p.pos[0],
        p.pos[1],
        p.pos[2],
        clean_wire(&p.source_block),
        clean_wire(&p.target_block),
        enc_list(&p.sources),
        p.source_idx,
        clean_wire(&p.option_label),
        enc_list(&p.options),
        p.option_idx,
        clean_wire(&p.label),
        u8::from(p.has_filter),
        clean_wire(&p.filter),
        p.target_line,
        enc_list(&p.target_lines),
        s = SEP,
    )
}

pub fn decode_dl_open(text: &str) -> Option<DisplayLinkOpenPayload> {
    let mut it = text.split(SEP);
    if it.next()? != "dl1" {
        return None;
    }
    Some(DisplayLinkOpenPayload {
        pos: [it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?],
        source_block: it.next()?.to_owned(),
        target_block: it.next()?.to_owned(),
        sources: dec_list(it.next()?),
        source_idx: it.next()?.parse().ok()?,
        option_label: it.next()?.to_owned(),
        options: dec_list(it.next()?),
        option_idx: it.next()?.parse().ok()?,
        label: it.next()?.to_owned(),
        has_filter: it.next()? == "1",
        filter: it.next()?.to_owned(),
        target_line: it.next()?.parse().ok()?,
        target_lines: dec_list(it.next()?),
    })
}

pub fn encode_dl_save(p: &DisplayLinkSavePayload) -> String {
    format!(
        "dls1{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}",
        p.pos[0],
        p.pos[1],
        p.pos[2],
        p.source_idx,
        p.option_idx,
        clean_wire(&p.label),
        u8::from(p.has_filter),
        clean_wire(&p.filter),
        p.target_line,
        s = SEP,
    )
}

pub fn decode_dl_save(text: &str) -> Option<DisplayLinkSavePayload> {
    let mut it = text.split(SEP);
    if it.next()? != "dls1" {
        return None;
    }
    Some(DisplayLinkSavePayload {
        pos: [it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?],
        source_idx: it.next()?.parse().ok()?,
        option_idx: it.next()?.parse().ok()?,
        label: it.next()?.to_owned(),
        has_filter: it.next()? == "1",
        filter: it.next()?.to_owned(),
        target_line: it.next()?.parse().ok()?,
    })
}

pub fn encode_ts_open(p: &ThresholdOpenPayload) -> String {
    format!(
        "ts1{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}",
        p.pos[0],
        p.pos[1],
        p.pos[2],
        p.kind,
        p.current,
        p.min_val,
        p.max_val,
        p.on_above,
        p.off_below,
        u8::from(p.inverted),
        s = SEP,
    )
}

pub fn decode_ts_open(text: &str) -> Option<ThresholdOpenPayload> {
    let mut it = text.split(SEP);
    if it.next()? != "ts1" {
        return None;
    }
    Some(ThresholdOpenPayload {
        pos: [it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?],
        kind: it.next()?.parse().ok()?,
        current: it.next()?.parse().ok()?,
        min_val: it.next()?.parse().ok()?,
        max_val: it.next()?.parse().ok()?,
        on_above: it.next()?.parse().ok()?,
        off_below: it.next()?.parse().ok()?,
        inverted: it.next()? == "1",
    })
}

pub fn encode_ts_save(p: &ThresholdSavePayload) -> String {
    format!(
        "tss1{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}",
        p.pos[0],
        p.pos[1],
        p.pos[2],
        p.on_above,
        p.off_below,
        u8::from(p.inverted),
        s = SEP,
    )
}

pub fn decode_ts_save(text: &str) -> Option<ThresholdSavePayload> {
    let mut it = text.split(SEP);
    if it.next()? != "tss1" {
        return None;
    }
    Some(ThresholdSavePayload {
        pos: [it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?],
        on_above: it.next()?.parse().ok()?,
        off_below: it.next()?.parse().ok()?,
        inverted: it.next()? == "1",
    })
}

enum Mode {
    DisplayLink {
        data: DisplayLinkOpenPayload,
        label: TextField,
        filter: TextField,
    },
    Threshold {
        pos: [i32; 3],
        kind: u8,
        current: i32,
        min_val: i32,
        max_val: i32,
        on_above: i32,
        off_below: i32,
        inverted: bool,
    },
}

struct Open {
    mode: Mode,
    size: [i32; 2],
    close: bool,
}

enum OutMsg {
    Dl(DisplayLinkSavePayload),
    Ts(ThresholdSavePayload),
}

static CLIENT: Mutex<Option<Open>> = Mutex::new(None);
static OUTBOX: Mutex<Vec<OutMsg>> = Mutex::new(Vec::new());

fn client() -> std::sync::MutexGuard<'static, Option<Open>> {
    CLIENT.lock().unwrap_or_else(PoisonError::into_inner)
}

fn send_out(msg: OutMsg) {
    OUTBOX.lock().unwrap_or_else(PoisonError::into_inner).push(msg);
}

fn save_msg(mode: &Mode) -> OutMsg {
    match mode {
        Mode::DisplayLink { data, label, filter } => OutMsg::Dl(DisplayLinkSavePayload {
            pos: data.pos,
            source_idx: data.source_idx,
            option_idx: data.option_idx,
            label: label.value(),
            has_filter: data.has_filter,
            filter: filter.value(),
            target_line: data.target_line,
        }),
        Mode::Threshold { pos, on_above, off_below, inverted, .. } => OutMsg::Ts(ThresholdSavePayload {
            pos: *pos,
            on_above: *on_above,
            off_below: *off_below,
            inverted: *inverted,
        }),
    }
}

impl Hooks for Screen {
    fn net_from_server(&self, host: &mut dyn Host, event: &mut NetFromServerEvent<'_>) {
        let Ok(text) = std::str::from_utf8(event.data) else { return };
        match event.channel {
            DL_OPEN_CHANNEL => {
                let Some(p) = decode_dl_open(text) else { return };
                let mut label = TextField::new(48);
                label.set(&p.label);
                let mut filter = TextField::new(48);
                filter.set(&p.filter);
                if gui::open(host, TAG) {
                    *client() = Some(Open {
                        mode: Mode::DisplayLink { data: p, label, filter },
                        size: [0, 0],
                        close: false,
                    });
                }
            }
            TS_OPEN_CHANNEL => {
                let Some(p) = decode_ts_open(text) else { return };
                if gui::open(host, TAG) {
                    *client() = Some(Open {
                        mode: Mode::Threshold {
                            pos: p.pos,
                            kind: p.kind,
                            current: p.current,
                            min_val: p.min_val,
                            max_val: p.max_val,
                            on_above: p.on_above,
                            off_below: p.off_below,
                            inverted: p.inverted,
                        },
                        size: [0, 0],
                        close: false,
                    });
                }
            }
            _ => {}
        }
    }

    fn client_tick_post(&self, host: &mut dyn Host, _: &mut ClientTickPostEvent) {
        if !host.window() {
            return;
        }
        let pending = std::mem::take(&mut *OUTBOX.lock().unwrap_or_else(PoisonError::into_inner));
        for msg in pending {
            match msg {
                OutMsg::Dl(p) => {
                    let _ = host.net_send_to_server(DL_SAVE_CHANNEL, encode_dl_save(&p).as_bytes());
                }
                OutMsg::Ts(p) => {
                    let _ = host.net_send_to_server(TS_SAVE_CHANNEL, encode_ts_save(&p).as_bytes());
                }
            }
        }
        let close_now = {
            let mut c = client();
            if c.is_some() && gui::sync_closed(&*host, TAG) {
                *c = None;
                return;
            }
            let Some(open) = c.as_mut() else { return };
            if open.close {
                *c = None;
                true
            } else {
                false
            }
        };
        if close_now {
            gui::close(host, TAG);
        }
    }

    fn screen_closing(&self, _: &dyn Host, event: &mut ScreenClosingEvent<'_>) {
        if event.screen.id == "mod" {
            *client() = None;
            gui::on_screen_closing(TAG);
        }
    }

    fn screen_render_post(&self, host: &dyn Host, event: &mut ScreenRenderPostEvent<'_>) {
        if event.screen.id != "mod" || !gui::is_active(host, TAG) {
            return;
        }
        let mut c = client();
        let Some(open) = c.as_mut() else { return };
        open.size = [event.batch.width as i32, event.batch.height as i32];
        draw(host, event, open);
    }

    fn screen_mouse_button_pressed_pre(
        &self,
        host: &dyn Host,
        event: &mut ScreenMouseButtonPressedPreEvent<'_>,
    ) -> Flow {
        if event.screen.id != "mod" || !gui::is_active(host, TAG) {
            return Flow::Continue;
        }
        if event.button != 0 {
            return Flow::Cancel;
        }
        let mut c = client();
        let Some(open) = c.as_mut() else { return Flow::Continue };
        let (left, top) = gui::screen_origin(open.size, PANEL_W, PANEL_H);
        let mouse = event.screen.mouse;

        match &mut open.mode {
            Mode::DisplayLink { data, label, filter } => {
                let lbl_r = UiRect::new(left + 96, top + 112, 238, 18);
                let flt_r = UiRect::new(left + 96, top + 136, 238, 18);
                if lbl_r.contains(mouse) {
                    label.focus(true);
                    filter.focus(false);
                    return Flow::Cancel;
                }
                if data.has_filter && flt_r.contains(mouse) {
                    label.focus(false);
                    filter.focus(true);
                    return Flow::Cancel;
                }
                label.focus(false);
                filter.focus(false);

                let src_prev = UiRect::new(left + 96, top + 58, 22, 18);
                let src_next = UiRect::new(left + 312, top + 58, 22, 18);
                let opt_prev = UiRect::new(left + 96, top + 84, 22, 18);
                let opt_next = UiRect::new(left + 312, top + 84, 22, 18);
                let line_prev = UiRect::new(left + 96, top + 160, 22, 18);
                let line_next = UiRect::new(left + 312, top + 160, 22, 18);
                let cancel = UiRect::new(left + 212, top + 184, 60, 20);
                let save = UiRect::new(left + 278, top + 184, 60, 20);

                if !data.sources.is_empty() {
                    let n = data.sources.len();
                    if src_prev.contains(mouse) {
                        data.source_idx = (data.source_idx + n - 1) % n;
                        return Flow::Cancel;
                    }
                    if src_next.contains(mouse) {
                        data.source_idx = (data.source_idx + 1) % n;
                        return Flow::Cancel;
                    }
                }
                if !data.options.is_empty() {
                    let n = data.options.len();
                    if opt_prev.contains(mouse) {
                        data.option_idx = (data.option_idx + n - 1) % n;
                        return Flow::Cancel;
                    }
                    if opt_next.contains(mouse) {
                        data.option_idx = (data.option_idx + 1) % n;
                        return Flow::Cancel;
                    }
                }
                if !data.target_lines.is_empty() {
                    let n = data.target_lines.len();
                    if line_prev.contains(mouse) {
                        data.target_line = (data.target_line + n - 1) % n;
                        return Flow::Cancel;
                    }
                    if line_next.contains(mouse) {
                        data.target_line = (data.target_line + 1) % n;
                        return Flow::Cancel;
                    }
                }
                if cancel.contains(mouse) {
                    open.close = true;
                    return Flow::Cancel;
                }
                if save.contains(mouse) {
                    send_out(save_msg(&open.mode));
                    open.close = true;
                }
            }
            Mode::Threshold { kind, min_val, max_val, on_above, off_below, inverted, .. } => {
                let small = if *kind == 2 { 250 } else { 1 };
                let big = if *kind == 2 { 1000 } else { 16 };
                let ceiling = (*max_val).max(if *kind == 2 { 8000 } else { 64 });
                let lo = *min_val;

                let on_m_b = UiRect::new(left + 114, top + 84, 34, 18);
                let on_m_s = UiRect::new(left + 152, top + 84, 24, 18);
                let on_p_s = UiRect::new(left + 252, top + 84, 24, 18);
                let on_p_b = UiRect::new(left + 280, top + 84, 34, 18);

                let off_m_b = UiRect::new(left + 114, top + 110, 34, 18);
                let off_m_s = UiRect::new(left + 152, top + 110, 24, 18);
                let off_p_s = UiRect::new(left + 252, top + 110, 24, 18);
                let off_p_b = UiRect::new(left + 280, top + 110, 34, 18);

                let inv_btn = UiRect::new(left + 14, top + 144, 128, 20);
                let cancel = UiRect::new(left + 212, top + 182, 60, 20);
                let save = UiRect::new(left + 278, top + 182, 60, 20);

                if on_m_b.contains(mouse) {
                    *on_above = (*on_above - big).clamp(lo, ceiling);
                    *off_below = (*off_below).min(*on_above);
                    return Flow::Cancel;
                }
                if on_m_s.contains(mouse) {
                    *on_above = (*on_above - small).clamp(lo, ceiling);
                    *off_below = (*off_below).min(*on_above);
                    return Flow::Cancel;
                }
                if on_p_s.contains(mouse) {
                    *on_above = (*on_above + small).clamp(lo, ceiling);
                    return Flow::Cancel;
                }
                if on_p_b.contains(mouse) {
                    *on_above = (*on_above + big).clamp(lo, ceiling);
                    return Flow::Cancel;
                }

                if off_m_b.contains(mouse) {
                    *off_below = (*off_below - big).clamp(lo, *on_above);
                    return Flow::Cancel;
                }
                if off_m_s.contains(mouse) {
                    *off_below = (*off_below - small).clamp(lo, *on_above);
                    return Flow::Cancel;
                }
                if off_p_s.contains(mouse) {
                    *off_below = (*off_below + small).clamp(lo, *on_above);
                    return Flow::Cancel;
                }
                if off_p_b.contains(mouse) {
                    *off_below = (*off_below + big).clamp(lo, *on_above);
                    return Flow::Cancel;
                }

                if inv_btn.contains(mouse) {
                    *inverted = !*inverted;
                    return Flow::Cancel;
                }
                if cancel.contains(mouse) {
                    open.close = true;
                    return Flow::Cancel;
                }
                if save.contains(mouse) {
                    send_out(save_msg(&open.mode));
                    open.close = true;
                }
            }
        }
        Flow::Cancel
    }

    fn screen_char_typed_pre(&self, host: &dyn Host, event: &mut ScreenCharTypedPreEvent<'_>) -> Flow {
        if event.screen.id != "mod" || !gui::is_active(host, TAG) {
            return Flow::Continue;
        }
        let mut c = client();
        let Some(open) = c.as_mut() else { return Flow::Continue };
        if let Mode::DisplayLink { data, label, filter } = &mut open.mode {
            if label.focused {
                label.insert(event.c);
            } else if data.has_filter && filter.focused {
                filter.insert(event.c);
            }
        }
        Flow::Cancel
    }

    fn screen_key_pressed_pre(&self, host: &dyn Host, event: &mut ScreenKeyPressedPreEvent<'_>) -> Flow {
        if event.screen.id != "mod" || !gui::is_active(host, TAG) {
            return Flow::Continue;
        }
        let mut c = client();
        let Some(open) = c.as_mut() else { return Flow::Continue };
        if let Mode::DisplayLink { data, label, filter } = &mut open.mode {
            for (enabled, f) in [(true, label), (data.has_filter, filter)] {
                if enabled && f.focused {
                    if event.key == "key.keyboard.escape" || event.key == "key.keyboard.enter" || event.key == "key.keyboard.kp.enter" {
                        f.focus(false);
                    } else {
                        f.key(event.key);
                    }
                    return Flow::Cancel;
                }
            }
        }
        if event.key == "key.keyboard.escape" {
            open.close = true;
            return Flow::Cancel;
        }
        if event.key == "key.keyboard.enter" || event.key == "key.keyboard.kp.enter" {
            send_out(save_msg(&open.mode));
            open.close = true;
            return Flow::Cancel;
        }
        Flow::Cancel
    }
}

fn draw(host: &dyn Host, event: &mut ScreenRenderPostEvent<'_>, open: &Open) {
    gui::fill(host, event, UiRect::new(0, 0, open.size[0], open.size[1]), gui::DIM);
    let (left, top) = gui::screen_origin(open.size, PANEL_W, PANEL_H);
    gui::panel(host, event, UiRect::new(left, top, PANEL_W, PANEL_H), FRAME_BRASS, PANEL_BRASS);

    match &open.mode {
        Mode::DisplayLink { data, label, filter } => {
            let title_route = format!("{} -> {}", data.source_block, data.target_block);
            gui::header(
                host,
                event,
                UiRect::new(left + 2, top + 2, PANEL_W - 4, 22),
                Some("create:display_link"),
                "Display Link",
                None,
            );
            host.text(event.batch, &gui::truncate(host, &title_route, 320.0), [(left + 14) as f32, (top + 32) as f32], TEXT_GOLD, false);

            host.text(event.batch, "Source:", [(left + 14) as f32, (top + 63) as f32], TEXT_LIGHT, false);
            let src_name = data.sources.get(data.source_idx).map_or("None", String::as_str);
            gui::button(host, event, UiRect::new(left + 96, top + 58, 22, 18), "<", BtnStyle::Normal, data.sources.len() > 1);
            gui::fill(host, event, UiRect::new(left + 122, top + 58, 186, 18), SLOT_BG);
            host.text(event.batch, &gui::truncate(host, src_name, 178.0), [(left + 128) as f32, (top + 63) as f32], TEXT_LIGHT, false);
            gui::button(host, event, UiRect::new(left + 312, top + 58, 22, 18), ">", BtnStyle::Normal, data.sources.len() > 1);

            let opt_lbl = if data.option_label.is_empty() { "Option:" } else { &data.option_label };
            host.text(event.batch, opt_lbl, [(left + 14) as f32, (top + 89) as f32], TEXT_LIGHT, false);
            let opt_val = data.options.get(data.option_idx).map_or("-", String::as_str);
            gui::button(host, event, UiRect::new(left + 96, top + 84, 22, 18), "<", BtnStyle::Normal, data.options.len() > 1);
            gui::fill(host, event, UiRect::new(left + 122, top + 84, 186, 18), SLOT_BG);
            host.text(event.batch, &gui::truncate(host, opt_val, 178.0), [(left + 128) as f32, (top + 89) as f32], TEXT_GOLD, false);
            gui::button(host, event, UiRect::new(left + 312, top + 84, 22, 18), ">", BtnStyle::Normal, data.options.len() > 1);

            host.text(event.batch, "Label:", [(left + 14) as f32, (top + 117) as f32], TEXT_LIGHT, false);
            gui::text_field(host, event, UiRect::new(left + 96, top + 112, 238, 18), label, "Optional prefix...", true);

            if data.has_filter {
                host.text(event.batch, "Filter:", [(left + 14) as f32, (top + 141) as f32], TEXT_LIGHT, false);
                gui::text_field(host, event, UiRect::new(left + 96, top + 136, 238, 18), filter, "Station filter...", true);
            } else {
                host.text(event.batch, "Filter: (N/A)", [(left + 14) as f32, (top + 141) as f32], TEXT_MUTED, false);
            }

            host.text(event.batch, "Target Line:", [(left + 14) as f32, (top + 165) as f32], TEXT_LIGHT, false);
            let line_txt = data
                .target_lines
                .get(data.target_line)
                .map_or("Line 1", String::as_str);
            gui::button(host, event, UiRect::new(left + 96, top + 160, 22, 18), "<", BtnStyle::Normal, data.target_lines.len() > 1);
            gui::fill(host, event, UiRect::new(left + 122, top + 160, 186, 18), SLOT_BG);
            host.text(event.batch, &gui::truncate(host, line_txt, 178.0), [(left + 128) as f32, (top + 165) as f32], TEXT_LIGHT, false);
            gui::button(host, event, UiRect::new(left + 312, top + 160, 22, 18), ">", BtnStyle::Normal, data.target_lines.len() > 1);

            gui::button(host, event, UiRect::new(left + 212, top + 184, 60, 20), "Cancel", BtnStyle::Normal, true);
            gui::button(host, event, UiRect::new(left + 278, top + 184, 60, 20), "Save", BtnStyle::Primary, true);
        }
        Mode::Threshold { kind, current, min_val, max_val, on_above, off_below, inverted, .. } => {
            let unit = if *kind == 2 { "mB" } else { "items" };
            let badge = if *kind == 0 {
                "Unsupported Target".to_owned()
            } else {
                format!("{}/{} {unit}", (*current).max(0), (*max_val).max(0))
            };
            gui::header(
                host,
                event,
                UiRect::new(left + 2, top + 2, PANEL_W - 4, 22),
                Some("create:stockpile_switch"),
                "Threshold Switch",
                Some(&badge),
            );
            let bar = UiRect::new(left + 14, top + 40, 320, 22);
            gui::fill(host, event, bar, SLOT_BG);
            let span = (*max_val - *min_val).max(1) as f32;
            let fill_frac = ((*current - *min_val).max(0) as f32 / span).clamp(0.0, 1.0);
            let fill_w = (bar.w as f32 * fill_frac).round() as i32;
            if fill_w > 0 {
                gui::fill(host, event, UiRect::new(bar.x + 1, bar.y + 1, (fill_w - 2).max(1), bar.h - 2), 0xFF_3C884C);
            }
            let off_x = bar.x + (((*off_below - *min_val).max(0) as f32 / span).clamp(0.0, 1.0) * (bar.w - 2) as f32) as i32;
            let on_x = bar.x + (((*on_above - *min_val).max(0) as f32 / span).clamp(0.0, 1.0) * (bar.w - 2) as f32) as i32;
            gui::fill(host, event, UiRect::new(off_x, bar.y - 2, 2, bar.h + 4), 0xFF_E06060);
            gui::fill(host, event, UiRect::new(on_x, bar.y - 2, 2, bar.h + 4), 0xFF_F5D27A);

            let (s_lbl, b_lbl) = if *kind == 2 { ("-250", "+250") } else { ("-", "+") };
            let (ms_lbl, pb_lbl) = if *kind == 2 { ("-1k", "+1k") } else { ("-16", "+16") };

            host.text(event.batch, "ON Above:", [(left + 14) as f32, (top + 89) as f32], TEXT_GOLD, false);
            gui::button(host, event, UiRect::new(left + 114, top + 84, 34, 18), ms_lbl, BtnStyle::Normal, true);
            gui::button(host, event, UiRect::new(left + 152, top + 84, 24, 18), s_lbl, BtnStyle::Normal, true);
            host.text(event.batch, &format!("{on_above}"), [(left + 184) as f32, (top + 89) as f32], TEXT_GREEN, false);
            gui::button(host, event, UiRect::new(left + 252, top + 84, 24, 18), if *kind == 2 { "+250" } else { "+" }, BtnStyle::Normal, true);
            gui::button(host, event, UiRect::new(left + 280, top + 84, 34, 18), pb_lbl, BtnStyle::Normal, true);

            host.text(event.batch, "OFF Below:", [(left + 14) as f32, (top + 115) as f32], TEXT_RED, false);
            gui::button(host, event, UiRect::new(left + 114, top + 110, 34, 18), ms_lbl, BtnStyle::Normal, true);
            gui::button(host, event, UiRect::new(left + 152, top + 110, 24, 18), s_lbl, BtnStyle::Normal, true);
            host.text(event.batch, &format!("{off_below}"), [(left + 184) as f32, (top + 115) as f32], TEXT_RED, false);
            gui::button(host, event, UiRect::new(left + 252, top + 110, 24, 18), if *kind == 2 { "+250" } else { "+" }, BtnStyle::Normal, true);
            gui::button(host, event, UiRect::new(left + 280, top + 110, 34, 18), pb_lbl, BtnStyle::Normal, true);
            let _ = b_lbl;

            gui::button(
                host,
                event,
                UiRect::new(left + 14, top + 144, 128, 20),
                if *inverted { "Invert: ON" } else { "Invert: OFF" },
                if *inverted { BtnStyle::ToggleOn } else { BtnStyle::Normal },
                true,
            );
            gui::button(host, event, UiRect::new(left + 212, top + 182, 60, 20), "Cancel", BtnStyle::Normal, true);
            gui::button(host, event, UiRect::new(left + 278, top + 182, 60, 20), "Save", BtnStyle::Primary, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_link_and_threshold_roundtrip() {
        let dl = DisplayLinkOpenPayload {
            pos: [1, 64, 2],
            source_block: "Depot".into(),
            target_block: "Display Board".into(),
            sources: vec!["Item Name".into(), "Count".into()],
            source_idx: 1,
            option_label: "Format".into(),
            options: vec!["Short".into(), "Full".into()],
            option_idx: 0,
            label: "Iron".into(),
            has_filter: true,
            filter: "Main*".into(),
            target_line: 2,
            target_lines: vec!["Row 1".into(), "Row 2".into(), "Row 3".into()],
        };
        assert_eq!(decode_dl_open(&encode_dl_open(&dl)), Some(dl));

        let dls = DisplayLinkSavePayload {
            pos: [1, 64, 2],
            source_idx: 0,
            option_idx: 1,
            label: "Steel".into(),
            has_filter: false,
            filter: "".into(),
            target_line: 1,
        };
        assert_eq!(decode_dl_save(&encode_dl_save(&dls)), Some(dls));

        let ts = ThresholdOpenPayload {
            pos: [5, 65, 6],
            kind: 1,
            current: 96,
            min_val: 0,
            max_val: 1728,
            on_above: 128,
            off_below: 64,
            inverted: true,
        };
        assert_eq!(decode_ts_open(&encode_ts_open(&ts)), Some(ts));

        let tss = ThresholdSavePayload {
            pos: [5, 65, 6],
            on_above: 256,
            off_below: 32,
            inverted: false,
        };
        assert_eq!(decode_ts_save(&encode_ts_save(&tss)), Some(tss));
    }
}
