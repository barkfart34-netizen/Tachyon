//! Client custom GUIs for Create transit & package port blocks:
//! - Elevator Contact (`create:elevator_contact`) -> `create:ec_open` / `create:ec_save`
//! - Package Frogport (`create:package_frogport`) & Postboxes (`create:*_postbox`) -> `create:pp_open` / `create:pp_save`

use std::sync::{Mutex, PoisonError};

use create_kinetics::rules::trains::station_screen::TextField;
use crate::compat::{
    ClientTickPostEvent, Flow, Hooks, Host, NetFromServerEvent, ScreenCharTypedPreEvent,
    ScreenClosingEvent, ScreenKeyPressedPreEvent, ScreenMouseButtonPressedPreEvent,
    ScreenRenderPostEvent, Text,
};

use super::create_gui::{
    self as gui, clean_wire, BtnStyle, UiRect, FRAME_BRASS, PANEL_BRASS, TEXT_GOLD, TEXT_LIGHT,
    TEXT_MUTED,
};

pub const EC_OPEN_CHANNEL: &str = "create:ec_open";
pub const EC_SAVE_CHANNEL: &str = "create:ec_save";
pub const PP_OPEN_CHANNEL: &str = "create:pp_open";
pub const PP_SAVE_CHANNEL: &str = "create:pp_save";

const TAG: &str = "port_ui";
const SEP: char = '\u{1f}';
const SUB_SEP: char = '\u{1e}';
const FIELD_SEP: char = '\u{1d}';
const PANEL_W: i32 = 332;
const PANEL_H: i32 = 208;

struct Screen;

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Screen)
}

