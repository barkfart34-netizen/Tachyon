//! Create's metal girder and the girder with a shaft through it
//! (GirderBlock, GirderEncasedShaftBlock; MIT, THIRD_PARTY_NOTICES.md) on
//! the ported create:* blocks, after Kinetics for Horizon's girder
//! (sakurartro, MIT). The girder keeps Create's own states (`x`, `z`,
//! `top`, `bottom` beside its template's `axis`), so Create's multipart
//! model draws it; they are settled after each level tick around a block
//! that changed (`updateShape`). A shaft used on a girder goes through it.
use super::water_wheel::replaceable;
use horizon_client::items::ItemStack;

pub(super) const GIRDER: &str = "create:metal_girder";
pub(super) const ENCASED: &str = "create:metal_girder_encased_shaft";
const SHAFT_ITEM: &str = "create:shaft";

fn is_girder(block: &str) -> bool {
    block == GIRDER || block == ENCASED
}

/// -x, +x, -y, +y, -z, +z: index / 2 is the axis.
const DIRS: [[i32; 3]; 6] = [
    [-1, 0, 0],
    [1, 0, 0],
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
];

/// What stands beside a girder, as far as its connections care.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Neighbour {
    Other,
    /// Something to hold on to (Create: a full solid face).
    Solid,
    Girder {
        x: bool,
        z: bool,
    },
    /// A girder with a shaft through it along x (0) or z (2).
    Shafted {
        axis: usize,
    },
    WallUp,
    ChainY,
    Lantern {
        hanging: bool,
    },
}

