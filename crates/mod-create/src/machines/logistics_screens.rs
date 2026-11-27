//! Client custom GUIs for Create Logistics blocks:
//! - Stock Ticker (`create:stock_ticker`) -> `create:st_open` / `create:st_act`
//! - Redstone Requester (`create:redstone_requester`) -> `create:rr_open` / `create:rr_save`
//! - Factory Gauge (`create:factory_gauge`) -> `create:fg_open` / `create:fg_save`

use std::sync::{Mutex, PoisonError};

use create_kinetics::rules::trains::station_screen::TextField;
use crate::compat::{
    ClientTickPostEvent, Flow, Hooks, Host, NetFromServerEvent, ScreenCharTypedPreEvent,
    ScreenClosingEvent, ScreenKeyPressedPreEvent, ScreenMouseButtonPressedPreEvent,
    ScreenMouseScrolledPreEvent, ScreenRenderPostEvent, Text,
};

use super::create_gui::{
    self as gui, clean_wire, BtnStyle, UiRect, FRAME_BRASS, HEADER_BG, PANEL_BRASS, ROW_ALT,
    ROW_BG, TEXT_GOLD, TEXT_GREEN, TEXT_LIGHT, TEXT_MUTED, TEXT_RED,
};

pub const ST_OPEN_CHANNEL: &str = "create:st_open";
pub const ST_ACT_CHANNEL: &str = "create:st_act";
pub const RR_OPEN_CHANNEL: &str = "create:rr_open";
pub const RR_SAVE_CHANNEL: &str = "create:rr_save";
pub const FG_OPEN_CHANNEL: &str = "create:fg_open";
pub const FG_SAVE_CHANNEL: &str = "create:fg_save";

const TAG: &str = "logistics";
const SEP: char = '\u{1f}';
const SUB_SEP: char = '\u{1e}';
const FIELD_SEP: char = '\u{1d}';

const PANEL_W: i32 = 356;
const PANEL_H: i32 = 228;
const STOCK_COLS: usize = 6;
const STOCK_ROWS: usize = 4;
const STOCK_PAGE: usize = STOCK_COLS * STOCK_ROWS;

struct Screen;

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Screen)
}

fn clean_f(s: &str) -> String {
    clean_wire(s)
        .chars()
        .filter(|&c| c != FIELD_SEP)
        .collect()
}

