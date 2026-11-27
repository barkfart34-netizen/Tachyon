//! What a player sees of value boxes (`ValueBox`, `ValueBoxRenderer`, `ScrollValueRenderer`, `FilteringRenderer`,
//! `ValueSettingsClient.render`): the filter item on its slot (always), and, while a block with boxes is looked at,
//! each box's content (its number, its option's name, its filter's amount), an outline on the box the crosshair is
//! on, and the label and hint at the bottom of the screen. The positions and what a box holds are the rules in
//! `create_kinetics::rules::value_box*`; this only reads the client's world and draws.
//!
//! Known differences from Create, for the README:
//! - The outline is four lines round the box and its text is name-tag text that turns to face the camera: Horizon
//!   draws neither Create's outline textures nor text lying on the box. An option is shown by its name, not its icon
//!   (those are in Create's own art sheet, which this port doesn't import).
//! - The filter item is drawn with the box's own pose as Create's renderer does (`ValueBoxRenderer`); the glint of an
//!   enchanted item is frozen and the hover fade of the hint is a plain on and off.
//! - The checkered face highlight of Create's filter box (`AllSpecialTextures.THIN_CHECKERED`) isn't drawn.

use create_kinetics::rules::value_box::{self as geo, Pose, Props, Vec3};
use create_kinetics::rules::value_box_blocks::{self as table, BoxDef, Kind};
use create_kinetics::rules::value_box_board::{self as board, Behaviour, Values};
use create_kinetics::{Axis, Dir, Pos};
use crate::compat::{
    BlockEntitySubmitEvent, DisplayContext, Hooks, Host, HudEvent, ItemStack, NativeScreenKind, WorldPrimitivesEvent,
};

use super::value_box::{self as vb, Hit};

struct Draw;

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Draw)
}

/// The block the crosshair is on, with where.
#[derive(Clone, Debug)]
pub struct Looked {
    pub pos: Pos,
    pub block: String,
    pub hit: Hit,
}

/// `mc.hitResult` of a block with value boxes, with the face and point worked out as the server does.
pub fn looked_at(host: &dyn Host) -> Option<Looked> {
    let client = host.client_state()?;
    if client.screen != NativeScreenKind::Playing {
        return None;
    }
    let hit = host.raycast(vb::REACH as f32)?;
    if !table::has_boxes(&hit.block) {
        return None;
    }
    let pos = [hit.block_x, hit.block_y, hit.block_z];
    let eye = client.eye_position?;
    let look = geo::view_vector(f64::from(client.yaw), f64::from(client.pitch));
    let at = [hit.x - f64::from(pos[0]), hit.y - f64::from(pos[1]), hit.z - f64::from(pos[2])];
    let guess = geo::nearest_face(at).normal();
    let hit_side = vb::hit_of(host, eye, look, pos, guess)?;
    Some(Looked { pos, block: hit.block, hit: hit_side })
}

/// One box of the looked-at block as it is drawn.
pub struct Shown {
    pub def: &'static BoxDef,
    pub pose: Pose,
    /// The crosshair is on it.
    pub hovered: bool,
    pub values: Values,
    pub filter: Option<ItemStack>,
    /// `FilteringBehaviour.isCountVisible`.
    pub counts: bool,
}

/// The boxes of the looked-at block that are there on the face looked at.
pub fn shown(host: &dyn Host, looked: &Looked) -> Vec<Shown> {
    let side = looked.hit.side;
    vb::with_props(host, looked.pos, &looked.block, |p, nbt| {
        let Some(nbt) = nbt else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for def in table::boxes(&looked.block) {
            if !(def.active)(p) || !def.slot.exists(p, side) {
                continue;
            }
            let Some(pose) = def.slot.pose(p, side) else {
                continue;
            };
            let values = vb::values_of(host, nbt, def, side);
            let hovered = def.slot.test_hit(p, side, looked.hit.local)
                || matches!(
                    looked.block.as_str(),
                    "create:creative_motor" | "create:rotation_speed_controller"
                );
            out.push(Shown {
                def,
                pose,
                hovered,
                counts: vb::takes_board(def, p, &values),
                filter: def.is_filter().then(|| vb::filter_item(nbt, def, side)).flatten(),
                values,
            });
        }
        out
    })
}

/// A language key in the player's language (the key itself where the package lacks it, as Minecraft shows it).
fn say(host: &dyn Host, key: &str) -> String {
    super::goggles::lang(host).translate(key, &[])
}

// --- the world --------------------------------------------------------------------------------------------

