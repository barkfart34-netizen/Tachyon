//! The client's custom screen for List Filter (`create:filter`) and Attribute Filter (`create:attribute_filter`).
//! Renders interactive filter slots/attribute rules and an in-screen player inventory picker for desktop and Android touch.

use std::sync::{Mutex, PoisonError};

use create_kinetics::rules::filter::{self as rules, Attribute, Mode};
use crate::compat::{
    ClientTickPostEvent, Flow, Hooks, Host, NetFromServerEvent, ScreenClosingEvent,
    ScreenKeyPressedPreEvent, ScreenMouseButtonPressedPreEvent, ScreenMouseScrolledPreEvent,
    ScreenRenderPostEvent, Text,
};

use super::create_gui::{
    self as gui, clean_wire, BtnStyle, UiRect, FRAME_BRASS, FRAME_DARK, PANEL_BRASS, PANEL_DARK,
    ROW_ALT, ROW_BG, TEXT_GOLD, TEXT_LIGHT, TEXT_MUTED,
};

pub const OPEN_CHANNEL: &str = "create:filter_open";
pub const SAVE_CHANNEL: &str = "create:filter_save";
pub const INSPECT_CHANNEL: &str = "create:filter_inspect";
pub const ATTRS_CHANNEL: &str = "create:filter_attrs";

const TAG: &str = "filter";
const SEP: char = '\u{1f}';
const SUB_SEP: char = '\u{1e}';
const FIELD_SEP: char = '\u{1d}';

const LIST_W: i32 = 304;
const LIST_H: i32 = 236;
const ATTR_W: i32 = 368;
const ATTR_H: i32 = 246;

struct Screen;

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Screen)
}

// --- wire types & codecs ----------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvEntry {
    pub slot: u8,
    pub item_id: String,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListSource {
    Existing(u16),
    Inv(u8),
}

impl ListSource {
    fn encode(self) -> String {
        match self {
            Self::Existing(i) => format!("E:{i}"),
            Self::Inv(s) => format!("I:{s}"),
        }
    }

