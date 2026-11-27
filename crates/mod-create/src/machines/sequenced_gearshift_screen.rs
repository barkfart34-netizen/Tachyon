//! The client's custom screen for the Sequenced Gearshift (`SequencedGearshiftScreen`, `ConfigureSequencedGearshiftPacket`).
//! Displays all five instruction slots visually with desktop click/scroll and Android touch controls.

use std::sync::{Mutex, PoisonError};

use create_kinetics::rules::sequenced_gearshift::{self as rules, Instruction, Modifier, Op};
use crate::compat::{
    ClientTickPostEvent, Flow, Hooks, Host, NetFromServerEvent, ScreenClosingEvent,
    ScreenKeyPressedPreEvent, ScreenMouseButtonPressedPreEvent, ScreenMouseScrolledPreEvent,
    ScreenRenderPostEvent, Text,
};

use super::create_gui::{
    self as gui, clean_wire, BtnStyle, UiRect, FRAME_BRASS, PANEL_BRASS, ROW_ACTIVE, ROW_ALT,
    ROW_BG, ROW_DIM, TEXT_GOLD, TEXT_LIGHT, TEXT_MUTED,
};

pub const OPEN_CHANNEL: &str = "create:seq_open";
pub const SAVE_CHANNEL: &str = "create:seq_save";

const TAG: &str = "sequenced_gearshift";
const SEP: char = '\u{1f}';
const SUB_SEP: char = '\u{1e}';

const PANEL_W: i32 = 344;
const PANEL_H: i32 = 206;

struct Screen;

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Screen)
}

// --- wire protocol ----------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenPayload {
    pub pos: [i32; 3],
    pub current: i32,
    pub instructions: Vec<Instruction>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavePayload {
    pub pos: [i32; 3],
    pub instructions: Vec<Instruction>,
}

fn encode_instructions(instructions: &[Instruction]) -> String {
    rules::padded(instructions)
        .into_iter()
        .map(|ins| format!("{},{},{}", ins.op.name(), ins.value, ins.modifier.name()))
        .collect::<Vec<_>>()
        .join(&SUB_SEP.to_string())
}

fn decode_instructions(raw: &str) -> Option<Vec<Instruction>> {
    let mut out = Vec::new();
    for entry in raw.split(SUB_SEP) {
        if entry.is_empty() {
            continue;
        }
        let mut parts = entry.split(',');
        let op = Op::from_name(parts.next()?)?;
        let value: i32 = parts.next()?.parse().ok()?;
        let modifier = Modifier::from_name(parts.next()?).unwrap_or(Modifier::Forward);
        out.push(Instruction {
            op,
            modifier,
            value: if op.has_value() { value.clamp(1, op.max_value()) } else { 1 },
        });
    }
    Some(rules::padded(&out))
}

pub fn encode_open(payload: &OpenPayload) -> String {
    format!(
        "seq1{s}{}{s}{}{s}{}{s}{}{s}{}",
        payload.pos[0],
        payload.pos[1],
        payload.pos[2],
        payload.current,
        encode_instructions(&payload.instructions),
        s = SEP,
    )
}

pub fn decode_open(text: &str) -> Option<OpenPayload> {
    let mut it = text.split(SEP);
    if it.next()? != "seq1" {
        return None;
    }
    let pos = [it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?];
    let current = it.next()?.parse().ok()?;
    let instructions = decode_instructions(it.next()?)?;
    Some(OpenPayload { pos, current, instructions })
}

pub fn encode_save(payload: &SavePayload) -> String {
    let _ = clean_wire("");
    format!(
        "seqsave1{s}{}{s}{}{s}{}{s}{}",
        payload.pos[0],
        payload.pos[1],
        payload.pos[2],
        encode_instructions(&payload.instructions),
        s = SEP,
    )
}

pub fn decode_save(text: &str) -> Option<SavePayload> {
    let mut it = text.split(SEP);
    if it.next()? != "seqsave1" {
        return None;
    }
    let pos = [it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?];
    let instructions = decode_instructions(it.next()?)?;
    Some(SavePayload { pos, instructions })
}

// --- pure instruction helpers -----------------------------------------------------------------------------------

fn op_short_label(op: Op) -> &'static str {
    match op {
        Op::TurnAngle => "Turn Angle",
        Op::TurnDistance => "Turn Distance",
        Op::Delay => "Timed Delay",
        Op::Await => "Await Redstone",
        Op::End => "End",
    }
}

fn modifier_button_label(m: Modifier) -> &'static str {
    match m {
        Modifier::ForwardFast => ">> 2x",
        Modifier::Forward => "-> 1x",
        Modifier::Back => "<- -1x",
        Modifier::BackFast => "<< -2x",
    }
}