fn clean_f(s: &str) -> String {
    clean_wire(s).chars().filter(|&c| c != FIELD_SEP).collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElevatorOpenPayload {
    pub pos: [i32; 3],
    pub short_name: String,
    pub long_name: String,
    pub door_control: u8,
    pub calling: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElevatorSavePayload {
    pub pos: [i32; 3],
    pub short_name: String,
    pub long_name: String,
    pub door_control: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortSlotWire {
    pub item_id: String,
    pub name: String,
    pub count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortOpenPayload {
    pub pos: [i32; 3],
    pub is_frogport: bool,
    pub block_id: String,
    pub address: String,
    pub accepts_packages: bool,
    pub target_desc: String,
    pub slots: Vec<PortSlotWire>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortSavePayload {
    pub pos: [i32; 3],
    pub address: String,
    pub accepts_packages: bool,
    pub take_all: bool,
}

pub fn encode_ec_open(p: &ElevatorOpenPayload) -> String {
    format!(
        "ec1{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}",
        p.pos[0],
        p.pos[1],
        p.pos[2],
        clean_wire(&p.short_name),
        clean_wire(&p.long_name),
        p.door_control,
        u8::from(p.calling),
        s = SEP,
    )
}

pub fn decode_ec_open(text: &str) -> Option<ElevatorOpenPayload> {
    let mut it = text.split(SEP);
    if it.next()? != "ec1" {
        return None;
    }
    Some(ElevatorOpenPayload {
        pos: [it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?],
        short_name: it.next()?.to_owned(),
        long_name: it.next()?.to_owned(),
        door_control: it.next()?.parse().ok()?,
        calling: it.next()? == "1",
    })
}

pub fn encode_ec_save(p: &ElevatorSavePayload) -> String {
    format!(
        "ecs1{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}",
        p.pos[0],
        p.pos[1],
        p.pos[2],
        clean_wire(&p.short_name),
        clean_wire(&p.long_name),
        p.door_control,
        s = SEP,
    )
}

pub fn decode_ec_save(text: &str) -> Option<ElevatorSavePayload> {
    let mut it = text.split(SEP);
    if it.next()? != "ecs1" {
        return None;
    }
    Some(ElevatorSavePayload {
        pos: [it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?],
        short_name: it.next()?.to_owned(),
        long_name: it.next()?.to_owned(),
        door_control: it.next()?.parse().ok()?,
    })
}

pub fn encode_pp_open(p: &PortOpenPayload) -> String {
    let slots = p
        .slots
        .iter()
        .map(|s| format!("{}{FIELD_SEP}{}{FIELD_SEP}{}", clean_f(&s.item_id), clean_f(&s.name), s.count))
        .collect::<Vec<_>>()
        .join(&SUB_SEP.to_string());
    format!(
        "pp1{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}{s}{slots}",
        p.pos[0],
        p.pos[1],
        p.pos[2],
        u8::from(p.is_frogport),
        clean_wire(&p.block_id),
        clean_wire(&p.address),
        u8::from(p.accepts_packages),
        clean_wire(&p.target_desc),
        s = SEP,
    )
}

pub fn decode_pp_open(text: &str) -> Option<PortOpenPayload> {
    let mut it = text.split(SEP);
    if it.next()? != "pp1" {
        return None;
    }
    let pos = [it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?];
    let is_frogport = it.next()? == "1";
    let block_id = it.next()?.to_owned();
    let address = it.next()?.to_owned();
    let accepts_packages = it.next()? == "1";
    let target_desc = it.next()?.to_owned();
    let raw_slots = it.next()?;
    let slots = if raw_slots.is_empty() {
        Vec::new()
    } else {
        raw_slots
            .split(SUB_SEP)
            .filter(|r| !r.is_empty())
            .filter_map(|rec| {
                let mut f = rec.splitn(3, FIELD_SEP);
                Some(PortSlotWire {
                    item_id: f.next()?.to_owned(),
                    name: f.next()?.to_owned(),
                    count: f.next()?.parse().ok()?,
                })
            })
            .collect()
    };
    Some(PortOpenPayload {
        pos,
        is_frogport,
        block_id,
        address,
        accepts_packages,
        target_desc,
        slots,
    })
}

pub fn encode_pp_save(p: &PortSavePayload) -> String {
    format!(
        "pps1{s}{}{s}{}{s}{}{s}{}{s}{}{s}{}",
        p.pos[0],
        p.pos[1],
        p.pos[2],
        clean_wire(&p.address),
        u8::from(p.accepts_packages),
        u8::from(p.take_all),
        s = SEP,
    )
}

pub fn decode_pp_save(text: &str) -> Option<PortSavePayload> {
    let mut it = text.split(SEP);
    if it.next()? != "pps1" {
        return None;
    }
    Some(PortSavePayload {
        pos: [it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?],
        address: it.next()?.to_owned(),
        accepts_packages: it.next()? == "1",
        take_all: it.next()? == "1",
    })
}

enum Mode {
    Elevator {
        pos: [i32; 3],
        short_field: TextField,
        long_field: TextField,
        door_control: u8,
        calling: bool,
    },
    Port {
        data: PortOpenPayload,
        address: TextField,
        accepts_packages: bool,
    },
}

struct Open {
    mode: Mode,
    size: [i32; 2],
    close: bool,
}

enum OutMsg {
    Ec(ElevatorSavePayload),
    Pp(PortSavePayload),
}

static CLIENT: Mutex<Option<Open>> = Mutex::new(None);
static OUTBOX: Mutex<Vec<OutMsg>> = Mutex::new(Vec::new());

fn client() -> std::sync::MutexGuard<'static, Option<Open>> {
    CLIENT.lock().unwrap_or_else(PoisonError::into_inner)
}

fn send_out(msg: OutMsg) {
    OUTBOX.lock().unwrap_or_else(PoisonError::into_inner).push(msg);
}

fn save_msg(mode: &Mode, take_all: bool) -> OutMsg {
    match mode {
        Mode::Elevator { pos, short_field, long_field, door_control, .. } => OutMsg::Ec(ElevatorSavePayload {
            pos: *pos,
            short_name: short_field.value(),
            long_name: long_field.value(),
            door_control: *door_control,
        }),
        Mode::Port { data, address, accepts_packages } => OutMsg::Pp(PortSavePayload {
            pos: data.pos,
            address: address.value(),
            accepts_packages: *accepts_packages,
            take_all,
        }),
    }
}

impl Hooks for Screen {
    fn net_from_server(&self, host: &mut dyn Host, event: &mut NetFromServerEvent<'_>) {
        let Ok(text) = std::str::from_utf8(event.data) else { return };
        match event.channel {
            EC_OPEN_CHANNEL => {
                let Some(p) = decode_ec_open(text) else { return };
                let mut short_field = TextField::new(4);
                short_field.set(&p.short_name);
                let mut long_field = TextField::new(40);
                long_field.set(&p.long_name);
                if gui::open(host, TAG) {
                    *client() = Some(Open {
                        mode: Mode::Elevator {
                            pos: p.pos,
                            short_field,
                            long_field,
                            door_control: p.door_control,
                            calling: p.calling,
                        },
                        size: [0, 0],
                        close: false,
                    });
                }
            }
            PP_OPEN_CHANNEL => {
                let Some(p) = decode_pp_open(text) else { return };
                let mut address = TextField::new(48);
                address.set(&p.address);
                let accepts = p.accepts_packages;
                if gui::open(host, TAG) {
                    *client() = Some(Open {
                        mode: Mode::Port { data: p, address, accepts_packages: accepts },
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
                OutMsg::Ec(p) => {
                    let _ = host.net_send_to_server(EC_SAVE_CHANNEL, encode_ec_save(&p).as_bytes());
                }
                OutMsg::Pp(p) => {
                    let _ = host.net_send_to_server(PP_SAVE_CHANNEL, encode_pp_save(&p).as_bytes());
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
            Mode::Elevator { short_field, long_field, door_control, .. } => {
                let short_r = UiRect::new(left + 110, top + 46, 68, 20);
                let long_r = UiRect::new(left + 110, top + 78, 206, 20);
                if short_r.contains(mouse) {
                    short_field.focus(true);
                    long_field.focus(false);
                    return Flow::Cancel;
                }
                if long_r.contains(mouse) {
                    short_field.focus(false);
                    long_field.focus(true);
                    return Flow::Cancel;
                }
                short_field.focus(false);
                long_field.focus(false);

                let door_prev = UiRect::new(left + 110, top + 112, 24, 20);
                let door_next = UiRect::new(left + 292, top + 112, 24, 20);
                let cancel = UiRect::new(left + 194, top + 174, 60, 22);
                let save = UiRect::new(left + 260, top + 174, 60, 22);
                if door_prev.contains(mouse) {
                    *door_control = (*door_control + 5) % 6;
                    return Flow::Cancel;
                }
                if door_next.contains(mouse) {
                    *door_control = (*door_control + 1) % 6;
                    return Flow::Cancel;
                }
                if cancel.contains(mouse) {
                    open.close = true;
                    return Flow::Cancel;
                }
                if save.contains(mouse) {
                    send_out(save_msg(&open.mode, false));
                    open.close = true;
                }
            }
            Mode::Port { data, address, accepts_packages } => {
                let addr_r = UiRect::new(left + 84, top + 124, 236, 18);
                if addr_r.contains(mouse) {
                    address.focus(true);
                    return Flow::Cancel;
                }
                address.focus(false);

                let acc_btn = UiRect::new(left + 12, top + 150, 148, 20);
                let take_btn = UiRect::new(left + 166, top + 150, 154, 20);
                let cancel = UiRect::new(left + 194, top + 178, 60, 20);
                let save = UiRect::new(left + 260, top + 178, 60, 20);
                if data.is_frogport && acc_btn.contains(mouse) {
                    *accepts_packages = !*accepts_packages;
                    return Flow::Cancel;
                }
                if take_btn.contains(mouse) && data.slots.iter().any(|s| s.count > 0) {
                    send_out(save_msg(&open.mode, true));
                    open.close = true;
                    return Flow::Cancel;
                }
                if cancel.contains(mouse) {
                    open.close = true;
                    return Flow::Cancel;
                }
                if save.contains(mouse) {
                    send_out(save_msg(&open.mode, false));
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
        match &mut open.mode {
            Mode::Elevator { short_field, long_field, .. } => {
                if short_field.focused {
                    short_field.insert(event.c);
                } else if long_field.focused {
                    long_field.insert(event.c);
                }
            }
            Mode::Port { address, .. } => {
                if address.focused {
                    address.insert(event.c);
                }
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
        let fields: Vec<&mut TextField> = match &mut open.mode {
            Mode::Elevator { short_field, long_field, .. } => vec![short_field, long_field],
            Mode::Port { address, .. } => vec![address],
        };
        for f in fields {
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
            send_out(save_msg(&open.mode, false));
            open.close = true;
            return Flow::Cancel;
        }
        Flow::Cancel
    }
}

fn door_label(code: u8) -> &'static str {
    match code {
        0 => "Open All Doors",
        1 => "Open Facing North",
        2 => "Open Facing East",
        3 => "Open Facing South",
        4 => "Open Facing West",
        _ => "Keep Doors Closed",
    }
}

fn draw(host: &dyn Host, event: &mut ScreenRenderPostEvent<'_>, open: &Open) {
    gui::fill(host, event, UiRect::new(0, 0, open.size[0], open.size[1]), gui::DIM);
    let (left, top) = gui::screen_origin(open.size, PANEL_W, PANEL_H);
    gui::panel(host, event, UiRect::new(left, top, PANEL_W, PANEL_H), FRAME_BRASS, PANEL_BRASS);

    let mut tip: Option<Vec<String>> = None;
    match &open.mode {
        Mode::Elevator { short_field, long_field, door_control, calling, .. } => {
            gui::header(
                host,
                event,
                UiRect::new(left + 2, top + 2, PANEL_W - 4, 22),
                Some("create:elevator_contact"),
                "Elevator Contact",
                Some(if *calling { "Active Floor" } else { "Standby" }),
            );
            host.text(event.batch, "Floor Code:", [(left + 14) as f32, (top + 52) as f32], TEXT_LIGHT, false);
            gui::text_field(host, event, UiRect::new(left + 110, top + 46, 68, 20), short_field, "e.g. G", true);

            host.text(event.batch, "Floor Name:", [(left + 14) as f32, (top + 84) as f32], TEXT_LIGHT, false);
            gui::text_field(host, event, UiRect::new(left + 110, top + 78, 206, 20), long_field, "Lobby / Storage", true);

            host.text(event.batch, "Door Mode:", [(left + 14) as f32, (top + 118) as f32], TEXT_LIGHT, false);
            gui::button(host, event, UiRect::new(left + 110, top + 112, 24, 20), "<", BtnStyle::Normal, true);
            gui::button(host, event, UiRect::new(left + 138, top + 112, 150, 20), door_label(*door_control), BtnStyle::ToggleOn, false);
            gui::button(host, event, UiRect::new(left + 292, top + 112, 24, 20), ">", BtnStyle::Normal, true);

            gui::button(host, event, UiRect::new(left + 194, top + 174, 60, 22), "Cancel", BtnStyle::Normal, true);
            gui::button(host, event, UiRect::new(left + 260, top + 174, 60, 22), "Save", BtnStyle::Primary, true);
        }
        Mode::Port { data, address, accepts_packages } => {
            let title = if data.is_frogport { "Package Frogport" } else { "Package Postbox" };
            gui::header(
                host,
                event,
                UiRect::new(left + 2, top + 2, PANEL_W - 4, 22),
                Some(&data.block_id),
                title,
                None,
            );
            host.text(
                event.batch,
                &gui::truncate(host, &data.target_desc, 308.0),
                [(left + 12) as f32, (top + 30) as f32],
                TEXT_MUTED,
                false,
            );

            for i in 0..18 {
                let r = UiRect::new(left + 14 + (i % 9) * 34, top + 46 + (i / 9) * 34, 30, 30);
                if let Some(slot) = data.slots.get(i as usize).filter(|s| s.count > 0 && !s.item_id.is_empty()) {
                    if gui::item_slot(host, event, r, Some(&slot.item_id), false) {
                        tip = Some(vec![format!("{} x{}", slot.name, slot.count)]);
                    }
                    host.text(event.batch, &slot.count.to_string(), [(r.x + 2) as f32, (r.y + 20) as f32], TEXT_GOLD, true);
                } else {
                    gui::item_slot(host, event, r, None, false);
                }
            }

            host.text(event.batch, "Address:", [(left + 12) as f32, (top + 129) as f32], TEXT_LIGHT, false);
            gui::text_field(host, event, UiRect::new(left + 84, top + 124, 236, 18), address, "Filter address...", true);

            if data.is_frogport {
                gui::button(
                    host,
                    event,
                    UiRect::new(left + 12, top + 150, 148, 20),
                    if *accepts_packages { "Accepts: ON" } else { "Accepts: OFF" },
                    if *accepts_packages { BtnStyle::ToggleOn } else { BtnStyle::Normal },
                    true,
                );
            }
            let filled = data.slots.iter().any(|s| s.count > 0);
            gui::button(host, event, UiRect::new(left + 166, top + 150, 154, 20), "Take All Packages", BtnStyle::Normal, filled);

            gui::button(host, event, UiRect::new(left + 194, top + 178, 60, 20), "Cancel", BtnStyle::Normal, true);
            gui::button(host, event, UiRect::new(left + 260, top + 178, 60, 20), "Save", BtnStyle::Primary, true);
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
    fn elevator_and_port_codecs_roundtrip() {
        let ec = ElevatorOpenPayload {
            pos: [12, 70, -8],
            short_name: "G".into(),
            long_name: "Ground Floor".into(),
            door_control: 2,
            calling: true,
        };
        assert_eq!(decode_ec_open(&encode_ec_open(&ec)), Some(ec));

        let ecs = ElevatorSavePayload {
            pos: [12, 70, -8],
            short_name: "2".into(),
            long_name: "Upper Deck".into(),
            door_control: 0,
        };
        assert_eq!(decode_ec_save(&encode_ec_save(&ecs)), Some(ecs));

        let pp = PortOpenPayload {
            pos: [-4, 65, 19],
            is_frogport: true,
            block_id: "create:package_frogport".into(),
            address: "Storage*".into(),
            accepts_packages: true,
            target_desc: "Chain conveyor at [1, 3, 0]".into(),
            slots: vec![PortSlotWire { item_id: "create:cardboard_package_12x12".into(), name: "Package".into(), count: 1 }],
        };
        assert_eq!(decode_pp_open(&encode_pp_open(&pp)), Some(pp));

        let pps = PortSavePayload {
            pos: [-4, 65, 19],
            address: "Smelter".into(),
            accepts_packages: false,
            take_all: true,
        };
        assert_eq!(decode_pp_save(&encode_pp_save(&pps)), Some(pps));
    }
}