const WHITE: u32 = 0xFF_FFFFFF;
const TEXT: u32 = 0xFF_EDEDED;
const PLATE_BG: u32 = 0xEB_26211C;
const PLATE_BORDER: u32 = 0xFF_C49A45;
const PLATE_HOVER: u32 = 0xFF_F5D27A;
/// How far in front of the box (in its own units) the outline and the text float.
const LIFT: f64 = 0.06;

fn corner_lines(primitives: &mut crate::compat::WorldPrimitives, origin: Vec3, corners: [Vec3; 4], width: f32) {
    let at = |p: Vec3| [origin[0] + p[0], origin[1] + p[1], origin[2] + p[2]];
    for i in 0..4 {
        primitives.line(at(corners[i]), at(corners[(i + 1) % 4]), WHITE, width);
    }
}

/// Emits a quad in `pose`'s local frame (`x0 > x1` spans left->right, `y1 > y0` spans bottom->top).
fn pose_quad(
    primitives: &mut crate::compat::WorldPrimitives,
    origin: Vec3,
    pose: &Pose,
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
    lift: f64,
    argb: u32,
) {
    let at = |lx: f64, ly: f64| {
        let p = pose.apply([lx, ly, -lift]);
        [origin[0] + p[0], origin[1] + p[1], origin[2] + p[2]]
    };
    primitives.quad([at(x0, y1), at(x0, y0), at(x1, y0), at(x1, y1)], argb);
}

/// Compact 3x5 bitmap font rows (3 bits per row, MSB left) for on-block value display.
fn glyph_bits(ch: char) -> [u8; 5] {
    match ch {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        '-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        '+' => [0b000, 0b010, 0b111, 0b010, 0b000],
        '*' => [0b101, 0b010, 0b111, 0b010, 0b101],
        ':' => [0b000, 0b010, 0b000, 0b010, 0b000],
        '.' => [0b000, 0b000, 0b000, 0b000, 0b010],
        '<' | '\u{2264}' => [0b001, 0b010, 0b100, 0b010, 0b001],
        '=' => [0b000, 0b111, 0b000, 0b111, 0b000],
        '\u{27f3}' => [0b111, 0b001, 0b101, 0b100, 0b111],
        '\u{27f2}' => [0b111, 0b100, 0b101, 0b001, 0b111],
        'A' | 'a' => [0b010, 0b101, 0b111, 0b101, 0b101],
        'B' | 'b' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'C' | 'c' => [0b011, 0b100, 0b100, 0b100, 0b011],
        'D' | 'd' => [0b110, 0b101, 0b101, 0b101, 0b110],
        'E' | 'e' => [0b111, 0b100, 0b110, 0b100, 0b111],
        'F' | 'f' => [0b111, 0b100, 0b110, 0b100, 0b100],
        'G' | 'g' => [0b111, 0b100, 0b101, 0b101, 0b111],
        'H' | 'h' => [0b101, 0b101, 0b111, 0b101, 0b101],
        'I' | 'i' => [0b111, 0b010, 0b010, 0b010, 0b111],
        'K' | 'k' => [0b101, 0b101, 0b110, 0b101, 0b101],
        'L' | 'l' => [0b100, 0b100, 0b100, 0b100, 0b111],
        'M' | 'm' => [0b101, 0b111, 0b101, 0b101, 0b101],
        'N' | 'n' => [0b110, 0b101, 0b101, 0b101, 0b101],
        'O' | 'o' => [0b111, 0b101, 0b101, 0b101, 0b111],
        'P' | 'p' => [0b111, 0b101, 0b111, 0b100, 0b100],
        'R' | 'r' => [0b110, 0b101, 0b110, 0b101, 0b101],
        'S' | 's' => [0b011, 0b100, 0b010, 0b001, 0b110],
        'T' | 't' => [0b111, 0b010, 0b010, 0b010, 0b010],
        'U' | 'u' => [0b101, 0b101, 0b101, 0b101, 0b111],
        'V' | 'v' => [0b101, 0b101, 0b101, 0b101, 0b010],
        'W' | 'w' => [0b101, 0b101, 0b101, 0b111, 0b101],
        'X' | 'x' => [0b101, 0b101, 0b010, 0b101, 0b101],
        'Y' | 'y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        _ => [0b000, 0b000, 0b000, 0b000, 0b000],
    }
}

