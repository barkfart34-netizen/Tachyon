//! The client's half of using a value box: holding the right button on a box, and the value board that opens
//! (`ValueSettingsClient`, `ValueSettingsScreen`, `ValueSettingsPacket`). The box looked at, what a held click
//! decides and the layout of the board are rules (`create_kinetics::rules::value_box*`); this reads the buttons and
//! the pointer, draws the board on a mod screen of its own (`Host::mod_screen_open`) and sends the answer to the
//! server (`create:value_box`).
//!
//! Known differences from Create, for the README (the board's look is `rules::value_box_board`'s note):
//! - Horizon can't move the pointer, so the board doesn't open with the pointer on the box's current value: the
//!   current value is marked on the bar and stays selected until the pointer moves (or the wheel is turned); then
//!   the value under the pointer is the one set.
//! - A synthetic click from the agent tools doesn't reach the `key` hook that starts the hold, so a box with a board
//!   can only be tried with a real mouse.

use std::sync::{Mutex, PoisonError};

use create_kinetics::rules::value_box::Props;
use create_kinetics::rules::value_box_blocks::{self as table, BoxDef, Kind};
use create_kinetics::rules::value_box_board::{self as board, Behaviour, Layout, Settings};
use create_kinetics::rules::value_box_input::{self as input, Held, Hold, Message};
use create_kinetics::{Dir, Pos};
use crate::compat::{
    ClientTickPostEvent, Flow, Hooks, Host, KeyEvent, NativeScreenKind, NetFromServerEvent,
    ScreenClosingEvent, ScreenKeyPressedPreEvent, ScreenMouseButtonPressedPreEvent,
    ScreenMouseButtonReleasedPreEvent, ScreenMouseScrolledPreEvent, ScreenRenderPostEvent,
};

use super::value_box as vb;
use super::value_box_draw::{looked_at, shown, Shown};

/// The mod's own screens belong to mod `create` (Ponder's too: only one is open at a time).
const MOD: &str = "create";
pub const OPEN_CHANNEL: &str = "create:value_box_open";
/// `GLFW_MOUSE_BUTTON_LEFT` / `GLFW_MOUSE_BUTTON_RIGHT` / `GLFW_KEY_ESCAPE` / `GLFW_KEY_ENTER`.
const LEFT: i32 = 0;
const RIGHT: i32 = 1;
const TOUCH_BAR_H: f32 = 20.0;
const PRESET_BAR_H: f32 = 20.0;
const KINETIC_PRESETS: [i32; 7] = [-256, -64, -16, 1, 16, 64, 256];

pub fn encode_open(pos: Pos, net_id: u8, side: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(14);
    for c in pos {
        out.extend_from_slice(&c.to_le_bytes());
    }
    out.push(net_id);
    out.push(side);
    out
}

pub fn decode_open(data: &[u8]) -> Option<(Pos, u8, u8)> {
    if data.len() < 14 {
        return None;
    }
    let pos = [
        i32::from_le_bytes(data[0..4].try_into().ok()?),
        i32::from_le_bytes(data[4..8].try_into().ok()?),
        i32::from_le_bytes(data[8..12].try_into().ok()?),
    ];
    Some((pos, data[12], data[13]))
}

struct Screen;

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Screen)
}

/// The box a hold or an open board is about.
#[derive(Clone)]
struct Target {
    pos: Pos,
    side: Dir,
    def: &'static BoxDef,
}

impl Target {
    fn side_id(&self) -> u8 {
        Dir::ALL.iter().position(|d| *d == self.side).unwrap_or(1) as u8
    }

    fn same_as(&self, other: &Shown, pos: Pos, side: Dir) -> bool {
        self.pos == pos && self.side == side && std::ptr::eq(self.def, other.def)
    }
}