/// A block and its properties as a neighbour. Anything that isn't air, a
/// fluid or something built over counts as solid (no face test here).
fn classify(name: &str, prop: impl Fn(&str) -> Option<String>) -> Neighbour {
    let on = |k: &str| prop(k).as_deref() == Some("true");
    match name {
        GIRDER => Neighbour::Girder {
            x: on("x"),
            z: on("z"),
        },
        ENCASED => Neighbour::Shafted {
            axis: if prop("axis").as_deref() == Some("x") {
                0
            } else {
                2
            },
        },
        "minecraft:iron_chain" | "minecraft:copper_chain"
            if prop("axis").as_deref() == Some("y") =>
        {
            Neighbour::ChainY
        }
        n if n.ends_with("_wall") && on("up") => Neighbour::WallUp,
        n if n.ends_with("lantern") => Neighbour::Lantern {
            hanging: on("hanging"),
        },
        n if replaceable(n, prop("layers").as_deref())
            || n.ends_with("air")
            || n.ends_with("lava") =>
        {
            Neighbour::Other
        }
        _ => Neighbour::Solid,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct GirderState {
    axis: usize,
    x: bool,
    z: bool,
    top: bool,
    bottom: bool,
}

/// `updateVerticalProperty`: whether the end toward `d` (2 down, 3 up) holds.
fn attaches(own_post: bool, side: Neighbour, d: usize) -> bool {
    own_post
        || match side {
            Neighbour::Girder { .. }
            | Neighbour::Shafted { .. }
            | Neighbour::WallUp
            | Neighbour::ChainY => true,
            Neighbour::Lantern { hanging } => (d == 2) == hanging,
            _ => false,
        }
}

/// `GirderBlock.updateShape` for every side.
fn update(mut g: GirderState, side: impl Fn(usize) -> Neighbour) -> GirderState {
    for a in [0usize, 2] {
        if g.axis != a && side(2 * a) != Neighbour::Solid && side(2 * a + 1) != Neighbour::Solid {
            if a == 0 {
                g.x = false;
            } else {
                g.z = false;
            }
        }
    }
    if g.axis != 1 {
        g.bottom &= side(2) == Neighbour::Solid;
        g.top &= side(3) == Neighbour::Solid;
    }
    for d in 0..6 {
        let a = d / 2;
        if a == 1 {
            if attaches(g.axis == 1, side(d), d) {
                if d == 2 {
                    g.bottom = true;
                } else {
                    g.top = true;
                }
            }
            continue;
        }
        let join = g.axis == a
            || match side(d) {
                Neighbour::Shafted { axis } => axis != a,
                Neighbour::Girder { x, z } => {
                    if a == 0 {
                        x
                    } else {
                        z
                    }
                }
                _ => false,
            };
        if join {
            if a == 0 {
                g.x = true;
            } else {
                g.z = true;
            }
        }
    }
    g
}

/// `GirderEncasedShaftBlock.updateShape`: only its vertical ends.
fn update_shafted(top: bool, bottom: bool, side: impl Fn(usize) -> Neighbour) -> (bool, bool) {
    let end =
        |on: bool, d: usize| (on && side(d) == Neighbour::Solid) || attaches(false, side(d), d);
    (end(top, 3), end(bottom, 2))
}

/// `GirderBlock.useItemOn`: the shaft runs along z through a girder with an
/// x beam or clicked on a z face, else along x.
fn shaft_axis(x_beam: bool, clicked_axis: usize) -> usize {
    if x_beam || clicked_axis == 2 { 2 } else { 0 }
}

fn flag(on: bool) -> &'static str {
    if on { "true" } else { "false" }
}

/// Girders beside blocks that changed, settled after their level's tick.
static DIRTY: std::sync::Mutex<Vec<(&'static str, [i32; 3])>> = std::sync::Mutex::new(Vec::new());

fn take(dimension: &str) -> Vec<[i32; 3]> {
    let mut dirty = DIRTY.lock().unwrap_or_else(|e| e.into_inner());
    let mut out: Vec<[i32; 3]> = dirty
        .iter()
        .filter(|d| d.0 == dimension)
        .map(|d| d.1)
        .collect();
    dirty.retain(|d| d.0 != dimension);
    out.sort();
    out.dedup();
    out
}

fn add(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

impl super::super::GameLevel<'_> {
    /// A block came or went: it and the girders around it settle later.
    pub(in crate::game) fn tachyon_girder_changed(
        &self,
        dimension: &'static str,
        pos: [i32; 3],
        new: horizon_world::registry::BlockStateId,
    ) {
        let girder = |state| self.native_state_block(state).is_some_and(is_girder);
        let mut dirty = Vec::new();
        if girder(new) {
            dirty.push((dimension, pos));
        }
        for d in DIRS {
            let at = add(pos, d);
            if girder(self.world.block(at[0], at[1], at[2])) {
                dirty.push((dimension, at));
            }
        }
        if !dirty.is_empty() {
            DIRTY
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .extend(dirty);
        }
    }
}

impl super::super::Game<'_> {
    /// The block at `pos` with `properties` set, if its block has them.
    fn tachyon_with_properties(
        &self,
        block: &str,
        pos: Option<[i32; 3]>,
        properties: &[(&str, &str)],
    ) -> Option<horizon_world::registry::BlockStateId> {
        let registry = &self.pack.assets.registry;
        let base = match pos {
            Some(p) => self.world.block(p[0], p[1], p[2]),
            None => registry.default_state(block)?,
        };
        let mut state = registry.state(base)?.clone();
        for (key, value) in properties {
            state.properties.insert((*key).into(), (*value).into());
        }
        registry.id(&state)
    }

    /// The girders marked this tick take the connections around them.
    pub(in crate::game) fn tachyon_girder_settle(&mut self, dimension: &str) {
        for pos in take(dimension) {
            let name = self.native_block(pos);
            if !is_girder(&name) {
                continue;
            }
            let side = |d: usize| {
                let p = add(pos, DIRS[d]);
                classify(&self.native_block(p), |k| self.native_block_property(p, k))
            };
            let on = |key: &str| self.native_block_property(pos, key).as_deref() == Some("true");
            let next = if name == GIRDER {
                let axis = self.native_block_property(pos, "axis");
                let before = GirderState {
                    axis: ["x", "y", "z"]
                        .iter()
                        .position(|a| Some(*a) == axis.as_deref())
                        .unwrap_or(1),
                    x: on("x"),
                    z: on("z"),
                    top: on("top"),
                    bottom: on("bottom"),
                };
                let after = update(before, side);
                if after == before {
                    continue;
                }
                let props = [
                    ("x", flag(after.x)),
                    ("z", flag(after.z)),
                    ("top", flag(after.top)),
                    ("bottom", flag(after.bottom)),
                ];
                self.tachyon_with_properties(&name, Some(pos), &props)
            } else {
                let (top, bottom) = update_shafted(on("top"), on("bottom"), side);
                if (top, bottom) == (on("top"), on("bottom")) {
                    continue;
                }
                self.tachyon_with_properties(
                    &name,
                    Some(pos),
                    &[("top", flag(top)), ("bottom", flag(bottom))],
                )
            };
            if let Some(id) = next {
                self.native_set_block(dimension, pos, id, 3);
            }
        }
    }

    /// A shaft used on a metal girder goes through it.
    pub(in crate::game) fn tachyon_girder_use(
        &mut self,
        stack: &ItemStack,
        pos: [i32; 3],
        face: [i32; 3],
        claimed: &mut bool,
    ) {
        if *claimed
            || &*stack.item != SHAFT_ITEM
            || !self.may_build()
            || self.block_action_restricted(pos)
            || self.native_block(pos) != GIRDER
        {
            return;
        }
        let on = |key: &str| self.native_block_property(pos, key).as_deref() == Some("true");
        let clicked = face.iter().position(|c| *c != 0).unwrap_or(1);
        let axis = ["x", "y", "z"][shaft_axis(on("x"), clicked)];
        let props = [
            ("axis", axis),
            ("top", flag(on("top"))),
            ("bottom", flag(on("bottom"))),
        ];
        let Some(id) = self.tachyon_with_properties(ENCASED, None, &props) else {
            return;
        };
        let dimension = self.native_dimension();
        if self.native_set_block(dimension, pos, id, 3) {
            if !self.creative {
                self.native_take_items(SHAFT_ITEM, 1);
            }
            *claimed = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn around(sides: [Neighbour; 6]) -> impl Fn(usize) -> Neighbour {
        move |d| sides[d]
    }

    #[test]
    fn a_post_takes_beams_from_girders_and_holds_at_both_ends() {
        use Neighbour::*;
        let post = GirderState {
            axis: 1,
            x: false,
            z: false,
            top: false,
            bottom: false,
        };
        let lone = update(post, around([Other; 6]));
        assert!(lone.top && lone.bottom && !lone.x && !lone.z);
        let beside = [
            Girder { x: true, z: false },
            Other,
            Other,
            Other,
            Other,
            Other,
        ];
        let joined = update(post, around(beside));
        assert!(joined.x && !joined.z);
    }

    #[test]
    fn a_beam_runs_its_axis_and_drops_a_cross_beam_with_nothing_to_hold() {
        use Neighbour::*;
        let beam = GirderState {
            axis: 0,
            x: false,
            z: true,
            top: true,
            bottom: false,
        };
        let g = update(beam, around([Other; 6]));
        assert!(g.x && !g.z && !g.top && !g.bottom);
        let held = update(beam, around([Other, Other, Other, Solid, Solid, Other]));
        assert!(held.z && held.top);
        let shafted = update(
            beam,
            around([Other, Other, Other, Other, Shafted { axis: 0 }, Other]),
        );
        assert!(shafted.z);
        let lantern = update(
            beam,
            around([Other, Other, Lantern { hanging: true }, Other, Other, Other]),
        );
        assert!(lantern.bottom);
    }

    #[test]
    fn a_shafted_girder_keeps_ends_only_where_held() {
        use Neighbour::*;
        assert_eq!(
            update_shafted(true, true, around([Other; 6])),
            (false, false)
        );
        assert_eq!(
            update_shafted(
                false,
                false,
                around([Other, Other, ChainY, WallUp, Other, Other])
            ),
            (true, true)
        );
        assert_eq!(shaft_axis(true, 0), 2);
        assert_eq!(shaft_axis(false, 2), 2);
        assert_eq!(shaft_axis(false, 1), 0);
    }

    #[test]
    fn neighbours_are_read_from_their_names() {
        let none = |_: &str| None;
        assert_eq!(classify("minecraft:air", none), Neighbour::Other);
        assert_eq!(classify("minecraft:short_grass", none), Neighbour::Other);
        assert_eq!(classify("minecraft:stone", none), Neighbour::Solid);
        let x = |k: &str| (k == "x").then(|| "true".to_owned());
        assert_eq!(classify(GIRDER, x), Neighbour::Girder { x: true, z: false });
        let y = |k: &str| (k == "axis").then(|| "y".to_owned());
        assert_eq!(classify("minecraft:iron_chain", y), Neighbour::ChainY);
        let hanging = |k: &str| (k == "hanging").then(|| "true".to_owned());
        assert_eq!(
            classify("minecraft:lantern", hanging),
            Neighbour::Lantern { hanging: true }
        );
    }
}
