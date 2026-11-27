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

const VALUE_SETTINGS_SHEET: &str = "create:gui/value_settings";
const ICONS_SHEET: &str = "create:gui/icons";
const ICON_CARD_W: f32 = 22.0;
const ICON_CARD_H: f32 = 20.0;
const ICON_STRIDE: f32 = 24.0;
const ICON_ROW_Y_OFFSET: f32 = -2.0;

/// `(col, row)` 16x16 coordinates in `create:textures/gui/icons.png` matching Create's `AllIcons`.
fn option_icon_uv(key: &str) -> Option<(u8, u8)> {
    Some(match key {
        "create.contraptions.movement_mode.rotate_place" => (4, 2),
        "create.contraptions.movement_mode.rotate_place_returned" => (5, 2),
        "create.contraptions.movement_mode.rotate_never_place" => (6, 2),
        "create.generic.clockwise" => (3, 1),
        "create.generic.counter_clockwise" => (4, 9),
        "create.contraptions.roller_mode.tunnel_pave" => (7, 2),
        "create.contraptions.roller_mode.straight_fill" => (8, 2),
        "create.contraptions.roller_mode.wide_fill" => (9, 2),
        "create.tunnel.selection_mode.split" => (4, 3),
        "create.tunnel.selection_mode.forced_split" => (5, 3),
        "create.tunnel.selection_mode.round_robin" => (6, 3),
        "create.tunnel.selection_mode.forced_round_robin" => (7, 3),
        "create.tunnel.selection_mode.prefer_nearest" => (8, 3),
        "create.tunnel.selection_mode.randomize" => (9, 3),
        "create.tunnel.selection_mode.synchronize" => (10, 3),
        "create.mechanical_arm.selection_mode.round_robin" => (4, 4),
        "create.mechanical_arm.selection_mode.forced_round_robin" => (5, 4),
        "create.mechanical_arm.selection_mode.prefer_first" => (6, 4),
        _ => return None,
    })
}

fn icon_slot_rect(geom: &PanelGeometry, gui_top: f32, count: usize, index: usize) -> [f32; 4] {
    let total_w = if count == 0 {
        0.0
    } else {
        (count - 1) as f32 * ICON_STRIDE + ICON_CARD_W
    };
    let start_x = geom.left + (geom.bg_width - total_w) * 0.5;
    let x = start_x + index as f32 * ICON_STRIDE;
    let y = gui_top + ICON_ROW_Y_OFFSET;
    [x, y, ICON_CARD_W, ICON_CARD_H]
}