/// The open value board.
struct Open {
    target: Target,
    behaviour: Behaviour,
    max_stack: i32,
    title: String,
    layout: Option<Layout>,
    /// What the box holds now.
    current: Settings,
    /// What would be set if the button were let go.
    selected: Settings,
    /// Whether the pointer chooses (it moved since the board opened or the wheel last turned).
    pointer_drives: bool,
    last_mouse: Option<[f32; 2]>,
    /// Opened by touch / sticky mode: lifting the button keeps the board open until `[OK]` or `[X]`.
    sticky: bool,
    /// The right button was let go (or `[OK]`/`[X]` pressed): the board closes on the next tick.
    closing: bool,
}

#[derive(Default)]
struct Client {
    right_down: bool,
    hold: Option<(Hold, Target)>,
    open: Option<Open>,
    outbox: Vec<Message>,
}

static CLIENT: Mutex<Client> = Mutex::new(Client {
    right_down: false,
    hold: None,
    open: None,
    outbox: Vec::new(),
});

fn client() -> std::sync::MutexGuard<'static, Client> {
    CLIENT.lock().unwrap_or_else(PoisonError::into_inner)
}

fn say(host: &dyn Host, key: &str) -> String {
    super::goggles::lang(host).translate(key, &[])
}

/// The box with a board that the crosshair is on, if any (`ValueSettingsInputHandler`'s first hit that takes a value).
fn board_target(host: &dyn Host) -> Option<(Target, Shown)> {
    let looked = looked_at(host)?;
    let item = shown(host, &looked).into_iter().find(|s| s.hovered)?;
    let takes = match item.def.kind {
        Kind::Value { .. } => true,
        Kind::Filter { .. } => item.counts,
    };
    takes.then(|| (Target { pos: looked.pos, side: looked.hit.side, def: item.def }, item))
}

fn playing(host: &dyn Host) -> bool {
    host.client_state().is_some_and(|client| client.screen == NativeScreenKind::Playing)
}

fn open_board(
    host: &mut dyn Host,
    target: Target,
    behaviour: Behaviour,
    values: board::Values,
    sticky: bool,
) {
    if host.mod_screen() == Some(MOD)
        && client()
            .open
            .as_ref()
            .is_some_and(|open| open.target.pos == target.pos && std::ptr::eq(open.target.def, target.def))
    {
        return;
    }
    let current = board::settings_of(behaviour, &values);
    let title = match target.def.kind {
        Kind::Filter { .. } => say(host, "create.logistics.filter.extracted_amount"),
        Kind::Value { .. } => say(host, target.def.label()),
    };
    super::create_gui::clear_active();
    if host.mod_screen_open(MOD) {
        client().open = Some(Open {
            target,
            behaviour,
            max_stack: values.max_stack,
            title,
            layout: None,
            current,
            selected: current,
            pointer_drives: false,
            last_mouse: None,
            sticky,
            closing: false,
        });
    }
}

impl Hooks for Screen {
    /// `ValueSettingsInputHandler` + `startInteractionWith`: a right click on a box with a board starts the hold.
    fn key(&self, host: &mut dyn Host, event: &mut KeyEvent<'_>) {
        if !host.window() || event.key != "key.mouse.right" {
            return;
        }
        let mut c = client();
        c.right_down = event.pressed;
        if !event.pressed || c.hold.is_some() || !playing(&*host) || host.sneaking() {
            return;
        }
        drop(c);
        if let Some((target, _)) = board_target(&*host) {
            client().hold = Some((Hold::start(), target));
        }
    }