pub fn cycle_op(ins: &mut Instruction, forward: bool) {
    let idx = Op::ALL.iter().position(|op| *op == ins.op).unwrap_or(0);
    let len = Op::ALL.len();
    let next = Op::ALL[if forward { (idx + 1) % len } else { (idx + len - 1) % len }];
    ins.op = next;
    ins.value = if next.has_value() { next.default_value() } else { 1 };
}

pub fn cycle_modifier(ins: &mut Instruction, forward: bool) {
    if !ins.op.has_speed() {
        return;
    }
    let idx = Modifier::ALL.iter().position(|m| *m == ins.modifier).unwrap_or(1);
    let len = Modifier::ALL.len();
    ins.modifier = Modifier::ALL[if forward { (idx + 1) % len } else { (idx + len - 1) % len }];
}

pub fn step_value_preset(ins: &mut Instruction, forward: bool) {
    if !ins.op.has_value() {
        return;
    }
    let steps = ins.op.steps();
    if steps.is_empty() {
        return;
    }
    ins.value = if forward {
        steps.iter().copied().find(|v| *v > ins.value).unwrap_or(steps[0])
    } else {
        steps.iter().rev().copied().find(|v| *v < ins.value).unwrap_or(steps[steps.len() - 1])
    };
}

pub fn adjust_value(ins: &mut Instruction, forward: bool, fine_or_shift: bool, is_scroll: bool) {
    if !ins.op.has_value() {
        return;
    }
    let max = ins.op.max_value().max(1);
    if is_scroll {
        let delta = if fine_or_shift { ins.op.shift_step().max(1) } else { 1 };
        let next = if forward { ins.value + delta } else { ins.value - delta };
        ins.value = next.clamp(1, max);
    } else if fine_or_shift {
        let delta = if forward { 1 } else { -1 };
        ins.value = (ins.value + delta).clamp(1, max);
    } else {
        step_value_preset(ins, forward);
    }
}

// --- open screen state ------------------------------------------------------------------------------------------

struct Open {
    pos: [i32; 3],
    current: i32,
    instructions: Vec<Instruction>,
    size: [i32; 2],
    close: bool,
}

impl Open {
    fn from_payload(p: OpenPayload) -> Self {
        Self {
            pos: p.pos,
            current: p.current,
            instructions: rules::padded(&p.instructions),
            size: [0, 0],
            close: false,
        }
    }

    fn origin(&self) -> (i32, i32) {
        gui::screen_origin(self.size, PANEL_W, PANEL_H)
    }

    fn first_end(&self) -> usize {
        self.instructions
            .iter()
            .position(|ins| ins.op == Op::End)
            .unwrap_or(rules::MAX_INSTRUCTIONS)
    }

    fn save_and_close(&mut self) {
        send(SavePayload {
            pos: self.pos,
            instructions: self.instructions.clone(),
        });
        self.close = true;
    }
}

static CLIENT: Mutex<Option<Open>> = Mutex::new(None);
static OUTBOX: Mutex<Vec<SavePayload>> = Mutex::new(Vec::new());

fn client() -> std::sync::MutexGuard<'static, Option<Open>> {
    CLIENT.lock().unwrap_or_else(PoisonError::into_inner)
}

fn send(payload: SavePayload) {
    OUTBOX.lock().unwrap_or_else(PoisonError::into_inner).push(payload);
}

struct RowLayout {
    row: UiRect,
    op_btn: UiRect,
    minus_btn: UiRect,
    val_btn: UiRect,
    plus_btn: UiRect,
    speed_btn: UiRect,
}

fn row_layout(left: i32, top: i32, idx: usize) -> RowLayout {
    let row_y = top + 30 + idx as i32 * 28;
    RowLayout {
        row: UiRect::new(left + 6, row_y, PANEL_W - 12, 24),
        op_btn: UiRect::new(left + 28, row_y + 1, 140, 22),
        minus_btn: UiRect::new(left + 172, row_y + 1, 22, 22),
        val_btn: UiRect::new(left + 196, row_y + 1, 54, 22),
        plus_btn: UiRect::new(left + 252, row_y + 1, 22, 22),
        speed_btn: UiRect::new(left + 278, row_y + 1, 58, 22),
    }
}

fn footer_rects(left: i32, top: i32) -> (UiRect, UiRect, UiRect) {
    let y = top + 174;
    (
        UiRect::new(left + 8, y, 62, 22),
        UiRect::new(left + 206, y, 62, 22),
        UiRect::new(left + 274, y, 62, 22),
    )
}