// --- wire payloads ----------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StockItemWire {
    pub item_id: String,
    pub name: String,
    pub count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StockOpenPayload {
    pub pos: [i32; 3],
    pub links: usize,
    pub address: String,
    pub payments: u32,
    pub stock: Vec<StockItemWire>,
    pub order: Vec<StockItemWire>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StockAction {
    Add { item: String, count: u32 },
    Remove { item: String },
    Clear,
    Send { address: String },
    Program { address: String },
    Collect,
    Refresh,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StockActPacket {
    pub pos: [i32; 3],
    pub action: StockAction,
}

fn encode_items(items: &[StockItemWire]) -> String {
    items
        .iter()
        .map(|s| format!("{}{FIELD_SEP}{}{FIELD_SEP}{}", clean_f(&s.item_id), clean_f(&s.name), s.count))
        .collect::<Vec<_>>()
        .join(&SUB_SEP.to_string())
}

fn decode_items(raw: &str) -> Vec<StockItemWire> {
    if raw.is_empty() {
        return Vec::new();
    }
    raw.split(SUB_SEP)
        .filter(|r| !r.is_empty())
        .filter_map(|rec| {
            let mut it = rec.splitn(3, FIELD_SEP);
            Some(StockItemWire {
                item_id: it.next()?.to_owned(),
                name: it.next()?.to_owned(),
                count: it.next()?.parse().ok()?,
            })
        })
        .collect()
}

pub fn encode_stock_open(p: &StockOpenPayload) -> String {
    format!(
        "st1{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}",
        p.pos[0],
        p.pos[1],
        p.pos[2],
        p.links,
        clean_wire(&p.address),
        p.payments,
        encode_items(&p.stock),
        encode_items(&p.order),
        s = SEP,
    )
}

pub fn decode_stock_open(text: &str) -> Option<StockOpenPayload> {
    let mut it = text.split(SEP);
    if it.next()? != "st1" {
        return None;
    }
    Some(StockOpenPayload {
        pos: [it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?],
        links: it.next()?.parse().ok()?,
        address: it.next()?.to_owned(),
        payments: it.next()?.parse().ok()?,
        stock: decode_items(it.next()?),
        order: decode_items(it.next()?),
    })
}

pub fn encode_stock_act(p: &StockActPacket) -> String {
    let (kind, a, b) = match &p.action {
        StockAction::Add { item, count } => ("add", clean_wire(item), count.to_string()),
        StockAction::Remove { item } => ("rem", clean_wire(item), String::new()),
        StockAction::Clear => ("clr", String::new(), String::new()),
        StockAction::Send { address } => ("snd", clean_wire(address), String::new()),
        StockAction::Program { address } => ("prg", clean_wire(address), String::new()),
        StockAction::Collect => ("col", String::new(), String::new()),
        StockAction::Refresh => ("ref", String::new(), String::new()),
    };
    format!(
        "sta1{s}{}{s}{}{s}{}{s}{kind}{s}{a}{s}{b}",
        p.pos[0],
        p.pos[1],
        p.pos[2],
        s = SEP,
    )
}

pub fn decode_stock_act(text: &str) -> Option<StockActPacket> {
    let mut it = text.split(SEP);
    if it.next()? != "sta1" {
        return None;
    }
    let pos = [it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?];
    let kind = it.next()?;
    let a = it.next().unwrap_or("");
    let b = it.next().unwrap_or("");
    let action = match kind {
        "add" => StockAction::Add { item: a.to_owned(), count: b.parse().ok()? },
        "rem" => StockAction::Remove { item: a.to_owned() },
        "clr" => StockAction::Clear,
        "snd" => StockAction::Send { address: a.to_owned() },
        "prg" => StockAction::Program { address: a.to_owned() },
        "col" => StockAction::Collect,
        "ref" => StockAction::Refresh,
        _ => return None,
    };
    Some(StockActPacket { pos, action })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequesterOpenPayload {
    pub pos: [i32; 3],
    pub allow_partial: bool,
    pub address: String,
    pub entries: Vec<StockItemWire>,
    pub held_item: Option<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequesterSavePayload {
    pub pos: [i32; 3],
    pub allow_partial: bool,
    pub address: String,
    pub entries: Vec<(String, u32)>,
}

pub fn encode_requester_open(p: &RequesterOpenPayload) -> String {
    let held = p
        .held_item
        .as_ref()
        .map(|(id, nm)| format!("{}{FIELD_SEP}{}", clean_f(id), clean_f(nm)))
        .unwrap_or_default();
    format!(
        "rr1{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{held}",
        p.pos[0],
        p.pos[1],
        p.pos[2],
        u8::from(p.allow_partial),
        clean_wire(&p.address),
        encode_items(&p.entries),
        s = SEP,
    )
}

pub fn decode_requester_open(text: &str) -> Option<RequesterOpenPayload> {
    let mut it = text.split(SEP);
    if it.next()? != "rr1" {
        return None;
    }
    let pos = [it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?];
    let allow_partial = it.next()? == "1";
    let address = it.next()?.to_owned();
    let entries = decode_items(it.next()?);
    let held_raw = it.next().unwrap_or("");
    let held_item = if held_raw.is_empty() {
        None
    } else {
        let mut p = held_raw.splitn(2, FIELD_SEP);
        Some((p.next()?.to_owned(), p.next().unwrap_or("").to_owned()))
    };
    Some(RequesterOpenPayload { pos, allow_partial, address, entries, held_item })
}

pub fn encode_requester_save(p: &RequesterSavePayload) -> String {
    let entries = p
        .entries
        .iter()
        .map(|(id, c)| format!("{}{FIELD_SEP}{c}", clean_f(id)))
        .collect::<Vec<_>>()
        .join(&SUB_SEP.to_string());
    format!(
        "rrs1{s}{}{s}{}{s}{}{s}{}{s}{}{s}{entries}",
        p.pos[0],
        p.pos[1],
        p.pos[2],
        u8::from(p.allow_partial),
        clean_wire(&p.address),
        s = SEP,
    )
}

pub fn decode_requester_save(text: &str) -> Option<RequesterSavePayload> {
    let mut it = text.split(SEP);
    if it.next()? != "rrs1" {
        return None;
    }
    let pos = [it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?];
    let allow_partial = it.next()? == "1";
    let address = it.next()?.to_owned();
    let raw = it.next()?;
    let entries = if raw.is_empty() {
        Vec::new()
    } else {
        raw.split(SUB_SEP)
            .filter(|r| !r.is_empty())
            .filter_map(|rec| {
                let mut p = rec.splitn(2, FIELD_SEP);
                Some((p.next()?.to_owned(), p.next()?.parse().ok()?))
            })
            .collect()
    };
    Some(RequesterSavePayload { pos, allow_partial, address, entries })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GaugeOpenPayload {
    pub pos: [i32; 3],
    pub slot: u8,
    pub restocker: bool,
    pub item: String,
    pub item_name: String,
    pub target: i32,
    pub use_stacks: bool,
    pub address: String,
    pub output_amount: i32,
    pub promise_interval: i32,
    pub in_stock: i32,
    pub promised: i32,
    pub satisfied: bool,
    pub held_item: Option<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GaugeSavePayload {
    pub pos: [i32; 3],
    pub slot: u8,
    pub clear_promises: bool,
    pub item_override: Option<String>,
    pub target: i32,
    pub use_stacks: bool,
    pub address: String,
    pub output_amount: i32,
    pub promise_interval: i32,
}

pub fn encode_gauge_open(p: &GaugeOpenPayload) -> String {
    let held = p
        .held_item
        .as_ref()
        .map(|(id, nm)| format!("{}{FIELD_SEP}{}", clean_f(id), clean_f(nm)))
        .unwrap_or_default();
    format!(
        "fg1{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{held}",
        p.pos[0],
        p.pos[1],
        p.pos[2],
        p.slot,
        u8::from(p.restocker),
        clean_wire(&p.item),
        clean_wire(&p.item_name),
        p.target,
        u8::from(p.use_stacks),
        clean_wire(&p.address),
        p.output_amount,
        p.promise_interval,
        p.in_stock,
        p.promised,
        u8::from(p.satisfied),
        s = SEP,
    )
}

pub fn decode_gauge_open(text: &str) -> Option<GaugeOpenPayload> {
    let mut it = text.split(SEP);
    if it.next()? != "fg1" {
        return None;
    }
    let pos = [it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?];
    let slot = it.next()?.parse().ok()?;
    let restocker = it.next()? == "1";
    let item = it.next()?.to_owned();
    let item_name = it.next()?.to_owned();
    let target = it.next()?.parse().ok()?;
    let use_stacks = it.next()? == "1";
    let address = it.next()?.to_owned();
    let output_amount = it.next()?.parse().ok()?;
    let promise_interval = it.next()?.parse().ok()?;
    let in_stock = it.next()?.parse().ok()?;
    let promised = it.next()?.parse().ok()?;
    let satisfied = it.next()? == "1";
    let held_raw = it.next().unwrap_or("");
    let held_item = if held_raw.is_empty() {
        None
    } else {
        let mut p = held_raw.splitn(2, FIELD_SEP);
        Some((p.next()?.to_owned(), p.next().unwrap_or("").to_owned()))
    };
    Some(GaugeOpenPayload {
        pos,
        slot,
        restocker,
        item,
        item_name,
        target,
        use_stacks,
        address,
        output_amount,
        promise_interval,
        in_stock,
        promised,
        satisfied,
        held_item,
    })
}

pub fn encode_gauge_save(p: &GaugeSavePayload) -> String {
    let item_flag = match &p.item_override {
        None => String::new(),
        Some(id) => format!("set:{}", clean_wire(id)),
    };
    format!(
        "fgs1{s}{}{s}{}{s}{}{s}{}{s}{}{s}{item_flag}{s}{}{s}{}{s}{}{s}{}{s}{}",
        p.pos[0],
        p.pos[1],
        p.pos[2],
        p.slot,
        u8::from(p.clear_promises),
        p.target,
        u8::from(p.use_stacks),
        clean_wire(&p.address),
        p.output_amount,
        p.promise_interval,
        s = SEP,
    )
}

pub fn decode_gauge_save(text: &str) -> Option<GaugeSavePayload> {
    let mut it = text.split(SEP);
    if it.next()? != "fgs1" {
        return None;
    }
    let pos = [it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?];
    let slot = it.next()?.parse().ok()?;
    let clear_promises = it.next()? == "1";
    let item_raw = it.next()?;
    let item_override = item_raw.strip_prefix("set:").map(str::to_owned);
    let target = it.next()?.parse().ok()?;
    let use_stacks = it.next()? == "1";
    let address = it.next()?.to_owned();
    let output_amount = it.next()?.parse().ok()?;
    let promise_interval = it.next()?.parse().ok()?;
    Some(GaugeSavePayload {
        pos,
        slot,
        clear_promises,
        item_override,
        target,
        use_stacks,
        address,
        output_amount,
        promise_interval,
    })
}

// --- client state -----------------------------------------------------------------------------------------------

enum Mode {
    StockTicker {
        data: StockOpenPayload,
        page: usize,
        search: TextField,
        address: TextField,
    },
    Requester {
        pos: [i32; 3],
        allow_partial: bool,
        address: TextField,
        entries: Vec<StockItemWire>,
        held_item: Option<(String, String)>,
    },
    Gauge {
        pos: [i32; 3],
        slot: u8,
        restocker: bool,
        item: String,
        item_name: String,
        target: i32,
        use_stacks: bool,
        address: TextField,
        output_amount: i32,
        promise_interval: i32,
        in_stock: i32,
        promised: i32,
        satisfied: bool,
        held_item: Option<(String, String)>,
        item_override: Option<String>,
    },
}

struct Open {
    mode: Mode,
    size: [i32; 2],
    close: bool,
}

enum OutMsg {
    Stock(StockActPacket),
    Requester(RequesterSavePayload),
    Gauge(GaugeSavePayload),
}

static CLIENT: Mutex<Option<Open>> = Mutex::new(None);
static OUTBOX: Mutex<Vec<OutMsg>> = Mutex::new(Vec::new());

fn client() -> std::sync::MutexGuard<'static, Option<Open>> {
    CLIENT.lock().unwrap_or_else(PoisonError::into_inner)
}

fn send_out(msg: OutMsg) {
    OUTBOX.lock().unwrap_or_else(PoisonError::into_inner).push(msg);
}

fn filtered_stock<'a>(stock: &'a [StockItemWire], query: &str) -> Vec<&'a StockItemWire> {
    let q = query.trim().to_ascii_lowercase();
    stock
        .iter()
        .filter(|s| q.is_empty() || s.name.to_ascii_lowercase().contains(&q) || s.item_id.to_ascii_lowercase().contains(&q))
        .collect()
}

fn active_fields_mut(mode: &mut Mode) -> Vec<&mut TextField> {
    match mode {
        Mode::StockTicker { search, address, .. } => vec![search, address],
        Mode::Requester { address, .. } | Mode::Gauge { address, .. } => vec![address],
    }
}

fn save_payload_from_mode(mode: &Mode, clear_promises: bool) -> OutMsg {
    match mode {
        Mode::StockTicker { data, address, .. } => OutMsg::Stock(StockActPacket {
            pos: data.pos,
            action: StockAction::Send { address: address.value() },
        }),
        Mode::Requester { pos, allow_partial, address, entries, .. } => OutMsg::Requester(RequesterSavePayload {
            pos: *pos,
            allow_partial: *allow_partial,
            address: address.value(),
            entries: entries.iter().map(|e| (e.item_id.clone(), e.count)).collect(),
        }),
        Mode::Gauge {
            pos,
            slot,
            target,
            use_stacks,
            address,
            output_amount,
            promise_interval,
            item_override,
            ..
        } => OutMsg::Gauge(GaugeSavePayload {
            pos: *pos,
            slot: *slot,
            clear_promises,
            item_override: item_override.clone(),
            target: *target,
            use_stacks: *use_stacks,
            address: address.value(),
            output_amount: *output_amount,
            promise_interval: *promise_interval,
        }),
    }
}

// --- hooks ------------------------------------------------------------------------------------------------------

impl Hooks for Screen {
    fn net_from_server(&self, host: &mut dyn Host, event: &mut NetFromServerEvent<'_>) {
        let Ok(text) = std::str::from_utf8(event.data) else { return };
        match event.channel {
            ST_OPEN_CHANNEL => {
                let Some(p) = decode_stock_open(text) else { return };
                let search = TextField::new(40);
                let mut address = TextField::new(64);
                address.set(&p.address);
                if gui::open(host, TAG) {
                    *client() = Some(Open {
                        mode: Mode::StockTicker { data: p, page: 0, search, address },
                        size: [0, 0],
                        close: false,
                    });
                }
            }
            RR_OPEN_CHANNEL => {
                let Some(p) = decode_requester_open(text) else { return };
                let mut address = TextField::new(64);
                address.set(&p.address);
                if gui::open(host, TAG) {
                    *client() = Some(Open {
                        mode: Mode::Requester {
                            pos: p.pos,
                            allow_partial: p.allow_partial,
                            address,
                            entries: p.entries,
                            held_item: p.held_item,
                        },
                        size: [0, 0],
                        close: false,
                    });
                }
            }
            FG_OPEN_CHANNEL => {
                let Some(p) = decode_gauge_open(text) else { return };
                let mut address = TextField::new(64);
                address.set(&p.address);
                if gui::open(host, TAG) {
                    *client() = Some(Open {
                        mode: Mode::Gauge {
                            pos: p.pos,
                            slot: p.slot,
                            restocker: p.restocker,
                            item: p.item,
                            item_name: p.item_name,
                            target: p.target,
                            use_stacks: p.use_stacks,
                            address,
                            output_amount: p.output_amount,
                            promise_interval: p.promise_interval,
                            in_stock: p.in_stock,
                            promised: p.promised,
                            satisfied: p.satisfied,
                            held_item: p.held_item,
                            item_override: None,
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
                OutMsg::Stock(p) => {
                    let _ = host.net_send_to_server(ST_ACT_CHANNEL, encode_stock_act(&p).as_bytes());
                }
                OutMsg::Requester(p) => {
                    let _ = host.net_send_to_server(RR_SAVE_CHANNEL, encode_requester_save(&p).as_bytes());
                }
                OutMsg::Gauge(p) => {
                    let _ = host.net_send_to_server(FG_SAVE_CHANNEL, encode_gauge_save(&p).as_bytes());
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
        if event.button != 0 && event.button != 1 {
            return Flow::Cancel;
        }
        let mut c = client();
        let Some(open) = c.as_mut() else { return Flow::Continue };
        let (left, top) = gui::screen_origin(open.size, PANEL_W, PANEL_H);
        let mouse = event.screen.mouse;
        let right = event.button == 1;

        match &mut open.mode {
            Mode::StockTicker { data, page, search, address } => {
                let search_rect = UiRect::new(left + 10, top + 30, 148, 18);
                let prev_rect = UiRect::new(left + 162, top + 30, 22, 18);
                let next_rect = UiRect::new(left + 188, top + 30, 22, 18);
                let addr_rect = UiRect::new(left + 216, top + 158, 130, 18);
                if search_rect.contains(mouse) {
                    search.focus(true);
                    address.focus(false);
                    return Flow::Cancel;
                }
                if addr_rect.contains(mouse) {
                    search.focus(false);
                    address.focus(true);
                    return Flow::Cancel;
                }
                search.focus(false);
                address.focus(false);

                let filtered = filtered_stock(&data.stock, &search.value());
                let pages = filtered.len().max(1).div_ceil(STOCK_PAGE);
                *page = (*page).min(pages - 1);
                if prev_rect.contains(mouse) {
                    *page = page.saturating_sub(1);
                    return Flow::Cancel;
                }
                if next_rect.contains(mouse) {
                    *page = (*page + 1).min(pages - 1);
                    return Flow::Cancel;
                }
                let start = *page * STOCK_PAGE;
                for i in 0..STOCK_PAGE {
                    let Some(item) = filtered.get(start + i) else { break };
                    let r = UiRect::new(
                        left + 10 + (i % STOCK_COLS) as i32 * 32,
                        top + 54 + (i / STOCK_COLS) as i32 * 32,
                        28,
                        28,
                    );
                    if r.contains(mouse) {
                        let step = if right || host.has_shift_down() { 64 } else { 1 };
                        send_out(OutMsg::Stock(StockActPacket {
                            pos: data.pos,
                            action: StockAction::Add { item: item.item_id.clone(), count: step },
                        }));
                        return Flow::Cancel;
                    }
                }
                for (i, ord) in data.order.iter().take(9).enumerate() {
                    let rem_r = UiRect::new(left + 324, top + 46 + i as i32 * 12, 22, 11);
                    if rem_r.contains(mouse) {
                        send_out(OutMsg::Stock(StockActPacket {
                            pos: data.pos,
                            action: StockAction::Remove { item: ord.item_id.clone() },
                        }));
                        return Flow::Cancel;
                    }
                }
                let send_btn = UiRect::new(left + 216, top + 180, 62, 18);
                let prog_btn = UiRect::new(left + 284, top + 180, 62, 18);
                let col_btn = UiRect::new(left + 10, top + 196, 86, 22);
                let clr_btn = UiRect::new(left + 102, top + 196, 56, 22);
                let ref_btn = UiRect::new(left + 164, top + 196, 62, 22);
                let cls_btn = UiRect::new(left + 284, top + 200, 62, 20);
                if send_btn.contains(mouse) {
                    send_out(OutMsg::Stock(StockActPacket {
                        pos: data.pos,
                        action: StockAction::Send { address: address.value() },
                    }));
                    return Flow::Cancel;
                }
                if prog_btn.contains(mouse) {
                    send_out(OutMsg::Stock(StockActPacket {
                        pos: data.pos,
                        action: StockAction::Program { address: address.value() },
                    }));
                    return Flow::Cancel;
                }
                if data.payments > 0 && col_btn.contains(mouse) {
                    send_out(OutMsg::Stock(StockActPacket {
                        pos: data.pos,
                        action: StockAction::Collect,
                    }));
                    return Flow::Cancel;
                }
                if clr_btn.contains(mouse) {
                    send_out(OutMsg::Stock(StockActPacket {
                        pos: data.pos,
                        action: StockAction::Clear,
                    }));
                    return Flow::Cancel;
                }
                if ref_btn.contains(mouse) {
                    send_out(OutMsg::Stock(StockActPacket {
                        pos: data.pos,
                        action: StockAction::Refresh,
                    }));
                    return Flow::Cancel;
                }
                if cls_btn.contains(mouse) {
                    open.close = true;
                }
            }
            Mode::Requester { pos, allow_partial, address, entries, held_item } => {
                let addr_r = UiRect::new(left + 72, top + 170, 146, 18);
                if addr_r.contains(mouse) {
                    address.focus(true);
                    return Flow::Cancel;
                }
                address.focus(false);
                let mut remove_at = None;
                for (i, entry) in entries.iter_mut().take(9).enumerate() {
                    let y = top + 34 + i as i32 * 14;
                    let m16 = UiRect::new(left + 196, y, 24, 13);
                    let m1 = UiRect::new(left + 222, y, 18, 13);
                    let p1 = UiRect::new(left + 276, y, 18, 13);
                    let p16 = UiRect::new(left + 296, y, 24, 13);
                    let del = UiRect::new(left + 324, y, 20, 13);
                    if m16.contains(mouse) {
                        entry.count = entry.count.saturating_sub(16).max(1);
                        return Flow::Cancel;
                    }
                    if m1.contains(mouse) {
                        entry.count = entry.count.saturating_sub(1).max(1);
                        return Flow::Cancel;
                    }
                    if p1.contains(mouse) {
                        entry.count = (entry.count + 1).min(256);
                        return Flow::Cancel;
                    }
                    if p16.contains(mouse) {
                        entry.count = (entry.count + 16).min(256);
                        return Flow::Cancel;
                    }
                    if del.contains(mouse) {
                        remove_at = Some(i);
                        break;
                    }
                }
                if let Some(i) = remove_at {
                    entries.remove(i);
                    return Flow::Cancel;
                }
                let add_held = UiRect::new(left + 10, top + 198, 92, 20);
                let partial_btn = UiRect::new(left + 224, top + 170, 122, 18);
                let cancel_btn = UiRect::new(left + 220, top + 198, 60, 20);
                let save_btn = UiRect::new(left + 286, top + 198, 60, 20);
                if add_held.contains(mouse)
                    && entries.len() < 9
                    && let Some((id, nm)) = held_item.clone()
                {
                    if let Some(ex) = entries.iter_mut().find(|e| e.item_id == id) {
                        ex.count = (ex.count + 1).min(256);
                    } else {
                        entries.push(StockItemWire { item_id: id, name: nm, count: 1 });
                    }
                    return Flow::Cancel;
                }
                if partial_btn.contains(mouse) {
                    *allow_partial = !*allow_partial;
                    return Flow::Cancel;
                }
                if cancel_btn.contains(mouse) {
                    open.close = true;
                    return Flow::Cancel;
                }
                if save_btn.contains(mouse) {
                    let msg = RequesterSavePayload {
                        pos: *pos,
                        allow_partial: *allow_partial,
                        address: address.value(),
                        entries: entries.iter().map(|e| (e.item_id.clone(), e.count)).collect(),
                    };
                    send_out(OutMsg::Requester(msg));
                    open.close = true;
                }
            }
            Mode::Gauge {
                target,
                use_stacks,
                address,
                output_amount,
                promise_interval,
                held_item,
                item,
                item_name,
                item_override,
                promised,
                ..
            } => {
                let addr_r = UiRect::new(left + 84, top + 148, 258, 18);
                if addr_r.contains(mouse) {
                    address.focus(true);
                    return Flow::Cancel;
                }
                address.focus(false);
                let use_held = UiRect::new(left + 72, top + 40, 88, 18);
                let clr_item = UiRect::new(left + 166, top + 40, 82, 18);
                let stack_tog = UiRect::new(left + 254, top + 40, 90, 18);
                let m16 = UiRect::new(left + 84, top + 72, 34, 18);
                let m1 = UiRect::new(left + 122, top + 72, 26, 18);
                let p1 = UiRect::new(left + 196, top + 72, 26, 18);
                let p16 = UiRect::new(left + 226, top + 72, 34, 18);
                let out_m = UiRect::new(left + 84, top + 98, 24, 18);
                let out_p = UiRect::new(left + 146, top + 98, 24, 18);
                let prom_m = UiRect::new(left + 258, top + 98, 24, 18);
                let prom_p = UiRect::new(left + 318, top + 98, 24, 18);
                let clr_prom = UiRect::new(left + 10, top + 198, 108, 20);
                let cancel = UiRect::new(left + 220, top + 198, 60, 20);
                let save = UiRect::new(left + 286, top + 198, 60, 20);

                if use_held.contains(mouse)
                    && let Some((id, nm)) = held_item.clone()
                {
                    *item = id.clone();
                    *item_name = nm;
                    *item_override = Some(id);
                    return Flow::Cancel;
                }
                if clr_item.contains(mouse) {
                    item.clear();
                    item_name.clear();
                    *item_override = Some(String::new());
                    return Flow::Cancel;
                }
                if stack_tog.contains(mouse) {
                    *use_stacks = !*use_stacks;
                    return Flow::Cancel;
                }
                if m16.contains(mouse) {
                    *target = (*target - 16).max(1);
                    return Flow::Cancel;
                }
                if m1.contains(mouse) {
                    *target = (*target - 1).max(1);
                    return Flow::Cancel;
                }
                if p1.contains(mouse) {
                    *target = (*target + 1).min(64 * 64);
                    return Flow::Cancel;
                }
                if p16.contains(mouse) {
                    *target = (*target + 16).min(64 * 64);
                    return Flow::Cancel;
                }
                if out_m.contains(mouse) {
                    *output_amount = (*output_amount - 1).max(1);
                    return Flow::Cancel;
                }
                if out_p.contains(mouse) {
                    *output_amount = (*output_amount + 1).min(64);
                    return Flow::Cancel;
                }
                if prom_m.contains(mouse) {
                    *promise_interval = if *promise_interval <= 0 { -1 } else if *promise_interval <= 5 { -1 } else { *promise_interval - 5 };
                    return Flow::Cancel;
                }
                if prom_p.contains(mouse) {
                    *promise_interval = if *promise_interval < 0 { 5 } else { (*promise_interval + 5).min(600) };
                    return Flow::Cancel;
                }
                if clr_prom.contains(mouse) && *promised > 0 {
                    *promised = 0;
                    send_out(save_payload_from_mode(&open.mode, true));
                    return Flow::Cancel;
                }
                if cancel.contains(mouse) {
                    open.close = true;
                    return Flow::Cancel;
                }
                if save.contains(mouse) {
                    send_out(save_payload_from_mode(&open.mode, false));
                    open.close = true;
                }
            }
        }
        Flow::Cancel
    }

    fn screen_mouse_scrolled_pre(&self, host: &dyn Host, event: &mut ScreenMouseScrolledPreEvent<'_>) -> Flow {
        if event.screen.id != "mod" || !gui::is_active(host, TAG) {
            return Flow::Continue;
        }
        let mut c = client();
        let Some(open) = c.as_mut() else { return Flow::Continue };
        if let Mode::StockTicker { data, page, search, .. } = &mut open.mode {
            let pages = filtered_stock(&data.stock, &search.value()).len().max(1).div_ceil(STOCK_PAGE);
            if event.scroll[1] > 0.0 {
                *page = page.saturating_sub(1);
            } else if event.scroll[1] < 0.0 {
                *page = (*page + 1).min(pages - 1);
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
        for f in active_fields_mut(&mut open.mode) {
            if f.focused {
                f.insert(event.c);
                return Flow::Cancel;
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
        for f in active_fields_mut(&mut open.mode) {
            if f.focused {
                if event.key == "key.keyboard.escape" || event.key == "key.keyboard.enter" || event.key == "key.keyboard.kp.enter" {
                    f.focus(false);
                } else {
                    f.key(event.key);
                }
                return Flow::Cancel;
            }
        }
        if event.key == "key.keyboard.escape" {
            open.close = true;
            return Flow::Cancel;
        }
        if event.key == "key.keyboard.enter" || event.key == "key.keyboard.kp.enter" {
            let msg = save_payload_from_mode(&open.mode, false);
            send_out(msg);
            if !matches!(open.mode, Mode::StockTicker { .. }) {
                open.close = true;
            }
            return Flow::Cancel;
        }
        Flow::Cancel
    }
}

fn draw(host: &dyn Host, event: &mut ScreenRenderPostEvent<'_>, open: &Open) {
    gui::fill(host, event, UiRect::new(0, 0, open.size[0], open.size[1]), gui::DIM);
    let (left, top) = gui::screen_origin(open.size, PANEL_W, PANEL_H);
    gui::panel(host, event, UiRect::new(left, top, PANEL_W, PANEL_H), FRAME_BRASS, PANEL_BRASS);

    let mut tip: Option<Vec<String>> = None;
    match &open.mode {
        Mode::StockTicker { data, page, search, address } => {
            let badge = format!("{} items | {} link(s)", data.stock.len(), data.links);
            gui::header(
                host,
                event,
                UiRect::new(left + 2, top + 2, PANEL_W - 4, 22),
                Some("create:stock_ticker"),
                "Stock Keeper",
                Some(&badge),
            );
            gui::text_field(host, event, UiRect::new(left + 10, top + 30, 148, 18), search, "Filter items...", true);
            let filtered = filtered_stock(&data.stock, &search.value());
            let pages = filtered.len().max(1).div_ceil(STOCK_PAGE);
            let page = (*page).min(pages - 1);
            gui::button(host, event, UiRect::new(left + 162, top + 30, 22, 18), "<", BtnStyle::Normal, page > 0);
            gui::button(host, event, UiRect::new(left + 188, top + 30, 22, 18), ">", BtnStyle::Normal, page + 1 < pages);

            gui::fill(host, event, UiRect::new(left + 8, top + 51, 198, 134), HEADER_BG);
            let start = page * STOCK_PAGE;
            for i in 0..STOCK_PAGE {
                let r = UiRect::new(
                    left + 10 + (i % STOCK_COLS) as i32 * 32,
                    top + 54 + (i / STOCK_COLS) as i32 * 32,
                    28,
                    28,
                );
                if let Some(item) = filtered.get(start + i) {
                    if gui::item_slot(host, event, r, Some(&item.item_id), false) {
                        tip = Some(vec![
                            format!("{} x{}", item.name, item.count),
                            "Click: +1 | Right/Shift: +64".to_owned(),
                        ]);
                    }
                    let cnt = if item.count >= 1000 {
                        format!("{}k", item.count / 1000)
                    } else {
                        item.count.to_string()
                    };
                    host.text(event.batch, &cnt, [(r.x + 2) as f32, (r.y + 18) as f32], TEXT_GOLD, true);
                } else {
                    gui::item_slot(host, event, r, None, false);
                }
            }

            host.text(event.batch, "Request Order (9 max)", [(left + 216) as f32, (top + 32) as f32], TEXT_GOLD, false);
            for i in 0..9 {
                let y = top + 46 + i as i32 * 12;
                let bg = if i % 2 == 0 { ROW_BG } else { ROW_ALT };
                gui::fill(host, event, UiRect::new(left + 216, y, 130, 11), bg);
                if let Some(ord) = data.order.get(i) {
                    let lbl = gui::truncate(host, &format!("{} x{}", ord.name, ord.count), 102.0);
                    host.text(event.batch, &lbl, [(left + 218) as f32, (y + 2) as f32], TEXT_LIGHT, false);
                    gui::button(host, event, UiRect::new(left + 324, y, 22, 11), "-1", BtnStyle::Danger, true);
                }
            }
            gui::text_field(host, event, UiRect::new(left + 216, top + 158, 130, 18), address, "Address...", true);
            gui::button(host, event, UiRect::new(left + 216, top + 180, 62, 18), "Send", BtnStyle::Primary, !data.order.is_empty());
            gui::button(host, event, UiRect::new(left + 284, top + 180, 62, 18), "Program", BtnStyle::ToggleOn, !data.order.is_empty());

            let col_lbl = format!("Collect ({})", data.payments);
            gui::button(host, event, UiRect::new(left + 10, top + 196, 86, 22), &col_lbl, BtnStyle::ToggleOn, data.payments > 0);
            gui::button(host, event, UiRect::new(left + 102, top + 196, 56, 22), "Clear", BtnStyle::Danger, !data.order.is_empty());
            gui::button(host, event, UiRect::new(left + 164, top + 196, 62, 22), "Refresh", BtnStyle::Normal, true);
            gui::button(host, event, UiRect::new(left + 284, top + 200, 62, 20), "Close", BtnStyle::Normal, true);
        }
        Mode::Requester { allow_partial, address, entries, held_item, .. } => {
            gui::header(
                host,
                event,
                UiRect::new(left + 2, top + 2, PANEL_W - 4, 22),
                Some("create:redstone_requester"),
                "Redstone Requester",
                Some(&format!("{}/9 slots", entries.len())),
            );
            for (i, entry) in entries.iter().take(9).enumerate() {
                let y = top + 34 + i as i32 * 14;
                gui::fill(host, event, UiRect::new(left + 10, y, 336, 13), if i % 2 == 0 { ROW_BG } else { ROW_ALT });
                host.item(event.batch, &entry.item_id, [(left + 12) as f32, (y - 1) as f32], 14.0);
                let nm = gui::truncate(host, &entry.name, 160.0);
                host.text(event.batch, &nm, [(left + 30) as f32, (y + 3) as f32], TEXT_LIGHT, false);
                gui::button(host, event, UiRect::new(left + 196, y, 24, 13), "-16", BtnStyle::Normal, true);
                gui::button(host, event, UiRect::new(left + 222, y, 18, 13), "-", BtnStyle::Normal, true);
                host.text(event.batch, &entry.count.to_string(), [(left + 246) as f32, (y + 3) as f32], TEXT_GOLD, false);
                gui::button(host, event, UiRect::new(left + 276, y, 18, 13), "+", BtnStyle::Normal, true);
                gui::button(host, event, UiRect::new(left + 296, y, 24, 13), "+16", BtnStyle::Normal, true);
                gui::button(host, event, UiRect::new(left + 324, y, 20, 13), "X", BtnStyle::Danger, true);
            }
            if entries.is_empty() {
                host.text(event.batch, "Hold an item & click + Add Held, or use /requester", [(left + 20) as f32, (top + 88) as f32], TEXT_MUTED, false);
            }
            host.text(event.batch, "Address:", [(left + 12) as f32, (top + 175) as f32], TEXT_LIGHT, false);
            gui::text_field(host, event, UiRect::new(left + 72, top + 170, 146, 18), address, "Package address...", true);
            gui::button(
                host,
                event,
                UiRect::new(left + 224, top + 170, 122, 18),
                if *allow_partial { "Partial: ON" } else { "Partial: OFF" },
                if *allow_partial { BtnStyle::ToggleOn } else { BtnStyle::Normal },
                true,
            );
            gui::button(host, event, UiRect::new(left + 10, top + 198, 92, 20), "+ Add Held", BtnStyle::Normal, held_item.is_some() && entries.len() < 9);
            gui::button(host, event, UiRect::new(left + 220, top + 198, 60, 20), "Cancel", BtnStyle::Normal, true);
            gui::button(host, event, UiRect::new(left + 286, top + 198, 60, 20), "Save", BtnStyle::Primary, true);
        }
        Mode::Gauge {
            restocker,
            item,
            item_name,
            target,
            use_stacks,
            address,
            output_amount,
            promise_interval,
            in_stock,
            promised,
            satisfied,
            held_item,
            ..
        } => {
            let mode_badge = if *restocker { "Restocker" } else { "Factory Gauge" };
            gui::header(
                host,
                event,
                UiRect::new(left + 2, top + 2, PANEL_W - 4, 22),
                Some("create:factory_gauge"),
                "Factory Gauge",
                Some(mode_badge),
            );
            gui::item_slot(host, event, UiRect::new(left + 14, top + 34, 48, 48), (!item.is_empty()).then_some(item.as_str()), *satisfied);
            let title = if item_name.is_empty() { "No filter item" } else { item_name.as_str() };
            host.text(event.batch, &gui::truncate(host, title, 180.0), [(left + 72) as f32, (top + 28) as f32], TEXT_GOLD, false);
            gui::button(host, event, UiRect::new(left + 72, top + 40, 88, 18), "Use Held", BtnStyle::Normal, held_item.is_some());
            gui::button(host, event, UiRect::new(left + 166, top + 40, 82, 18), "Clear Item", BtnStyle::Danger, !item.is_empty());
            gui::button(
                host,
                event,
                UiRect::new(left + 254, top + 40, 90, 18),
                if *use_stacks { "Unit: Stacks" } else { "Unit: Items" },
                BtnStyle::ToggleOn,
                true,
            );

            host.text(event.batch, "Target:", [(left + 14) as f32, (top + 88) as f32], TEXT_LIGHT, false);
            gui::button(host, event, UiRect::new(left + 84, top + 72, 34, 18), "-16", BtnStyle::Normal, true);
            gui::button(host, event, UiRect::new(left + 122, top + 72, 26, 18), "-", BtnStyle::Normal, true);
            host.text(event.batch, &target.to_string(), [(left + 154) as f32, (top + 77) as f32], TEXT_GOLD, false);
            gui::button(host, event, UiRect::new(left + 196, top + 72, 26, 18), "+", BtnStyle::Normal, true);
            gui::button(host, event, UiRect::new(left + 226, top + 72, 34, 18), "+16", BtnStyle::Normal, true);

            host.text(event.batch, "Output:", [(left + 14) as f32, (top + 103) as f32], TEXT_LIGHT, false);
            gui::button(host, event, UiRect::new(left + 84, top + 98, 24, 18), "-", BtnStyle::Normal, true);
            host.text(event.batch, &output_amount.to_string(), [(left + 114) as f32, (top + 103) as f32], TEXT_LIGHT, false);
            gui::button(host, event, UiRect::new(left + 146, top + 98, 24, 18), "+", BtnStyle::Normal, true);

            host.text(event.batch, "Promise(s):", [(left + 182) as f32, (top + 103) as f32], TEXT_LIGHT, false);
            gui::button(host, event, UiRect::new(left + 258, top + 98, 24, 18), "-", BtnStyle::Normal, true);
            let ptxt = if *promise_interval < 0 { "Off".to_owned() } else { format!("{promise_interval}s") };
            host.text(event.batch, &ptxt, [(left + 286) as f32, (top + 103) as f32], TEXT_LIGHT, false);
            gui::button(host, event, UiRect::new(left + 318, top + 98, 24, 18), "+", BtnStyle::Normal, true);

            let stat_col = if *satisfied { TEXT_GREEN } else { TEXT_RED };
            let status_txt = format!("Stock: {in_stock} | Promised: {promised}");
            host.text(event.batch, &status_txt, [(left + 14) as f32, (top + 126) as f32], stat_col, false);

            host.text(event.batch, "Address:", [(left + 14) as f32, (top + 153) as f32], TEXT_LIGHT, false);
            gui::text_field(host, event, UiRect::new(left + 84, top + 148, 258, 18), address, "Routing address...", true);

            gui::button(host, event, UiRect::new(left + 10, top + 198, 108, 20), "Clear Promises", BtnStyle::Danger, *promised > 0);
            gui::button(host, event, UiRect::new(left + 220, top + 198, 60, 20), "Cancel", BtnStyle::Normal, true);
            gui::button(host, event, UiRect::new(left + 286, top + 198, 60, 20), "Save", BtnStyle::Primary, true);
        }
    }

    if let Some(lines) = tip {
        let lines: Vec<Text> = lines.into_iter().map(Text::literal).collect();
        host.gui_tooltip(event.batch, &lines, event.screen.mouse);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_open_and_action_roundtrip() {
        let p = StockOpenPayload {
            pos: [10, 64, -5],
            links: 3,
            address: "Depot-1".into(),
            payments: 2,
            stock: vec![StockItemWire { item_id: "minecraft:iron_ingot".into(), name: "Iron Ingot".into(), count: 128 }],
            order: vec![StockItemWire { item_id: "create:brass_ingot".into(), name: "Brass Ingot".into(), count: 16 }],
        };
        assert_eq!(decode_stock_open(&encode_stock_open(&p)), Some(p));

        let act = StockActPacket {
            pos: [1, 2, 3],
            action: StockAction::Program { address: "Forge".into() },
        };
        assert_eq!(decode_stock_act(&encode_stock_act(&act)), Some(act));
    }

    #[test]
    fn requester_and_gauge_roundtrip() {
        let rr = RequesterOpenPayload {
            pos: [4, 70, 9],
            allow_partial: true,
            address: "Smelter".into(),
            entries: vec![StockItemWire { item_id: "minecraft:coal".into(), name: "Coal".into(), count: 32 }],
            held_item: Some(("minecraft:oak_log".into(), "Oak Log".into())),
        };
        assert_eq!(decode_requester_open(&encode_requester_open(&rr)), Some(rr));

        let rrs = RequesterSavePayload {
            pos: [4, 70, 9],
            allow_partial: false,
            address: "Warehouse".into(),
            entries: vec![("minecraft:coal".into(), 64)],
        };
        assert_eq!(decode_requester_save(&encode_requester_save(&rrs)), Some(rrs));

        let fg = GaugeOpenPayload {
            pos: [7, 80, -3],
            slot: 2,
            restocker: false,
            item: "create:cogwheel".into(),
            item_name: "Cogwheel".into(),
            target: 64,
            use_stacks: false,
            address: "Factory".into(),
            output_amount: 4,
            promise_interval: 30,
            in_stock: 16,
            promised: 8,
            satisfied: false,
            held_item: None,
        };
        assert_eq!(decode_gauge_open(&encode_gauge_open(&fg)), Some(fg));

        let fgs = GaugeSavePayload {
            pos: [7, 80, -3],
            slot: 2,
            clear_promises: false,
            item_override: Some("create:cogwheel".into()),
            target: 128,
            use_stacks: true,
            address: "Main".into(),
            output_amount: 8,
            promise_interval: -1,
        };
        assert_eq!(decode_gauge_save(&encode_gauge_save(&fgs)), Some(fgs));
    }
}