    /// `ValueSettingsClient.tick`: a short click, or the board after six ticks.
    fn client_tick_post(&self, host: &mut dyn Host, _: &mut ClientTickPostEvent) {
        if !host.window() {
            return;
        }
        // What the board's hooks, which have no `&mut Host`, asked for.
        let (messages, close) = {
            let mut c = client();
            let close = c.open.as_ref().is_some_and(|open| open.closing);
            (std::mem::take(&mut c.outbox), close)
        };
        for message in messages {
            let _ = host.net_send_to_server(input::CHANNEL, &message.encode());
        }
        if close && host.mod_screen() == Some(MOD) {
            host.mod_screen_close(MOD);
        }
        let Some((mut hold, target)) = client().hold.take() else {
            return;
        };
        let down = client().right_down;
        let now = board_target(&*host);
        let valid = !host.sneaking()
            && now.as_ref().is_some_and(|(t, item)| target.same_as(item, t.pos, t.side));
        let outcome = hold.tick(valid, down);
        let short_open = matches!(outcome, Held::Short) && matches!(target.def.kind, Kind::Value { .. });
        let outcome = if short_open { Held::Open } else { outcome };
        match outcome {
            Held::Waiting => client().hold = Some((hold, target)),
            Held::Cancelled => {}
            Held::Short => {
                let message = Message::Short { pos: target.pos, net_id: target.def.net_id, side: target.side_id() };
                let _ = host.net_send_to_server(input::CHANNEL, &message.encode());
            }
            Held::Open => {
                let Some((_, item)) = now else {
                    return;
                };
                let Some(behaviour) = target.def.behaviour() else {
                    return;
                };
                open_board(
                    host,
                    target,
                    behaviour,
                    item.values,
                    short_open || cfg!(target_os = "android"),
                );
            }
        }
    }

    fn net_from_server(&self, host: &mut dyn Host, event: &mut NetFromServerEvent<'_>) {
        if !host.window() || event.channel != OPEN_CHANNEL {
            return;
        }
        let Some((pos, net_id, side_id)) = decode_open(event.data) else {
            return;
        };
        let Some(&side) = Dir::ALL.get(usize::from(side_id)) else {
            return;
        };
        let block = host.block(pos);
        let found = vb::with_props(&*host, pos, &block, |p: &Props, nbt| {
            let def = table::boxes(&block)
                .iter()
                .find(|d| d.net_id == net_id && (d.active)(p) && d.slot.exists(p, side))?;
            let behaviour = def.behaviour()?;
            let values = nbt.map_or_else(board::Values::default, |n| vb::values_of(&*host, n, def, side));
            Some((def, behaviour, values))
        });
        let Some((def, behaviour, values)) = found else {
            return;
        };
        open_board(host, Target { pos, side, def }, behaviour, values, true);
    }

    /// `ValueSettingsScreen.renderWindow`.
    fn screen_render_post(&self, host: &dyn Host, event: &mut ScreenRenderPostEvent<'_>) {
        if event.screen.id != "mod" || host.mod_screen() != Some(MOD) {
            return;
        }
        let mut c = client();
        let Some(open) = c.open.as_mut() else {
            return;
        };
        let (w, h) = (event.batch.width, event.batch.height);
        let words = |key: &str| say(host, key);
        let board_spec = board::board(open.behaviour, open.max_stack, &words);
        let label_width = board_spec.rows.iter().map(|row| host.text_width(row) as i32).max().unwrap_or(0);
        if open.layout.as_ref().is_none_or(|l| l.board != board_spec) {
            open.layout = Some(Layout::new(board_spec, label_width, [w as i32, h as i32]));
        }
        let layout = open.layout.clone().expect("just set");
        let mouse = event.screen.mouse;
        // On desktop hold-to-open, the pointer picks once moved; in touch sticky mode, taps drive instead.
        if !open.sticky {
            match open.last_mouse {
                None => open.last_mouse = Some(mouse),
                Some(last) if (mouse[0] - last[0]).abs() + (mouse[1] - last[1]).abs() > 1.0 => {
                    open.pointer_drives = true;
                    open.last_mouse = Some(mouse);
                }
                Some(_) => {}
            }
            if open.pointer_drives {
                open.selected = layout.closest(mouse, host.has_shift_down());
            }
        }
        draw(host, event, open, &layout);
    }