impl Hooks for Screen {
    fn net_from_server(&self, host: &mut dyn Host, event: &mut NetFromServerEvent<'_>) {
        if event.channel != OPEN_CHANNEL {
            return;
        }
        let Ok(text) = std::str::from_utf8(event.data) else { return };
        let Some(payload) = decode_open(text) else { return };
        if gui::open(host, TAG) {
            *client() = Some(Open::from_payload(payload));
        }
    }

    fn client_tick_post(&self, host: &mut dyn Host, _: &mut ClientTickPostEvent) {
        if !host.window() {
            return;
        }
        let pending = std::mem::take(&mut *OUTBOX.lock().unwrap_or_else(PoisonError::into_inner));
        for msg in pending {
            let _ = host.net_send_to_server(SAVE_CHANNEL, encode_save(&msg).as_bytes());
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
        let (left, top) = open.origin();
        let mouse = event.screen.mouse;
        let forward = event.button != 1;
        let shift = host.has_shift_down();

        for i in 0..rules::MAX_INSTRUCTIONS {
            let rl = row_layout(left, top, i);
            let ins = &mut open.instructions[i];
            if rl.op_btn.contains(mouse) {
                cycle_op(ins, forward);
                return Flow::Cancel;
            }
            if ins.op.has_value() {
                if rl.minus_btn.contains(mouse) {
                    adjust_value(ins, false, shift, false);
                    return Flow::Cancel;
                }
                if rl.val_btn.contains(mouse) {
                    step_value_preset(ins, forward);
                    return Flow::Cancel;
                }
                if rl.plus_btn.contains(mouse) {
                    adjust_value(ins, true, shift, false);
                    return Flow::Cancel;
                }
            }
            if ins.op.has_speed() && rl.speed_btn.contains(mouse) {
                cycle_modifier(ins, forward);
                return Flow::Cancel;
            }
        }

        let (reset_btn, cancel_btn, save_btn) = footer_rects(left, top);
        if reset_btn.contains(mouse) {
            open.instructions = rules::padded(&rules::default_program());
        } else if cancel_btn.contains(mouse) {
            open.close = true;
        } else if save_btn.contains(mouse) {
            open.save_and_close();
        }
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
        let forward = event.scroll[1] > 0.0;
        let shift = host.has_shift_down();

        for i in 0..rules::MAX_INSTRUCTIONS {
            let rl = row_layout(left, top, i);
            let ins = &mut open.instructions[i];
            if rl.op_btn.contains(mouse) {
                cycle_op(ins, forward);
                return Flow::Cancel;
            }
            if ins.op.has_value()
                && (rl.val_btn.contains(mouse) || rl.minus_btn.contains(mouse) || rl.plus_btn.contains(mouse))
            {
                adjust_value(ins, forward, shift, true);
                return Flow::Cancel;
            }
            if ins.op.has_speed() && rl.speed_btn.contains(mouse) {
                cycle_modifier(ins, forward);
                return Flow::Cancel;
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
            return Flow::Cancel;
        }
        Flow::Cancel
    }
}

fn draw(host: &dyn Host, event: &mut ScreenRenderPostEvent<'_>, open: &Open) {
    gui::fill(host, event, UiRect::new(0, 0, open.size[0], open.size[1]), gui::DIM);
    let (left, top) = open.origin();
    let panel_rect = UiRect::new(left, top, PANEL_W, PANEL_H);
    gui::panel(host, event, panel_rect, FRAME_BRASS, PANEL_BRASS);

    let badge = if open.current >= 0 && (open.current as usize) < rules::MAX_INSTRUCTIONS {
        format!("Running #{}", open.current + 1)
    } else {
        "Idle".to_owned()
    };
    gui::header(
        host,
        event,
        UiRect::new(left + 2, top + 2, PANEL_W - 4, 22),
        Some("create:sequenced_gearshift"),
        "Sequenced Gearshift",
        Some(&badge),
    );

    let mut tip: Option<Vec<String>> = None;
    let first_end = open.first_end();

    for i in 0..rules::MAX_INSTRUCTIONS {
        let rl = row_layout(left, top, i);
        let ins = open.instructions[i];
        let active = open.current == i as i32;
        let past_end = i > first_end;
        let bg = if active {
            ROW_ACTIVE
        } else if past_end {
            ROW_DIM
        } else if i % 2 == 0 {
            ROW_BG
        } else {
            ROW_ALT
        };
        gui::fill(host, event, rl.row, bg);
        let num_col = if active {
            TEXT_GOLD
        } else if past_end {
            TEXT_MUTED
        } else {
            TEXT_LIGHT
        };
        host.text(event.batch, &format!("#{}", i + 1), [(left + 10) as f32, (rl.row.y + 8) as f32], num_col, false);

        let op_style = if ins.op == Op::End {
            BtnStyle::Normal
        } else {
            BtnStyle::ToggleOn
        };
        if gui::button(host, event, rl.op_btn, op_short_label(ins.op), op_style, true) {
            tip = Some(vec![ins.op.label().to_owned(), "Click or scroll to change".to_owned()]);
        }

        if ins.op.has_value() {
            if gui::button(host, event, rl.minus_btn, "-", BtnStyle::Normal, true) {
                tip = Some(vec!["Decrease (Shift: -1)".to_owned()]);
            }
            if gui::button(host, event, rl.val_btn, &ins.format_value(), BtnStyle::Normal, true) {
                tip = Some(vec![
                    format!("Value: {} (max {})", ins.format_value(), ins.op.max_value()),
                    "Click for preset, scroll to adjust".to_owned(),
                ]);
            }
            if gui::button(host, event, rl.plus_btn, "+", BtnStyle::Normal, true) {
                tip = Some(vec!["Increase (Shift: +1)".to_owned()]);
            }
        }
        if ins.op.has_speed() && gui::button(host, event, rl.speed_btn, modifier_button_label(ins.modifier), BtnStyle::Normal, true) {
            tip = Some(vec![ins.modifier.label().to_owned(), "Click or scroll to change".to_owned()]);
        }
    }

    let (reset_btn, cancel_btn, save_btn) = footer_rects(left, top);
    if gui::button(host, event, reset_btn, "Reset", BtnStyle::Normal, true) {
        tip = Some(vec!["Reset to default 90 deg program".to_owned()]);
    }
    let hint = "Tap or Scroll fields";
    let hx = left + 76 + (124 - host.text_width(hint) as i32) / 2;
    host.text(event.batch, hint, [hx.max(left + 74) as f32, (top + 181) as f32], TEXT_MUTED, false);
    gui::button(host, event, cancel_btn, "Cancel", BtnStyle::Normal, true);
    gui::button(host, event, save_btn, "Save", BtnStyle::Primary, true);

    if let Some(lines) = tip {
        let lines: Vec<Text> = lines.into_iter().map(Text::literal).collect();
        host.gui_tooltip(event.batch, &lines, event.screen.mouse);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_and_save_payloads_round_trip() {
        let open = OpenPayload {
            pos: [12, 64, -8],
            current: 2,
            instructions: vec![
                Instruction { op: Op::TurnAngle, modifier: Modifier::ForwardFast, value: 180 },
                Instruction::with(Op::Delay, 40),
                Instruction::new(Op::Await),
            ],
        };
        let decoded_open = decode_open(&encode_open(&open)).expect("valid open payload");
        assert_eq!(decoded_open.pos, [12, 64, -8]);
        assert_eq!(decoded_open.current, 2);
        assert_eq!(decoded_open.instructions.len(), rules::MAX_INSTRUCTIONS);
        assert_eq!(decoded_open.instructions[0], Instruction { op: Op::TurnAngle, modifier: Modifier::ForwardFast, value: 180 });
        assert_eq!(decoded_open.instructions[1], Instruction::with(Op::Delay, 40));
        assert_eq!(decoded_open.instructions[2].op, Op::Await);
        assert_eq!(decoded_open.instructions[3].op, Op::End);

        let save = SavePayload {
            pos: [-5, 70, 99],
            instructions: decoded_open.instructions,
        };
        let decoded_save = decode_save(&encode_save(&save)).expect("valid save payload");
        assert_eq!(decoded_save.pos, [-5, 70, 99]);
        assert_eq!(decoded_save.instructions[0].modifier, Modifier::ForwardFast);
    }

    #[test]
    fn op_value_and_modifier_stepping_respect_bounds() {
        let mut ins = Instruction::with(Op::TurnAngle, 90);
        step_value_preset(&mut ins, true);
        assert_eq!(ins.value, 135);
        adjust_value(&mut ins, true, true, false);
        assert_eq!(ins.value, 136);
        adjust_value(&mut ins, false, true, true);
        assert_eq!(ins.value, 91);
        cycle_modifier(&mut ins, true);
        assert_eq!(ins.modifier, Modifier::Back);
        cycle_op(&mut ins, true);
        assert_eq!(ins.op, Op::TurnDistance);
        assert_eq!(ins.value, Op::TurnDistance.default_value());
    }
}