/// Draws a 3D backing plate + pixel-font readout right on a value box face (`WorldPrimitives::quad`).
fn draw_value_face(
    primitives: &mut crate::compat::WorldPrimitives,
    origin: Vec3,
    pose: &Pose,
    text: &str,
    tint: u32,
    hovered: bool,
) {
    let border = if hovered { PLATE_HOVER } else { PLATE_BORDER };
    pose_quad(primitives, origin, pose, 0.44, -0.44, -0.34, 0.34, LIFT * 0.65, border);
    pose_quad(primitives, origin, pose, 0.39, -0.39, -0.29, 0.29, LIFT * 0.85, PLATE_BG);

    let shown: String = text.chars().take(6).collect();
    let count = shown.chars().count();
    if count == 0 {
        return;
    }
    let total_cols = count * 4 - 1;
    let avail_w = 0.68_f64;
    let cell = (avail_w / total_cols as f64).min(0.085);
    let text_w = total_cols as f64 * cell;
    let text_h = 5.0 * cell;
    let start_x = text_w * 0.5;
    let top_y = text_h * 0.5;

    for (ci, ch) in shown.chars().enumerate() {
        let rows = glyph_bits(ch);
        let gx = start_x - (ci * 4) as f64 * cell;
        for (ry, bits) in rows.iter().enumerate() {
            let y1 = top_y - ry as f64 * cell;
            let y0 = y1 - cell * 0.92;
            for col in 0..3 {
                if (bits >> (2 - col)) & 1 != 0 {
                    let x0 = gx - col as f64 * cell;
                    let x1 = x0 - cell * 0.92;
                    pose_quad(primitives, origin, pose, x0, x1, y0, y1, LIFT * 1.15, tint);
                }
            }
        }
    }
}

impl Hooks for Draw {
    /// `ScrollValueRenderer.tick` and `FilteringRenderer.tick`: the boxes of the block looked at.
    fn world_primitives(&self, host: &dyn Host, event: &mut WorldPrimitivesEvent<'_>) {
        if !host.window() || host.sneaking() {
            return;
        }
        let Some(looked) = looked_at(host) else {
            return;
        };
        let origin = [f64::from(looked.pos[0]), f64::from(looked.pos[1]), f64::from(looked.pos[2])];
        for item in shown(host, &looked) {
            let tint = item.def.slot.override_color().map_or(TEXT, |rgb| 0xFF00_0000 | rgb);
            match item.def.kind {
                Kind::Value { behaviour, .. } => {
                    let text = match behaviour {
                        Behaviour::Options { options } => {
                            let key = options.get(item.values.scroll.max(0) as usize).copied().unwrap_or("");
                            say(host, key)
                        }
                        Behaviour::Kinetic { .. } => board::box_text(behaviour, &item.values),
                        _ => board::box_text(behaviour, &item.values),
                    };
                    draw_value_face(event.primitives, origin, &item.pose, &text, tint, item.hovered);
                }
                Kind::Filter { .. } => {
                    // The count's label beside the filter item (`ItemValueBox.renderContents`).
                    let label = board::count_label(item.counts, &item.values);
                    if !label.is_empty() {
                        draw_value_face(event.primitives, origin, &item.pose, &label, TEXT, item.hovered);
                    }
                }
            }
            if item.hovered {
                let wide = item.filter.is_some() || matches!(item.def.kind, Kind::Value { behaviour: Behaviour::Options { .. }, .. });
                corner_lines(event.primitives, origin, item.pose.corners(0.5, LIFT), if wide { 3.0 } else { 2.0 });
            }
        }
    }

    /// `ValueSettingsClient.render`: the label of the box looked at and what to do with it.
    fn hud(&self, host: &mut dyn Host, event: &mut HudEvent<'_>) {
        if !host.window() || host.sneaking() {
            return;
        }
        let Some(client) = host.client_state() else {
            return;
        };
        if client.screen != NativeScreenKind::Playing || client.gui_hidden {
            return;
        }
        let Some(looked) = looked_at(&*host) else {
            return;
        };
        let Some(item) = shown(&*host, &looked).into_iter().find(|item| item.hovered) else {
            return;
        };
        let words = |key: &str| say(&*host, key);
        let mut lines = match item.def.kind {
            Kind::Value { behaviour, .. } => {
                let current = board::format(behaviour, item.values.max_stack, board::settings_of(behaviour, &item.values), &words);
                let title = if matches!(behaviour, Behaviour::Kinetic { .. }) {
                    format!("{}: {current} RPM", say(&*host, item.def.label()))
                } else {
                    format!("{}: {current}", say(&*host, item.def.label()))
                };
                vec![title, say(&*host, "create.gui.value_settings.hold_to_edit")]
            }
            Kind::Filter { .. } => {
                let tip = if item.filter.is_none() {
                    "create.logistics.filter.click_to_set"
                } else {
                    "create.logistics.filter.click_to_replace"
                };
                let mut l = vec![say(&*host, item.def.label()), say(&*host, tip)];
                if item.counts {
                    l.push(say(&*host, "create.logistics.filter.hold_to_set_amount"));
                }
                l
            }
        };
        let (w, h) = (client.gui_size[0], client.gui_size[1]);
        let mut y = h - 75.0 - lines.len() as f32 * 12.0;
        for (i, line) in lines.iter_mut().enumerate() {
            let width = host.text_width(line);
            let rgb = if i == 0 { 0xFB_DC7D } else { 0xFF_FFFF };
            host.text(event.batch, line, [(w - width) / 2.0, y], rgb, true);
            y += 12.0;
        }
    }