    /// Touch & pointer clicks on buttons or directly on the value bar.
    fn screen_mouse_button_pressed_pre(
        &self,
        host: &dyn Host,
        event: &mut ScreenMouseButtonPressedPreEvent<'_>,
    ) -> Flow {
        if event.screen.id != "mod" || host.mod_screen() != Some(MOD) || event.button != LEFT {
            return Flow::Continue;
        }
        let mut c = client();
        let Some(open) = c.open.as_mut() else {
            return Flow::Continue;
        };
        if open.closing {
            return Flow::Cancel;
        }
        let Some(layout) = open.layout.clone() else {
            return Flow::Cancel;
        };
        let words = |key: &str| say(host, key);
        let geom = panel_geometry(host, open, &layout, &words);
        let mouse = event.screen.mouse;
        for (btn, rect_box) in touch_buttons(&geom) {
            if in_rect(mouse, rect_box) {
                match btn {
                    TouchButton::Step(delta, milestone) => {
                        open.selected = layout.scrolled(open.selected, delta, milestone);
                        open.pointer_drives = false;
                        open.last_mouse = Some(mouse);
                    }
                    TouchButton::Confirm => {
                        let msg = confirm_message(open);
                        open.closing = true;
                        c.outbox.push(msg);
                    }
                    TouchButton::Cancel => {
                        open.closing = true;
                    }
                }
                return Flow::Cancel;
            }
        }
        for (rpm, rect_box) in preset_buttons(&geom, open.behaviour) {
            if in_rect(mouse, rect_box) {
                open.selected = Settings {
                    row: i32::from(rpm > 0),
                    value: rpm.abs(),
                };
                open.pointer_drives = false;
                open.last_mouse = Some(mouse);
                return Flow::Cancel;
            }
        }
        let bar_area = [
            geom.left,
            layout.gui_top as f32 - 3.0,
            geom.bg_width,
            layout.window_height as f32 + 5.0,
        ];
        if in_rect(mouse, bar_area) {
            open.selected = layout.closest(mouse, host.has_shift_down());
            open.pointer_drives = !open.sticky;
            open.last_mouse = Some(mouse);
            return Flow::Cancel;
        }
        if open.sticky && !in_rect(mouse, geom.panel_rect()) {
            open.closing = true;
        }
        Flow::Cancel
    }

    fn screen_key_pressed_pre(&self, host: &dyn Host, event: &mut ScreenKeyPressedPreEvent<'_>) -> Flow {
        if event.screen.id != "mod" || host.mod_screen() != Some(MOD) {
            return Flow::Continue;
        }
        let mut c = client();
        let Some(open) = c.open.as_mut() else {
            return Flow::Continue;
        };
        if event.key == "key.keyboard.escape" {
            open.closing = true;
            return Flow::Cancel;
        }
        if event.key == "key.keyboard.enter" || event.key == "key.keyboard.kp.enter" {
            let msg = confirm_message(open);
            open.closing = true;
            c.outbox.push(msg);
            return Flow::Cancel;
        }
        Flow::Continue
    }

    /// `ValueSettingsScreen.mouseReleased`: letting go of the right button sets the value under the pointer.
    fn screen_mouse_button_released_pre(&self, host: &dyn Host, event: &mut ScreenMouseButtonReleasedPreEvent<'_>) -> Flow {
        if event.screen.id != "mod" || host.mod_screen() != Some(MOD) || event.button != RIGHT {
            return Flow::Continue;
        }
        let mut c = client();
        let Some(open) = c.open.as_mut() else {
            return Flow::Continue;
        };
        if open.closing || open.sticky {
            return Flow::Cancel;
        }
        if let Some(layout) = &open.layout
            && open.pointer_drives
        {
            open.selected = layout.closest(event.screen.mouse, host.has_shift_down());
        }
        let message = confirm_message(open);
        open.closing = true;
        c.outbox.push(message);
        Flow::Cancel
    }

    /// `ValueSettingsScreen.mouseScrolled`: the wheel steps the value, Shift by the marked ones.
    fn screen_mouse_scrolled_pre(&self, host: &dyn Host, event: &mut ScreenMouseScrolledPreEvent<'_>) -> Flow {
        if event.screen.id != "mod" || host.mod_screen() != Some(MOD) {
            return Flow::Continue;
        }
        let mut c = client();
        let Some(open) = c.open.as_mut() else {
            return Flow::Continue;
        };
        if let Some(layout) = &open.layout {
            open.selected = layout.scrolled(open.selected, event.scroll[1], host.has_shift_down());
            open.pointer_drives = false;
            open.last_mouse = Some(event.screen.mouse);
        }
        Flow::Cancel
    }