    fn decode(s: &str) -> Option<Self> {
        if let Some(rest) = s.strip_prefix("E:") {
            Some(Self::Existing(rest.parse().ok()?))
        } else if let Some(rest) = s.strip_prefix("I:") {
            Some(Self::Inv(rest.parse().ok()?))
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListEntry {
    pub source: ListSource,
    pub item_id: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttrRuleWire {
    pub type_id: String,
    pub value: Option<String>,
    pub inverted: bool,
    pub desc: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttrCandidateWire {
    pub type_id: String,
    pub value: Option<String>,
    pub desc_normal: String,
    pub desc_inverted: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InspectAttrsPayload {
    pub inv_slot: u8,
    pub item_id: String,
    pub item_name: String,
    pub candidates: Vec<AttrCandidateWire>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenFilterPayload {
    List {
        blacklist: bool,
        respect_nbt: bool,
        entries: Vec<ListEntry>,
        inv: Vec<InvEntry>,
    },
    Attribute {
        mode: Mode,
        rules: Vec<AttrRuleWire>,
        inv: Vec<InvEntry>,
        inspected_slot: Option<u8>,
        inspected_name: String,
        candidates: Vec<AttrCandidateWire>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveFilterPayload {
    List {
        blacklist: bool,
        respect_nbt: bool,
        sources: Vec<ListSource>,
    },
    Attribute {
        mode: Mode,
        rules: Vec<(String, Option<String>, bool)>,
    },
}

fn clean_field(s: &str) -> String {
    clean_wire(s)
        .chars()
        .filter(|c| *c != FIELD_SEP)
        .collect()
}

fn encode_inv(inv: &[InvEntry]) -> String {
    inv.iter()
        .map(|e| format!("{}{}{}{}{}", e.slot, FIELD_SEP, clean_field(&e.item_id), FIELD_SEP, clean_field(&e.name)))
        .collect::<Vec<_>>()
        .join(&SUB_SEP.to_string())
}

fn decode_inv(raw: &str) -> Vec<InvEntry> {
    if raw.is_empty() {
        return Vec::new();
    }
    raw.split(SUB_SEP)
        .filter(|s| !s.is_empty())
        .filter_map(|rec| {
            let mut it = rec.splitn(3, FIELD_SEP);
            Some(InvEntry {
                slot: it.next()?.parse().ok()?,
                item_id: it.next()?.to_owned(),
                name: it.next()?.to_owned(),
            })
        })
        .collect()
}

fn encode_candidates(candidates: &[AttrCandidateWire]) -> String {
    candidates
        .iter()
        .map(|c| {
            format!(
                "{}{f}{}{f}{}{f}{}",
                clean_field(&c.type_id),
                clean_field(c.value.as_deref().unwrap_or("")),
                clean_field(&c.desc_normal),
                clean_field(&c.desc_inverted),
                f = FIELD_SEP,
            )
        })
        .collect::<Vec<_>>()
        .join(&SUB_SEP.to_string())
}

fn decode_candidates(raw: &str) -> Vec<AttrCandidateWire> {
    if raw.is_empty() {
        return Vec::new();
    }
    raw.split(SUB_SEP)
        .filter(|s| !s.is_empty())
        .filter_map(|rec| {
            let mut it = rec.splitn(4, FIELD_SEP);
            let type_id = it.next()?.to_owned();
            let val = it.next()?;
            let desc_normal = it.next()?.to_owned();
            let desc_inverted = it.next()?.to_owned();
            Some(AttrCandidateWire {
                type_id,
                value: (!val.is_empty()).then(|| val.to_owned()),
                desc_normal,
                desc_inverted,
            })
        })
        .collect()
}

pub fn encode_open(payload: &OpenFilterPayload) -> String {
    let f = FIELD_SEP;
    let s = SEP;
    match payload {
        OpenFilterPayload::List { blacklist, respect_nbt, entries, inv } => {
            let entries_wire = entries
                .iter()
                .map(|e| format!("{}{f}{}{f}{}", e.source.encode(), clean_field(&e.item_id), clean_field(&e.name)))
                .collect::<Vec<_>>()
                .join(&SUB_SEP.to_string());
            format!(
                "fltL1{s}{}{s}{}{s}{entries_wire}{s}{}",
                u8::from(*blacklist),
                u8::from(*respect_nbt),
                encode_inv(inv),
            )
        }
        OpenFilterPayload::Attribute {
            mode,
            rules: active_rules,
            inv,
            inspected_slot,
            inspected_name,
            candidates,
        } => {
            let rules_wire = active_rules
                .iter()
                .map(|r| {
                    format!(
                        "{}{f}{}{f}{}{f}{}",
                        clean_field(&r.type_id),
                        clean_field(r.value.as_deref().unwrap_or("")),
                        u8::from(r.inverted),
                        clean_field(&r.desc),
                    )
                })
                .collect::<Vec<_>>()
                .join(&SUB_SEP.to_string());
            let slot_str = inspected_slot.map_or_else(|| "-1".to_owned(), |n| n.to_string());
            format!(
                "fltA1{s}{}{s}{rules_wire}{s}{}{s}{slot_str}{s}{}{s}{}",
                mode.name(),
                encode_inv(inv),
                clean_field(inspected_name),
                encode_candidates(candidates),
            )
        }
    }
}

pub fn decode_open(text: &str) -> Option<OpenFilterPayload> {
    let mut it = text.split(SEP);
    match it.next()? {
        "fltL1" => {
            let blacklist = it.next()? == "1";
            let respect_nbt = it.next()? == "1";
            let raw_entries = it.next()?;
            let entries = if raw_entries.is_empty() {
                Vec::new()
            } else {
                raw_entries
                    .split(SUB_SEP)
                    .filter(|s| !s.is_empty())
                    .filter_map(|rec| {
                        let mut parts = rec.splitn(3, FIELD_SEP);
                        Some(ListEntry {
                            source: ListSource::decode(parts.next()?)?,
                            item_id: parts.next()?.to_owned(),
                            name: parts.next()?.to_owned(),
                        })
                    })
                    .collect()
            };
            let inv = decode_inv(it.next()?);
            Some(OpenFilterPayload::List { blacklist, respect_nbt, entries, inv })
        }
        "fltA1" => {
            let mode = Mode::from_name(it.next()?)?;
            let raw_rules = it.next()?;
            let active_rules = if raw_rules.is_empty() {
                Vec::new()
            } else {
                raw_rules
                    .split(SUB_SEP)
                    .filter(|s| !s.is_empty())
                    .filter_map(|rec| {
                        let mut parts = rec.splitn(4, FIELD_SEP);
                        let type_id = parts.next()?.to_owned();
                        let val = parts.next()?;
                        let inverted = parts.next()? == "1";
                        let desc = parts.next()?.to_owned();
                        Some(AttrRuleWire {
                            type_id,
                            value: (!val.is_empty()).then(|| val.to_owned()),
                            inverted,
                            desc,
                        })
                    })
                    .collect()
            };
            let inv = decode_inv(it.next()?);
            let slot_raw: i32 = it.next()?.parse().ok()?;
            let inspected_slot = u8::try_from(slot_raw).ok();
            let inspected_name = it.next()?.to_owned();
            let candidates = decode_candidates(it.next()?);
            Some(OpenFilterPayload::Attribute {
                mode,
                rules: active_rules,
                inv,
                inspected_slot,
                inspected_name,
                candidates,
            })
        }
        _ => None,
    }
}

pub fn encode_attrs(payload: &InspectAttrsPayload) -> String {
    format!(
        "fltI1{s}{}{s}{}{s}{}{s}{}",
        payload.inv_slot,
        clean_field(&payload.item_id),
        clean_field(&payload.item_name),
        encode_candidates(&payload.candidates),
        s = SEP,
    )
}

pub fn decode_attrs(text: &str) -> Option<InspectAttrsPayload> {
    let mut it = text.split(SEP);
    if it.next()? != "fltI1" {
        return None;
    }
    let inv_slot = it.next()?.parse().ok()?;
    let item_id = it.next()?.to_owned();
    let item_name = it.next()?.to_owned();
    let candidates = decode_candidates(it.next()?);
    Some(InspectAttrsPayload { inv_slot, item_id, item_name, candidates })
}

pub fn encode_save(payload: &SaveFilterPayload) -> String {
    let s = SEP;
    let f = FIELD_SEP;
    match payload {
        SaveFilterPayload::List { blacklist, respect_nbt, sources } => {
            let list = sources.iter().map(|src| src.encode()).collect::<Vec<_>>().join(",");
            format!("fltsaveL1{s}{}{s}{}{s}{list}", u8::from(*blacklist), u8::from(*respect_nbt))
        }
        SaveFilterPayload::Attribute { mode, rules: active_rules } => {
            let list = active_rules
                .iter()
                .map(|(t, v, inv)| format!("{}{f}{}{f}{}", clean_field(t), clean_field(v.as_deref().unwrap_or("")), u8::from(*inv)))
                .collect::<Vec<_>>()
                .join(&SUB_SEP.to_string());
            format!("fltsaveA1{s}{}{s}{list}", mode.name())
        }
    }
}

pub fn decode_save(text: &str) -> Option<SaveFilterPayload> {
    let mut it = text.split(SEP);
    match it.next()? {
        "fltsaveL1" => {
            let blacklist = it.next()? == "1";
            let respect_nbt = it.next()? == "1";
            let raw = it.next()?;
            let sources = if raw.is_empty() {
                Vec::new()
            } else {
                raw.split(',').filter(|s| !s.is_empty()).filter_map(ListSource::decode).collect()
            };
            Some(SaveFilterPayload::List { blacklist, respect_nbt, sources })
        }
        "fltsaveA1" => {
            let mode = Mode::from_name(it.next()?)?;
            let raw = it.next()?;
            let active_rules = if raw.is_empty() {
                Vec::new()
            } else {
                raw.split(SUB_SEP)
                    .filter(|s| !s.is_empty())
                    .filter_map(|rec| {
                        let mut parts = rec.splitn(3, FIELD_SEP);
                        let t = parts.next()?.to_owned();
                        let v = parts.next()?;
                        let inv = parts.next()? == "1";
                        Some((t, (!v.is_empty()).then(|| v.to_owned()), inv))
                    })
                    .collect()
            };
            Some(SaveFilterPayload::Attribute { mode, rules: active_rules })
        }
        _ => None,
    }
}

// --- pure UI manipulation helpers -------------------------------------------------------------------------------

pub fn toggle_inv_entry_in_list(entries: &mut Vec<ListEntry>, item: &InvEntry) -> rules::Toggle {
    if let Some(pos) = entries.iter().position(|e| e.source == ListSource::Inv(item.slot)) {
        entries.remove(pos);
        return rules::Toggle::Removed;
    }
    if entries.len() < rules::LIST_SLOTS {
        entries.push(ListEntry {
            source: ListSource::Inv(item.slot),
            item_id: item.item_id.clone(),
            name: item.name.clone(),
        });
        return rules::Toggle::Added;
    }
    rules::Toggle::Full
}

pub fn describe_rule(type_id: &str, value: Option<&str>, inverted: bool, fallback: &str) -> String {
    Attribute::parse(type_id, value).map_or_else(|| fallback.to_owned(), |a| a.describe(inverted))
}

pub fn add_candidate_rule(active: &mut Vec<AttrRuleWire>, cand: &AttrCandidateWire, inverted: bool) -> bool {
    if active.iter().any(|r| r.type_id == cand.type_id && r.value == cand.value && r.inverted == inverted) {
        return false;
    }
    let desc = if inverted {
        cand.desc_inverted.clone()
    } else {
        cand.desc_normal.clone()
    };
    active.push(AttrRuleWire {
        type_id: cand.type_id.clone(),
        value: cand.value.clone(),
        inverted,
        desc,
    });
    true
}

// --- open screen state ------------------------------------------------------------------------------------------

enum ModeState {
    List {
        blacklist: bool,
        respect_nbt: bool,
        entries: Vec<ListEntry>,
        inv: Vec<InvEntry>,
        inv_page: usize,
        status: String,
    },
    Attribute {
        mode: Mode,
        rules: Vec<AttrRuleWire>,
        inv: Vec<InvEntry>,
        inspected_slot: Option<u8>,
        inspected_name: String,
        candidates: Vec<AttrCandidateWire>,
        rule_scroll: usize,
        cand_scroll: usize,
        inv_page: usize,
    },
}

struct Open {
    state: ModeState,
    size: [i32; 2],
    close: bool,
}

impl Open {
    fn from_payload(payload: OpenFilterPayload) -> Self {
        let state = match payload {
            OpenFilterPayload::List { blacklist, respect_nbt, entries, inv } => ModeState::List {
                blacklist,
                respect_nbt,
                entries,
                inv,
                inv_page: 0,
                status: String::new(),
            },
            OpenFilterPayload::Attribute {
                mode,
                rules,
                inv,
                inspected_slot,
                inspected_name,
                candidates,
            } => ModeState::Attribute {
                mode,
                rules,
                inv,
                inspected_slot,
                inspected_name,
                candidates,
                rule_scroll: 0,
                cand_scroll: 0,
                inv_page: 0,
            },
        };
        Self { state, size: [0, 0], close: false }
    }

    fn origin(&self) -> (i32, i32) {
        match &self.state {
            ModeState::List { .. } => gui::screen_origin(self.size, LIST_W, LIST_H),
            ModeState::Attribute { .. } => gui::screen_origin(self.size, ATTR_W, ATTR_H),
        }
    }

    fn save_and_close(&mut self) {
        let payload = match &self.state {
            ModeState::List { blacklist, respect_nbt, entries, .. } => SaveFilterPayload::List {
                blacklist: *blacklist,
                respect_nbt: *respect_nbt,
                sources: entries.iter().map(|e| e.source).collect(),
            },
            ModeState::Attribute { mode, rules, .. } => SaveFilterPayload::Attribute {
                mode: *mode,
                rules: rules.iter().map(|r| (r.type_id.clone(), r.value.clone(), r.inverted)).collect(),
            },
        };
        send(SAVE_CHANNEL, encode_save(&payload).into_bytes());
        self.close = true;
    }
}

static CLIENT: Mutex<Option<Open>> = Mutex::new(None);
static OUTBOX: Mutex<Vec<(&'static str, Vec<u8>)>> = Mutex::new(Vec::new());

fn client() -> std::sync::MutexGuard<'static, Option<Open>> {
    CLIENT.lock().unwrap_or_else(PoisonError::into_inner)
}

fn send(channel: &'static str, data: Vec<u8>) {
    OUTBOX.lock().unwrap_or_else(PoisonError::into_inner).push((channel, data));
}

impl Hooks for Screen {
    fn net_from_server(&self, host: &mut dyn Host, event: &mut NetFromServerEvent<'_>) {
        let Ok(text) = std::str::from_utf8(event.data) else { return };
        match event.channel {
            OPEN_CHANNEL => {
                let Some(payload) = decode_open(text) else { return };
                if gui::open(host, TAG) {
                    *client() = Some(Open::from_payload(payload));
                }
            }
            ATTRS_CHANNEL => {
                let Some(payload) = decode_attrs(text) else { return };
                if let Some(open) = client().as_mut()
                    && let ModeState::Attribute {
                        inspected_slot,
                        inspected_name,
                        candidates,
                        cand_scroll,
                        ..
                    } = &mut open.state
                {
                    *inspected_slot = Some(payload.inv_slot);
                    *inspected_name = payload.item_name;
                    *candidates = payload.candidates;
                    *cand_scroll = 0;
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
        for (channel, data) in pending {
            let _ = host.net_send_to_server(channel, &data);
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

    fn screen_mouse_button_pressed_pre(&self, host: &dyn Host, event: &mut ScreenMouseButtonPressedPreEvent<'_>) -> Flow {
        if event.screen.id != "mod" || !gui::is_active(host, TAG) {
            return Flow::Continue;
        }
        let mut c = client();
        let Some(open) = c.as_mut() else { return Flow::Continue };
        if open.size[0] == 0 {
            return Flow::Cancel;
        }
        click(open, event.screen.mouse);
        Flow::Cancel
    }

    fn screen_mouse_scrolled_pre(&self, host: &dyn Host, event: &mut ScreenMouseScrolledPreEvent<'_>) -> Flow {
        if event.screen.id != "mod" || !gui::is_active(host, TAG) {
            return Flow::Continue;
        }
        let mut c = client();
        let Some(open) = c.as_mut() else { return Flow::Continue };
        let (left, top) = open.origin();
        let mouse = event.screen.mouse;
        let up = event.scroll[1] > 0.0;
        if let ModeState::Attribute { rules, candidates, rule_scroll, cand_scroll, .. } = &mut open.state {
            let (left_pane, right_pane) = attr_panes(left, top);
            if left_pane.contains(mouse) {
                let max = rules.len().saturating_sub(4);
                *rule_scroll = if up { rule_scroll.saturating_sub(1) } else { (*rule_scroll + 1).min(max) };
            } else if right_pane.contains(mouse) {
                let max = candidates.len().saturating_sub(4);
                *cand_scroll = if up { cand_scroll.saturating_sub(1) } else { (*cand_scroll + 1).min(max) };
            }
        }
        Flow::Cancel
    }

    fn screen_key_pressed_pre(&self, host: &dyn Host, event: &mut ScreenKeyPressedPreEvent<'_>) -> Flow {
        if event.screen.id != "mod" || !gui::is_active(host, TAG) {
            return Flow::Continue;
        }
        if event.key == "key.keyboard.escape" {
            return Flow::Continue;
        }
        let mut c = client();
        let Some(open) = c.as_mut() else { return Flow::Continue };
        if event.key == "key.keyboard.enter" || event.key == "key.keyboard.kp.enter" {
            open.save_and_close();
        }
        Flow::Cancel
    }
}

// --- layout -----------------------------------------------------------------------------------------------------

fn list_slot_rect(left: i32, top: i32, idx: usize) -> UiRect {
    let col = (idx % 9) as i32;
    let row = (idx / 9) as i32;
    UiRect::new(left + 23 + col * 29, top + 34 + row * 28, 26, 26)
}

fn list_mode_rects(left: i32, top: i32) -> (UiRect, UiRect, UiRect) {
    let y = top + 92;
    (
        UiRect::new(left + 12, y, 96, 20),
        UiRect::new(left + 114, y, 108, 20),
        UiRect::new(left + 228, y, 64, 20),
    )
}

fn list_inv_slot_rect(left: i32, top: i32, idx: usize) -> UiRect {
    let col = (idx % 9) as i32;
    let row = (idx / 9) as i32;
    UiRect::new(left + 23 + col * 29, top + 132 + row * 28, 26, 26)
}

fn list_inv_pager(left: i32, top: i32) -> (UiRect, UiRect) {
    (UiRect::new(left + 244, top + 116, 20, 14), UiRect::new(left + 268, top + 116, 20, 14))
}

fn list_footer_rects(left: i32, top: i32) -> (UiRect, UiRect) {
    (UiRect::new(left + 160, top + 206, 62, 22), UiRect::new(left + 228, top + 206, 64, 22))
}

fn attr_mode_rects(left: i32, top: i32) -> ([UiRect; 3], UiRect) {
    let y = top + 30;
    (
        [
            UiRect::new(left + 8, y, 88, 20),
            UiRect::new(left + 100, y, 88, 20),
            UiRect::new(left + 192, y, 76, 20),
        ],
        UiRect::new(left + 296, y, 64, 20),
    )
}

fn attr_panes(left: i32, top: i32) -> (UiRect, UiRect) {
    (UiRect::new(left + 8, top + 56, 172, 112), UiRect::new(left + 186, top + 56, 174, 112))
}

fn attr_scroll_btns(pane: UiRect) -> (UiRect, UiRect) {
    (UiRect::new(pane.x + pane.w - 40, pane.y + 2, 18, 16), UiRect::new(pane.x + pane.w - 20, pane.y + 2, 18, 16))
}

fn attr_rule_row(pane: UiRect, vis: usize) -> (UiRect, UiRect, UiRect) {
    let y = pane.y + 21 + vis as i32 * 22;
    (
        UiRect::new(pane.x + 3, y, pane.w - 50, 20),
        UiRect::new(pane.x + pane.w - 44, y, 20, 20),
        UiRect::new(pane.x + pane.w - 22, y, 20, 20),
    )
}

fn attr_cand_row(pane: UiRect, vis: usize) -> (UiRect, UiRect, UiRect) {
    let y = pane.y + 21 + vis as i32 * 22;
    (
        UiRect::new(pane.x + 3, y, pane.w - 50, 20),
        UiRect::new(pane.x + pane.w - 44, y, 20, 20),
        UiRect::new(pane.x + pane.w - 22, y, 20, 20),
    )
}

fn attr_inv_rects(left: i32, top: i32) -> (UiRect, [UiRect; 10], UiRect) {
    let y = top + 182;
    let prev = UiRect::new(left + 22, y, 20, 24);
    let slots = std::array::from_fn(|i| UiRect::new(left + 46 + i as i32 * 27, y, 24, 24));
    let next = UiRect::new(left + 320, y, 20, 24);
    (prev, slots, next)
}

fn attr_footer_rects(left: i32, top: i32) -> (UiRect, UiRect) {
    (UiRect::new(left + 220, top + 216, 66, 22), UiRect::new(left + 292, top + 216, 68, 22))
}

// --- input ------------------------------------------------------------------------------------------------------

fn click(open: &mut Open, mouse: [f32; 2]) {
    let (left, top) = open.origin();
    let mut should_save = false;
    match &mut open.state {
        ModeState::List { blacklist, respect_nbt, entries, inv, inv_page, status } => {
            for i in 0..rules::LIST_SLOTS {
                if i < entries.len() && list_slot_rect(left, top, i).contains(mouse) {
                    let removed = entries.remove(i);
                    *status = format!("Removed {}", removed.name);
                    return;
                }
            }
            let (mode_btn, nbt_btn, clear_btn) = list_mode_rects(left, top);
            if mode_btn.contains(mouse) {
                *blacklist = !*blacklist;
                *status = if *blacklist { "Deny-List" } else { "Allow-List" }.to_owned();
                return;
            }
            if nbt_btn.contains(mouse) {
                *respect_nbt = !*respect_nbt;
                *status = if *respect_nbt { "Respect Data" } else { "Ignore Data" }.to_owned();
                return;
            }
            if clear_btn.contains(mouse) {
                entries.clear();
                *status = "Cleared".to_owned();
                return;
            }
            let pages = inv.len().div_ceil(18).max(1);
            *inv_page = (*inv_page).min(pages - 1);
            let (prev_btn, next_btn) = list_inv_pager(left, top);
            if pages > 1 && prev_btn.contains(mouse) {
                *inv_page = if *inv_page == 0 { pages - 1 } else { *inv_page - 1 };
                return;
            }
            if pages > 1 && next_btn.contains(mouse) {
                *inv_page = (*inv_page + 1) % pages;
                return;
            }
            let base = *inv_page * 18;
            for vis in 0..18 {
                let idx = base + vis;
                if idx < inv.len() && list_inv_slot_rect(left, top, vis).contains(mouse) {
                    let item = &inv[idx];
                    match toggle_inv_entry_in_list(entries, item) {
                        rules::Toggle::Added => *status = format!("Added {}", item.name),
                        rules::Toggle::Removed => *status = format!("Removed {}", item.name),
                        rules::Toggle::Full => *status = format!("Full ({} max)", rules::LIST_SLOTS),
                    }
                    return;
                }
            }
            let (cancel_btn, save_btn) = list_footer_rects(left, top);
            if cancel_btn.contains(mouse) {
                open.close = true;
            } else if save_btn.contains(mouse) {
                should_save = true;
            }
        }
        ModeState::Attribute {
            mode,
            rules,
            inv,
            inspected_slot,
            candidates,
            rule_scroll,
            cand_scroll,
            inv_page,
            ..
        } => {
            let (mode_btns, clear_btn) = attr_mode_rects(left, top);
            let modes = [Mode::WhitelistDisj, Mode::WhitelistConj, Mode::Blacklist];
            for (i, r) in mode_btns.into_iter().enumerate() {
                if r.contains(mouse) {
                    *mode = modes[i];
                    return;
                }
            }
            if clear_btn.contains(mouse) {
                rules.clear();
                *rule_scroll = 0;
                return;
            }
            let (left_pane, right_pane) = attr_panes(left, top);
            let (r_up, r_dn) = attr_scroll_btns(left_pane);
            if r_up.contains(mouse) {
                *rule_scroll = rule_scroll.saturating_sub(1);
                return;
            }
            if r_dn.contains(mouse) {
                let max = rules.len().saturating_sub(4);
                *rule_scroll = (*rule_scroll + 1).min(max);
                return;
            }
            for vis in 0..4 {
                let idx = *rule_scroll + vis;
                if idx >= rules.len() {
                    break;
                }
                let (_, inv_btn, del_btn) = attr_rule_row(left_pane, vis);
                if inv_btn.contains(mouse) {
                    let r = &mut rules[idx];
                    r.inverted = !r.inverted;
                    r.desc = describe_rule(&r.type_id, r.value.as_deref(), r.inverted, &r.desc);
                    return;
                }
                if del_btn.contains(mouse) {
                    rules.remove(idx);
                    *rule_scroll = (*rule_scroll).min(rules.len().saturating_sub(4));
                    return;
                }
            }

            let (c_up, c_dn) = attr_scroll_btns(right_pane);
            if c_up.contains(mouse) {
                *cand_scroll = cand_scroll.saturating_sub(1);
                return;
            }
            if c_dn.contains(mouse) {
                let max = candidates.len().saturating_sub(4);
                *cand_scroll = (*cand_scroll + 1).min(max);
                return;
            }
            for vis in 0..4 {
                let idx = *cand_scroll + vis;
                if idx >= candidates.len() {
                    break;
                }
                let (_, add_btn, not_btn) = attr_cand_row(right_pane, vis);
                if add_btn.contains(mouse) {
                    add_candidate_rule(rules, &candidates[idx], false);
                    return;
                }
                if not_btn.contains(mouse) {
                    add_candidate_rule(rules, &candidates[idx], true);
                    return;
                }
            }

            let pages = inv.len().div_ceil(10).max(1);
            *inv_page = (*inv_page).min(pages - 1);
            let (prev_btn, inv_slots, next_btn) = attr_inv_rects(left, top);
            if pages > 1 && prev_btn.contains(mouse) {
                *inv_page = if *inv_page == 0 { pages - 1 } else { *inv_page - 1 };
                return;
            }
            if pages > 1 && next_btn.contains(mouse) {
                *inv_page = (*inv_page + 1) % pages;
                return;
            }
            let base = *inv_page * 10;
            for (vis, rect) in inv_slots.into_iter().enumerate() {
                let idx = base + vis;
                if idx < inv.len() && rect.contains(mouse) {
                    let slot = inv[idx].slot;
                    *inspected_slot = Some(slot);
                    send(INSPECT_CHANNEL, slot.to_string().into_bytes());
                    return;
                }
            }

            let (cancel_btn, save_btn) = attr_footer_rects(left, top);
            if cancel_btn.contains(mouse) {
                open.close = true;
            } else if save_btn.contains(mouse) {
                should_save = true;
            }
        }
    }
    if should_save {
        open.save_and_close();
    }
}

// --- drawing ----------------------------------------------------------------------------------------------------

fn draw(host: &dyn Host, event: &mut ScreenRenderPostEvent<'_>, open: &Open) {
    gui::fill(host, event, UiRect::new(0, 0, open.size[0], open.size[1]), gui::DIM);
    let (left, top) = open.origin();
    let mut tip: Option<Vec<String>> = None;

    match &open.state {
        ModeState::List { blacklist, respect_nbt, entries, inv, inv_page, status } => {
            gui::panel(host, event, UiRect::new(left, top, LIST_W, LIST_H), FRAME_BRASS, PANEL_BRASS);
            let badge = format!("{}/{}", entries.len(), rules::LIST_SLOTS);
            gui::header(host, event, UiRect::new(left + 2, top + 2, LIST_W - 4, 22), Some(super::filter::LIST), "List Filter", Some(&badge));

            for i in 0..rules::LIST_SLOTS {
                let r = list_slot_rect(left, top, i);
                let entry = entries.get(i);
                if gui::item_slot(host, event, r, entry.map(|e| e.item_id.as_str()), false)
                    && let Some(e) = entry
                {
                    tip = Some(vec![e.name.clone(), "Click to remove".to_owned()]);
                }
            }

            let (mode_btn, nbt_btn, clear_btn) = list_mode_rects(left, top);
            let mode_lbl = if *blacklist { "Deny-List" } else { "Allow-List" };
            let mode_style = if *blacklist { BtnStyle::Danger } else { BtnStyle::Primary };
            if gui::button(host, event, mode_btn, mode_lbl, mode_style, true) {
                tip = Some(vec!["Click to toggle Allow-List / Deny-List".to_owned()]);
            }
            let nbt_lbl = if *respect_nbt { "Respect Data" } else { "Ignore Data" };
            let nbt_style = if *respect_nbt { BtnStyle::ToggleOn } else { BtnStyle::Normal };
            if gui::button(host, event, nbt_btn, nbt_lbl, nbt_style, true) {
                tip = Some(vec!["Compare item components / NBT data".to_owned()]);
            }
            gui::button(host, event, clear_btn, "Clear", BtnStyle::Normal, !entries.is_empty());

            host.text(event.batch, "Inventory (tap to toggle):", [(left + 12) as f32, (top + 118) as f32], TEXT_MUTED, false);
            let pages = inv.len().div_ceil(18).max(1);
            let page = (*inv_page).min(pages - 1);
            if pages > 1 {
                let (prev_btn, next_btn) = list_inv_pager(left, top);
                gui::button(host, event, prev_btn, "<", BtnStyle::Normal, true);
                gui::button(host, event, next_btn, ">", BtnStyle::Normal, true);
            }
            let base = page * 18;
            for vis in 0..18 {
                let r = list_inv_slot_rect(left, top, vis);
                let item = inv.get(base + vis);
                let selected = item.is_some_and(|it| entries.iter().any(|e| e.source == ListSource::Inv(it.slot)));
                if gui::item_slot(host, event, r, item.map(|it| it.item_id.as_str()), selected)
                    && let Some(it) = item
                {
                    let hint = if selected { "Click to remove from filter" } else { "Click to add to filter" };
                    tip = Some(vec![it.name.clone(), hint.to_owned()]);
                }
            }

            if !status.is_empty() {
                let s = gui::truncate(host, status, 140.0);
                host.text(event.batch, &s, [(left + 12) as f32, (top + 213) as f32], TEXT_GOLD, false);
            }
            let (cancel_btn, save_btn) = list_footer_rects(left, top);
            gui::button(host, event, cancel_btn, "Cancel", BtnStyle::Normal, true);
            gui::button(host, event, save_btn, "Save", BtnStyle::Primary, true);
        }
        ModeState::Attribute {
            mode,
            rules,
            inv,
            inspected_slot,
            inspected_name,
            candidates,
            rule_scroll,
            cand_scroll,
            inv_page,
        } => {
            gui::panel(host, event, UiRect::new(left, top, ATTR_W, ATTR_H), FRAME_BRASS, PANEL_BRASS);
            let badge = format!("{} rules", rules.len());
            gui::header(host, event, UiRect::new(left + 2, top + 2, ATTR_W - 4, 22), Some(super::filter::ATTRIBUTE), "Attribute Filter", Some(&badge));

            let (mode_btns, clear_btn) = attr_mode_rects(left, top);
            let mode_defs = [
                (Mode::WhitelistDisj, "Allow Any"),
                (Mode::WhitelistConj, "Allow All"),
                (Mode::Blacklist, "Deny-List"),
            ];
            for (i, (m, label)) in mode_defs.into_iter().enumerate() {
                let active = *mode == m;
                let style = if active && m == Mode::Blacklist {
                    BtnStyle::Danger
                } else if active {
                    BtnStyle::ToggleOn
                } else {
                    BtnStyle::Normal
                };
                gui::button(host, event, mode_btns[i], label, style, true);
            }
            gui::button(host, event, clear_btn, "Clear", BtnStyle::Normal, !rules.is_empty());

            let (left_pane, right_pane) = attr_panes(left, top);
            gui::panel(host, event, left_pane, FRAME_DARK, PANEL_DARK);
            host.text(event.batch, "Active Rules", [(left_pane.x + 6) as f32, (left_pane.y + 6) as f32], TEXT_GOLD, false);
            let (r_up, r_dn) = attr_scroll_btns(left_pane);
            gui::button(host, event, r_up, "^", BtnStyle::Normal, *rule_scroll > 0);
            gui::button(host, event, r_dn, "v", BtnStyle::Normal, *rule_scroll + 4 < rules.len());

            for vis in 0..4 {
                let idx = *rule_scroll + vis;
                let Some(rule) = rules.get(idx) else { continue };
                let (lbl_r, inv_btn, del_btn) = attr_rule_row(left_pane, vis);
                let row_bg = if idx % 2 == 0 { ROW_BG } else { ROW_ALT };
                gui::fill(host, event, lbl_r, row_bg);
                let text = gui::truncate(host, &rule.desc, (lbl_r.w - 6) as f32);
                let ink = if rule.inverted { gui::TEXT_RED } else { TEXT_LIGHT };
                host.text(event.batch, &text, [(lbl_r.x + 3) as f32, (lbl_r.y + 6) as f32], ink, false);
                if lbl_r.contains(event.screen.mouse) {
                    tip = Some(vec![rule.desc.clone()]);
                }
                let inv_style = if rule.inverted { BtnStyle::Danger } else { BtnStyle::Normal };
                if gui::button(host, event, inv_btn, "!", inv_style, true) {
                    tip = Some(vec!["Invert rule (IS / NOT)".to_owned()]);
                }
                if gui::button(host, event, del_btn, "X", BtnStyle::Normal, true) {
                    tip = Some(vec!["Remove rule".to_owned()]);
                }
            }
            if rules.is_empty() {
                host.text(event.batch, "Pick attributes ->", [(left_pane.x + 14) as f32, (left_pane.y + 52) as f32], TEXT_MUTED, false);
            }

            gui::panel(host, event, right_pane, FRAME_DARK, PANEL_DARK);
            let hdr = if inspected_name.is_empty() {
                "Pick item below".to_owned()
            } else {
                gui::truncate(host, &format!("Item: {inspected_name}"), (right_pane.w - 50) as f32)
            };
            host.text(event.batch, &hdr, [(right_pane.x + 6) as f32, (right_pane.y + 6) as f32], TEXT_GOLD, false);
            let (c_up, c_dn) = attr_scroll_btns(right_pane);
            gui::button(host, event, c_up, "^", BtnStyle::Normal, *cand_scroll > 0);
            gui::button(host, event, c_dn, "v", BtnStyle::Normal, *cand_scroll + 4 < candidates.len());

            for vis in 0..4 {
                let idx = *cand_scroll + vis;
                let Some(cand) = candidates.get(idx) else { continue };
                let (lbl_r, add_btn, not_btn) = attr_cand_row(right_pane, vis);
                let row_bg = if idx % 2 == 0 { ROW_BG } else { ROW_ALT };
                gui::fill(host, event, lbl_r, row_bg);
                let text = gui::truncate(host, &cand.desc_normal, (lbl_r.w - 6) as f32);
                host.text(event.batch, &text, [(lbl_r.x + 3) as f32, (lbl_r.y + 6) as f32], TEXT_LIGHT, false);
                if lbl_r.contains(event.screen.mouse) {
                    tip = Some(vec![cand.desc_normal.clone(), format!("NOT: {}", cand.desc_inverted)]);
                }
                if gui::button(host, event, add_btn, "+", BtnStyle::Primary, true) {
                    tip = Some(vec![format!("Add: {}", cand.desc_normal)]);
                }
                if gui::button(host, event, not_btn, "!", BtnStyle::Danger, true) {
                    tip = Some(vec![format!("Add: {}", cand.desc_inverted)]);
                }
            }
            if candidates.is_empty() {
                host.text(event.batch, "No item inspected", [(right_pane.x + 14) as f32, (right_pane.y + 52) as f32], TEXT_MUTED, false);
            }

            host.text(event.batch, "Inspect inventory item:", [(left + 8) as f32, (top + 171) as f32], TEXT_MUTED, false);
            let pages = inv.len().div_ceil(10).max(1);
            let page = (*inv_page).min(pages - 1);
            let (prev_btn, inv_slots, next_btn) = attr_inv_rects(left, top);
            gui::button(host, event, prev_btn, "<", BtnStyle::Normal, pages > 1);
            gui::button(host, event, next_btn, ">", BtnStyle::Normal, pages > 1);
            let base = page * 10;
            for (vis, rect) in inv_slots.into_iter().enumerate() {
                let item = inv.get(base + vis);
                let selected = item.is_some_and(|it| *inspected_slot == Some(it.slot));
                if gui::item_slot(host, event, rect, item.map(|it| it.item_id.as_str()), selected)
                    && let Some(it) = item
                {
                    tip = Some(vec![it.name.clone(), "Click to inspect attributes".to_owned()]);
                }
            }

            let hint = mode.label();
            host.text(event.batch, hint, [(left + 10) as f32, (top + 223) as f32], TEXT_MUTED, false);
            let (cancel_btn, save_btn) = attr_footer_rects(left, top);
            gui::button(host, event, cancel_btn, "Cancel", BtnStyle::Normal, true);
            gui::button(host, event, save_btn, "Save", BtnStyle::Primary, true);
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
    fn list_open_and_save_codecs_round_trip() {
        let open = OpenFilterPayload::List {
            blacklist: true,
            respect_nbt: false,
            entries: vec![ListEntry {
                source: ListSource::Existing(0),
                item_id: "minecraft:iron_ingot".into(),
                name: "Iron Ingot".into(),
            }],
            inv: vec![InvEntry {
                slot: 4,
                item_id: "create:brass_ingot".into(),
                name: "Brass Ingot".into(),
            }],
        };
        assert_eq!(decode_open(&encode_open(&open)), Some(open));

        let save = SaveFilterPayload::List {
            blacklist: false,
            respect_nbt: true,
            sources: vec![ListSource::Existing(0), ListSource::Inv(4)],
        };
        assert_eq!(decode_save(&encode_save(&save)), Some(save));
    }

    #[test]
    fn attribute_codecs_and_rules_round_trip() {
        let open = OpenFilterPayload::Attribute {
            mode: Mode::WhitelistConj,
            rules: vec![AttrRuleWire {
                type_id: "create:in_tag".into(),
                value: Some("minecraft:logs".into()),
                inverted: true,
                desc: "is not tagged #minecraft:logs".into(),
            }],
            inv: vec![InvEntry { slot: 1, item_id: "minecraft:oak_log".into(), name: "Oak Log".into() }],
            inspected_slot: Some(1),
            inspected_name: "Oak Log".into(),
            candidates: vec![AttrCandidateWire {
                type_id: "create:in_tag".into(),
                value: Some("minecraft:logs".into()),
                desc_normal: "is tagged #minecraft:logs".into(),
                desc_inverted: "is not tagged #minecraft:logs".into(),
            }],
        };
        assert_eq!(decode_open(&encode_open(&open)), Some(open));

        let attrs = InspectAttrsPayload {
            inv_slot: 3,
            item_id: "minecraft:apple".into(),
            item_name: "Apple".into(),
            candidates: vec![AttrCandidateWire {
                type_id: "create:consumable".into(),
                value: None,
                desc_normal: "can be eaten".into(),
                desc_inverted: "cannot be eaten".into(),
            }],
        };
        assert_eq!(decode_attrs(&encode_attrs(&attrs)), Some(attrs));

        let save = SaveFilterPayload::Attribute {
            mode: Mode::Blacklist,
            rules: vec![("create:consumable".into(), None, false)],
        };
        assert_eq!(decode_save(&encode_save(&save)), Some(save));
    }

    #[test]
    fn toggling_list_entries_respects_18_slot_limit() {
        let mut entries = Vec::new();
        for slot in 0..18u8 {
            let item = InvEntry { slot, item_id: format!("minecraft:item_{slot}"), name: format!("Item {slot}") };
            assert_eq!(toggle_inv_entry_in_list(&mut entries, &item), rules::Toggle::Added);
        }
        let extra = InvEntry { slot: 25, item_id: "minecraft:diamond".into(), name: "Diamond".into() };
        assert_eq!(toggle_inv_entry_in_list(&mut entries, &extra), rules::Toggle::Full);
        let existing = InvEntry { slot: 3, item_id: "minecraft:item_3".into(), name: "Item 3".into() };
        assert_eq!(toggle_inv_entry_in_list(&mut entries, &existing), rules::Toggle::Removed);
    }

    #[test]
    fn adding_candidate_deduplicates_identical_polarity() {
        let mut active = Vec::new();
        let cand = AttrCandidateWire {
            type_id: "create:consumable".into(),
            value: None,
            desc_normal: "can be eaten".into(),
            desc_inverted: "cannot be eaten".into(),
        };
        assert!(add_candidate_rule(&mut active, &cand, false));
        assert!(!add_candidate_rule(&mut active, &cand, false));
        assert!(add_candidate_rule(&mut active, &cand, true));
        assert_eq!(active.len(), 2);
    }
}