    /// `FilteringRenderer.renderOnBlockEntity`: the filter item on its slot, drawn with the slot's own pose.
    fn block_entity_submit(&self, host: &dyn Host, event: &mut BlockEntitySubmitEvent<'_>) {
        let defs = table::boxes(event.block);
        if !defs.iter().any(BoxDef::is_filter) {
            return;
        }
        let nbt = &event.entity.nbt;
        let mut drawn = false;
        for def in defs.iter().filter(|def| def.is_filter()) {
            let sides: &[Dir] = if def.slot.sided() { &Dir::ALL } else { &[Dir::Up] };
            for &side in sides {
                let Some(stack) = vb::filter_item(nbt, def, side) else {
                    continue;
                };
                let pose = vb::with_state_nbt(host, event.state, event.pos, event.block, Some(nbt), |p: &Props| {
                    if (def.active)(p) && def.slot.exists(p, side) { def.slot.pose(p, side) } else { None }
                });
                let Some(pose) = pose else {
                    continue;
                };
                if !drawn {
                    // Over the block's own drawing, which stays as it was.
                    event.draw.still();
                    drawn = true;
                }
                draw_filter(host, event, &pose, &stack);
            }
        }
    }
}

/// `ValueBoxRenderer.renderItemIntoValueBox`: the box's pose, then the item at a flat item's half size (a block item
/// at the box's size) nudged to the front for the few thin blocks Create names.
fn draw_filter(host: &dyn Host, event: &mut BlockEntitySubmitEvent<'_>, pose: &Pose, stack: &ItemStack) {
    let block_item = host.default_state(&stack.item).is_some();
    let scale = (if block_item { 1.0 } else { 0.5 }) + 1.0 / 64.0;
    let nudge = if nudged(&stack.item) { -0.1 } else { 0.0 };
    let z = (if block_item { 0.0 } else { -0.15 }) + nudge;
    let draw = &mut *event.draw;
    draw.push();
    draw.translate([pose.offset[0] as f32, pose.offset[1] as f32, pose.offset[2] as f32]);
    for (axis, degrees) in &pose.rotations {
        let vector = match axis {
            Axis::X => [1.0, 0.0, 0.0],
            Axis::Y => [0.0, 1.0, 0.0],
            Axis::Z => [0.0, 0.0, 1.0],
        };
        draw.rotate_about(vector, *degrees as f32, [0.0; 3]);
    }
    draw.scale([pose.scale as f32; 3]);
    draw.scale([scale as f32; 3]);
    draw.translate([0.0, 0.0, z as f32]);
    host.draw_item(draw, stack, DisplayContext::Fixed, None, 0);
    draw.pop();
}

/// `ValueBoxRenderer.customZOffset`: the block items Create pulls a tenth forward (shafts, fences, buttons, end rods).
fn nudged(item: &str) -> bool {
    let name = item.strip_prefix("minecraft:").unwrap_or(item);
    item == "create:shaft"
        || item == "create:powered_shaft"
        || name.ends_with("_fence")
        || name.ends_with("_button")
        || name == "end_rod"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_thin_blocks_are_nudged() {
        assert!(nudged("create:shaft"));
        assert!(nudged("minecraft:oak_fence"));
        assert!(nudged("minecraft:stone_button"));
        assert!(nudged("minecraft:end_rod"));
        assert!(!nudged("minecraft:stone"));
    }

    #[test]
    fn draws_3d_value_box_plate_and_digits() {
        let pose = Pose {
            offset: [0.5, 0.5, 15.5 / 16.0],
            rotations: vec![(Axis::Y, 180.0)],
            scale: 0.5,
        };
        let mut prims = crate::compat::WorldPrimitives::default();
        draw_value_face(&mut prims, [10.0, 64.0, 20.0], &pose, "256", TEXT, true);
        assert!(prims.quads.len() > 20, "expected backing plate + glyph quads, got {}", prims.quads.len());
    }
}