    fn screen_closing(&self, _: &dyn Host, event: &mut ScreenClosingEvent<'_>) {
        if event.screen.id == "mod" {
            client().open = None;
        }
    }
}

// --- drawing the board ----------------------------------------------------------------------------------------

const DIM: u32 = 0x50_101010;
const PANEL: u32 = 0xF0_2B2118;
const FRAME: u32 = 0xFF_B8924A;
const BAR_BG: u32 = 0xFF_3A2C1E;
const BAR: u32 = 0xFF_8A6B3C;
const MILESTONE: u32 = 0xFF_E8D08A;
const LABEL_BG: u32 = 0xFF_D9B96A;
const CURSOR: u32 = 0xFF_F0D890;
const INK: u32 = 0x44_2000;
const BTN_BG: u32 = 0xFF_4C3A28;
const BTN_OK: u32 = 0xFF_3C7A3E;
const BTN_CANCEL: u32 = 0xFF_8C3838;

#[derive(Clone, Copy)]
enum TouchButton {
    Step(f64, bool),
    Confirm,
    Cancel,
}

impl TouchButton {
    fn label(self) -> &'static str {
        match self {
            TouchButton::Step(d, true) if d < 0.0 => "<<",
            TouchButton::Step(_, false) if matches!(self, TouchButton::Step(d, _) if d < 0.0) => "<",
            TouchButton::Step(_, false) => ">",
            TouchButton::Step(_, true) => ">>",
            TouchButton::Confirm => "OK",
            TouchButton::Cancel => "X",
        }
    }

    fn bg(self) -> u32 {
        match self {
            TouchButton::Confirm => BTN_OK,
            TouchButton::Cancel => BTN_CANCEL,
            TouchButton::Step(_, _) => BTN_BG,
        }
    }
}

struct PanelGeometry {
    left: f32,
    top: f32,
    bg_width: f32,
    bg_height: f32,
    tip: String,
    tip_y: f32,
    presets_y: f32,
    buttons_y: f32,
}

impl PanelGeometry {
    fn panel_rect(&self) -> [f32; 4] {
        [self.left, self.top, self.bg_width, self.bg_height]
    }
}

fn confirm_message(open: &Open) -> Message {
    Message::Set {
        pos: open.target.pos,
        net_id: open.target.def.net_id,
        side: open.target.side_id(),
        row: open.selected.row,
        value: open.selected.value,
    }
}

fn in_rect(pt: [f32; 2], r: [f32; 4]) -> bool {
    pt[0] >= r[0] && pt[0] <= r[0] + r[2] && pt[1] >= r[1] && pt[1] <= r[1] + r[3]
}

fn panel_geometry(host: &dyn Host, open: &Open, layout: &Layout, words: &dyn Fn(&str) -> String) -> PanelGeometry {
    let b = &layout.board;
    let (x, y) = (layout.gui_left as f32, layout.gui_top as f32);
    let icon = b.icon_mode;
    let is_kinetic = matches!(open.behaviour, Behaviour::Kinetic { .. });
    let tip = if open.sticky {
        words("create.gui.value_settings.release_to_confirm").replace("%1$s", "OK")
    } else {
        words("create.gui.value_settings.release_to_confirm").replace("%1$s", "Right Button")
    };
    let preset_extra = if is_kinetic { PRESET_BAR_H } else { 0.0 };
    let additional = if icon { 46.0 } else { 33.0 } + TOUCH_BAR_H + preset_extra;
    let min_width = if is_kinetic { 236.0 } else { 132.0 };
    let mut fattest = host
        .text_width(&tip)
        .max(host.text_width(&open.title))
        .max(min_width);
    if icon {
        for value in 0..=b.max_value {
            let text = board::format(open.behaviour, open.max_stack, Settings { row: 0, value }, words);
            fattest = fattest.max(host.text_width(&text));
        }
    }
    let bg_width = (layout.window_width as f32 + 13.0).max(fattest + 10.0);
    let fat_offset = ((fattest + 10.0 - (layout.window_width as f32 + 13.0)).max(0.0)) / 2.0;
    let left = x - 11.0 - fat_offset;
    let top = y - 17.0;
    let bg_height = layout.window_height as f32 + additional;
    let tip_y = top + bg_height - 10.0 - TOUCH_BAR_H - preset_extra;
    let presets_y = top + bg_height - TOUCH_BAR_H - preset_extra + 2.0;
    let buttons_y = top + bg_height - TOUCH_BAR_H + 2.0;
    PanelGeometry {
        left,
        top,
        bg_width,
        bg_height,
        tip,
        tip_y,
        presets_y,
        buttons_y,
    }
}