fn icon_index_at(geom: &PanelGeometry, gui_top: f32, count: usize, mouse: [f32; 2]) -> Option<i32> {
    (0..count)
        .find(|&idx| in_rect(mouse, icon_slot_rect(geom, gui_top, count, idx)))
        .map(|idx| idx as i32)
}

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
                if layout.board.icon_mode {
                    let words = |key: &str| say(host, key);
                    let geom = panel_geometry(host, open, &layout, &words);
                    let count = (layout.board.max_value + 1).max(1) as usize;
                    if let Some(idx) = icon_index_at(&geom, layout.gui_top as f32, count, mouse) {
                        open.selected = Settings { row: 0, value: idx };
                    }
                } else {
                    open.selected = layout.closest(mouse, host.has_shift_down());
                }
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
        if layout.board.icon_mode {
            let count = (layout.board.max_value + 1).max(1) as usize;
            if let Some(idx) = icon_index_at(&geom, layout.gui_top as f32, count, mouse) {
                open.selected = Settings { row: 0, value: idx };
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
            if !layout.board.icon_mode {
                open.selected = layout.closest(mouse, host.has_shift_down());
            }
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
    let additional = if icon { 52.0 } else { 33.0 } + TOUCH_BAR_H + preset_extra;
    let icon_row_min = if icon {
        let count = (b.max_value + 1).max(1) as f32;
        count * ICON_STRIDE + 16.0
    } else {
        0.0
    };
    let min_width = if is_kinetic { 236.0 } else { 132.0_f32.max(icon_row_min) };
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

fn tile_sprite_x(
    host: &dyn Host,
    event: &mut ScreenRenderPostEvent<'_>,
    sheet: &str,
    u: f32,
    v: f32,
    tile_w: f32,
    src_h: f32,
    dst: [f32; 4],
) -> bool {
    let mut drawn_x = 0.0;
    while drawn_x < dst[2] {
        let seg_w = (dst[2] - drawn_x).min(tile_w);
        if seg_w <= 0.0 {
            break;
        }
        if !host.sprite(event.batch, sheet, [u, v, seg_w, src_h], [dst[0] + drawn_x, dst[1], seg_w, dst[3]]) {
            return false;
        }
        drawn_x += seg_w;
    }
    true
}

/// `ValueSettingsScreen.renderWindow` using Create's original `value_settings.png` and `icons.png` sheets
/// (with flat rectangle fallback when sprite assets are absent), plus quick RPM presets and touch controls.
fn draw(host: &dyn Host, event: &mut ScreenRenderPostEvent<'_>, open: &Open, layout: &Layout) {
    let words = |key: &str| say(host, key);
    let (w, h) = (event.batch.width, event.batch.height);
    rect(host, event, 0.0, 0.0, w, h, DIM);

    let b = &layout.board;
    let (x, y) = (layout.gui_left as f32, layout.gui_top as f32);
    let icon = b.icon_mode;
    let geom = panel_geometry(host, open, layout, &words);
    rect(host, event, geom.left - 1.0, geom.top - 1.0, geom.bg_width + 2.0, geom.bg_height + 2.0, 0xCC_14100C);
    rect(host, event, geom.left, geom.top, geom.bg_width, geom.bg_height, PANEL);
    rect(host, event, geom.left, geom.top, geom.bg_width, 1.0, 0xFF_6D5432);
    rect(host, event, geom.left, geom.top + geom.bg_height - 1.0, geom.bg_width, 1.0, 0xFF_6D5432);
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
        let icon_uv = match btn {
            TouchButton::Confirm => Some((0u8, 1u8)),
            TouchButton::Cancel => Some((1u8, 9u8)),
            TouchButton::Step(_, _) => None,
        };
        let drew_icon = icon_uv.is_some_and(|(col, row)| {
            let sz = 14.0_f32.min(bh - 2.0);
            let ix = bx + (bw - sz) * 0.5;
            let iy = by + (bh - sz) * 0.5;
            host.sprite(event.batch, ICONS_SHEET, [f32::from(col) * 16.0, f32::from(row) * 16.0, 16.0, 16.0], [ix, iy, sz, sz])
        });
        if !drew_icon {
            let lbl = btn.label();
            let tw = host.text_width(lbl);
            host.text(event.batch, lbl, [bx + (bw - tw) / 2.0, by + (bh - 8.0) / 2.0], 0xFF_FFFF, false);
        }
    }

    if icon {
        let count = (b.max_value + 1).max(1) as usize;
        for idx in 0..count {
            let slot = icon_slot_rect(&geom, y, count, idx);
            let is_selected = open.selected.value == idx as i32;
            let is_current = open.current.value == idx as i32;
            if is_selected {
                rect(host, event, slot[0] - 1.0, slot[1] - 1.0, slot[2] + 2.0, slot[3] + 2.0, 0xFF_F5D27A);
            } else if is_current {
                rect(host, event, slot[0] - 1.0, slot[1] - 1.0, slot[2] + 2.0, slot[3] + 2.0, 0xAA_FFFFFF);
            }
            if !host.sprite(event.batch, VALUE_SETTINGS_SHEET, [0.0, 44.0, 22.0, 20.0], slot) {
                rect(host, event, slot[0], slot[1], slot[2], slot[3], if is_selected { CURSOR } else { LABEL_BG });
            }
            let key = match open.behaviour {
                Behaviour::Options { options } => options.get(idx).copied().unwrap_or(""),
                _ => "",
            };
            let drew_opt_icon = option_icon_uv(key).is_some_and(|(col, row)| {
                host.sprite(
                    event.batch,
                    ICONS_SHEET,
                    [f32::from(col) * 16.0, f32::from(row) * 16.0, 16.0, 16.0],
                    [slot[0] + 3.0, slot[1] + 2.0, 16.0, 16.0],
                )
            });
            if !drew_opt_icon {
                let short = idx.to_string();
                let tw = host.text_width(&short);
                host.text(event.batch, &short, [slot[0] + (slot[2] - tw) * 0.5, slot[1] + 6.0], INK, false);
            }
        }
        let text = board::format(open.behaviour, open.max_stack, open.selected, &words);
        host.text(
            event.batch,
            &text,
            [geom.left + (geom.bg_width - host.text_width(&text)) / 2.0, y + ICON_CARD_H + 3.0],
            0xFB_DC7D,
            false,
        );
        return;
    }

    let rows = b.rows.len() as f32;
    let bar_x = x + layout.max_label_width as f32 + 14.0;
    let bar_w = layout.value_bar_width as f32;
    rect(host, event, bar_x, y - 3.0, bar_w + 8.0, rows * 11.0 + 5.0, FRAME);
    rect(host, event, bar_x + 1.0, y - 2.0, bar_w + 6.0, rows * 11.0 + 3.0, 0xFF_382A1E);
    rect(host, event, bar_x + 3.0, y, bar_w + 2.0, rows * 11.0 - 1.0, BAR_BG);

    let milestones = b.max_value / b.milestone_interval + 1;
    let first = bar_x + 4.0;
    for (i, label) in b.rows.iter().enumerate() {
        let row_y = y + i as f32 * 11.0;
        let label_dst = [x - 4.0, row_y, layout.max_label_width as f32 + 8.0, 11.0];
        if !host.sprite(
            event.batch,
            VALUE_SETTINGS_SHEET,
            [0.0, 31.0, label_dst[2].min(161.0), 11.0],
            label_dst,
        ) {
            rect(host, event, label_dst[0], label_dst[1], label_dst[2], label_dst[3], LABEL_BG);
        }
        let track_dst = [first, row_y + 1.0, bar_w, 8.0];
        if !tile_sprite_x(host, event, VALUE_SETTINGS_SHEET, 0.0, 0.0, 256.0, 8.0, track_dst) {
            rect(host, event, track_dst[0], track_dst[1], track_dst[2], track_dst[3], BAR);
        }
        let brass_dst = [first, row_y + 2.0, bar_w, 6.0];
        let _ = tile_sprite_x(host, event, VALUE_SETTINGS_SHEET, 0.0, 24.0, 256.0, 6.0, brass_dst);
        host.text(event.batch, label, [x, row_y + 1.0], INK, false);

        let mut at = first;
        for _ in 0..milestones {
            let m_w = layout.milestone_size as f32;
            if !host.sprite(event.batch, VALUE_SETTINGS_SHEET, [75.0, 9.0, 4.0, 4.0], [at, row_y + 3.0, m_w.min(4.0), 4.0]) {
                rect(host, event, at, row_y + 1.0, m_w, 8.0, MILESTONE);
            }
            at += (layout.milestone_size + b.milestone_interval * layout.scale) as f32;
        }
        // The value the box holds now (`ValueSettingsScreen` current marker at (65, 9, 9, 14)).
        if i as i32 == open.current.row {
            let [cx, _] = layout.coordinate_of(open.current.row, open.current.value);
            if !host.sprite(event.batch, VALUE_SETTINGS_SHEET, [65.0, 9.0, 9.0, 14.0], [cx - 4.5, row_y - 2.0, 9.0, 14.0]) {
                rect(host, event, cx - 0.5, row_y + 1.0, 1.0, 8.0, 0xFF_FFFFFF);
            }
        }
    }

    let [cx, cy] = layout.coordinate_of(open.selected.row, open.selected.value);
    let text = board::format(open.behaviour, open.max_stack, open.selected, &words);
    let width = (host.text_width(&text) / 2.0).floor() * 2.0 + 4.0;
    let (cursor_x, cursor_y) = (cx.floor() - (width / 2.0).floor(), cy.floor() - 7.0);
    let drew_pill = host.sprite(event.batch, VALUE_SETTINGS_SHEET, [0.0, 9.0, 3.0, 14.0], [cursor_x - 3.0, cursor_y, 3.0, 14.0])
        && tile_sprite_x(host, event, VALUE_SETTINGS_SHEET, 4.0, 9.0, 56.0, 14.0, [cursor_x, cursor_y, width, 14.0])
        && host.sprite(event.batch, VALUE_SETTINGS_SHEET, [61.0, 9.0, 3.0, 14.0], [cursor_x + width, cursor_y, 3.0, 14.0]);
    if !drew_pill {
        rect(host, event, cursor_x - 3.0, cursor_y, width + 6.0, 14.0, CURSOR);
    }
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

    #[test]
    fn option_icon_uv_covers_bearing_windmill_roller_tunnel_and_arm() {
        for key in table::ROTATION_MODES
            .iter()
            .chain(table::WINDMILL_DIRECTIONS)
            .chain(table::TUNNEL_MODES)
            .chain(table::ARM_MODES)
            .chain(create_kinetics::rules::roller::MODE_KEYS.iter())
        {
            assert!(option_icon_uv(key).is_some(), "missing icon uv for {key}");
        }
    }

    #[test]
    fn icon_slots_lay_out_and_hit_test_without_overlap() {
        let geom = PanelGeometry {
            left: 100.0,
            top: 40.0,
            bg_width: 120.0,
            bg_height: 80.0,
            tip: String::new(),
            tip_y: 90.0,
            presets_y: 90.0,
            buttons_y: 95.0,
        };
        let r0 = icon_slot_rect(&geom, 60.0, 3, 0);
        let r1 = icon_slot_rect(&geom, 60.0, 3, 1);
        let r2 = icon_slot_rect(&geom, 60.0, 3, 2);
        assert!(r0[0] + r0[2] < r1[0]);
        assert!(r1[0] + r1[2] < r2[0]);
        assert_eq!(icon_index_at(&geom, 60.0, 3, [r0[0] + 5.0, r0[1] + 5.0]), Some(0));
        assert_eq!(icon_index_at(&geom, 60.0, 3, [r1[0] + 10.0, r1[1] + 5.0]), Some(1));
        assert_eq!(icon_index_at(&geom, 60.0, 3, [r2[0] + 10.0, r2[1] + 5.0]), Some(2));
        assert_eq!(icon_index_at(&geom, 60.0, 3, [r0[0] + r0[2] + 1.0, r0[1] + 5.0]), None);
    }
}