fn preset_buttons(geom: &PanelGeometry, behaviour: Behaviour) -> Vec<(i32, [f32; 4])> {
    if !matches!(behaviour, Behaviour::Kinetic { .. }) {
        return Vec::new();
    }
    let pad = 6.0;
    let gap = 3.0;
    let count = KINETIC_PRESETS.len() as f32;
    let usable = (geom.bg_width - pad * 2.0).max(140.0);
    let btn_w = (usable - gap * (count - 1.0)) / count;
    let btn_h = PRESET_BAR_H - 4.0;
    let start_x = geom.left + (geom.bg_width - (btn_w * count + gap * (count - 1.0))) / 2.0;
    KINETIC_PRESETS
        .iter()
        .enumerate()
        .map(|(i, &rpm)| {
            let bx = start_x + i as f32 * (btn_w + gap);
            (rpm, [bx, geom.presets_y, btn_w, btn_h])
        })
        .collect()
}

fn touch_buttons(geom: &PanelGeometry) -> [(TouchButton, [f32; 4]); 6] {
    let defs = [
        TouchButton::Step(-1.0, true),
        TouchButton::Step(-1.0, false),
        TouchButton::Step(1.0, false),
        TouchButton::Step(1.0, true),
        TouchButton::Confirm,
        TouchButton::Cancel,
    ];
    let pad = 6.0;
    let gap = 3.0;
    let usable = (geom.bg_width - pad * 2.0).max(96.0);
    let btn_w = (usable - gap * 5.0) / 6.0;
    let btn_h = TOUCH_BAR_H - 5.0;
    let start_x = geom.left + (geom.bg_width - (btn_w * 6.0 + gap * 5.0)) / 2.0;
    std::array::from_fn(|i| {
        let bx = start_x + i as f32 * (btn_w + gap);
        (defs[i], [bx, geom.buttons_y, btn_w, btn_h])
    })
}

fn rect(host: &dyn Host, event: &mut ScreenRenderPostEvent<'_>, x: f32, y: f32, w: f32, h: f32, argb: u32) {
    host.fill(event.batch, [x, y, w, h], argb);
}

/// `ValueSettingsScreen.renderWindow` in flat rectangles plus quick RPM presets and touch adjustment bar.
fn draw(host: &dyn Host, event: &mut ScreenRenderPostEvent<'_>, open: &Open, layout: &Layout) {
    let words = |key: &str| say(host, key);
    let (w, h) = (event.batch.width, event.batch.height);
    rect(host, event, 0.0, 0.0, w, h, DIM);

    let b = &layout.board;
    let (x, y) = (layout.gui_left as f32, layout.gui_top as f32);
    let icon = b.icon_mode;
    let geom = panel_geometry(host, open, layout, &words);
    rect(host, event, geom.left, geom.top, geom.bg_width, geom.bg_height, PANEL);
    let centre = geom.left + geom.bg_width / 2.0;
    host.text(event.batch, &open.title, [centre - host.text_width(&open.title) / 2.0, y - 14.0], 0xDD_DDDD, false);
    host.text(event.batch, &geom.tip, [centre - host.text_width(&geom.tip) / 2.0, geom.tip_y], 0xDD_DDDD, false);

    let selected_signed = if open.selected.row == 0 {
        -open.selected.value
    } else {
        open.selected.value
    };
    for (rpm, [bx, by, bw, bh]) in preset_buttons(&geom, open.behaviour) {
        let active = selected_signed == rpm;
        rect(host, event, bx, by, bw, bh, FRAME);
        rect(
            host,
            event,
            bx + 1.0,
            by + 1.0,
            (bw - 2.0).max(1.0),
            (bh - 2.0).max(1.0),
            if active { 0xFF_B07D2B } else { BTN_BG },
        );
        let lbl = if rpm > 1 { format!("+{rpm}") } else { rpm.to_string() };
        let tw = host.text_width(&lbl);
        host.text(event.batch, &lbl, [bx + (bw - tw) / 2.0, by + (bh - 8.0) / 2.0], 0xFF_FFFF, false);
    }

    for (btn, [bx, by, bw, bh]) in touch_buttons(&geom) {
        rect(host, event, bx, by, bw, bh, FRAME);
        rect(host, event, bx + 1.0, by + 1.0, (bw - 2.0).max(1.0), (bh - 2.0).max(1.0), btn.bg());
        let lbl = btn.label();
        let tw = host.text_width(lbl);
        host.text(event.batch, lbl, [bx + (bw - tw) / 2.0, by + (bh - 8.0) / 2.0], 0xFF_FFFF, false);
    }

    let rows = b.rows.len() as f32;
    let bar_x = x + layout.max_label_width as f32 + 14.0;
    rect(host, event, bar_x, y - 3.0, layout.value_bar_width as f32 + 8.0, rows * 11.0 + 5.0, FRAME);
    rect(host, event, bar_x + 3.0, y, layout.value_bar_width as f32 + 2.0, rows * 11.0 - 1.0, BAR_BG);

    let milestones = b.max_value / b.milestone_interval + 1;
    let first = bar_x + 4.0;
    for (i, label) in b.rows.iter().enumerate() {
        let row_y = y + i as f32 * 11.0;
        if !icon {
            rect(host, event, x - 4.0, row_y, layout.max_label_width as f32 + 8.0, 11.0, LABEL_BG);
            rect(host, event, first, row_y + 1.0, layout.value_bar_width as f32, 8.0, BAR);
            host.text(event.batch, label, [x, row_y + 1.0], INK, false);
        }
        let mut at = first;
        for _ in 0..milestones {
            rect(host, event, at, row_y + 1.0, layout.milestone_size as f32, 8.0, MILESTONE);
            at += (layout.milestone_size + b.milestone_interval * layout.scale) as f32;
        }
        // The value the box holds now.
        if i as i32 == open.current.row {
            let [cx, _] = layout.coordinate_of(open.current.row, open.current.value);
            rect(host, event, cx - 0.5, row_y + 1.0, 1.0, 8.0, 0xFF_FFFFFF);
        }
    }

    let [cx, cy] = layout.coordinate_of(open.selected.row, open.selected.value);
    let text = board::format(open.behaviour, open.max_stack, open.selected, &words);
    if icon {
        rect(host, event, cx - 4.0, cy - 4.0, 8.0, 8.0, CURSOR);
        host.text(
            event.batch,
            &text,
            [geom.left + (geom.bg_width - host.text_width(&text)) / 2.0, geom.tip_y - 13.0],
            0xFB_DC7D,
            false,
        );
        return;
    }
    let width = (host.text_width(&text) / 2.0).floor() * 2.0 + 3.0;
    let (cursor_x, cursor_y) = (cx.floor() - (width / 2.0).floor(), cy.floor() - 7.0);
    rect(host, event, cursor_x - 3.0, cursor_y, width + 6.0, 14.0, CURSOR);
    host.text(event.batch, &text, [cursor_x + 2.0, cursor_y + 3.0], INK, false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_packet_roundtrips() {
        let data = encode_open([12, 65, -30], 0, 3);
        assert_eq!(decode_open(&data), Some(([12, 65, -30], 0, 3)));
        assert_eq!(decode_open(&[1, 2, 3]), None);
    }
}
