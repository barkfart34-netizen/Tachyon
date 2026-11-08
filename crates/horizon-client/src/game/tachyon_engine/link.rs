//! Create's redstone link (RedstoneLinkBlock, RedstoneLinkBlockEntity, RedstoneLinkNetworkHandler,
//! LinkBehaviour; MIT, THIRD_PARTY_NOTICES.md) on create:redstone_link. A link is a transmitter or,
//! after a sneaking use with an empty hand, a receiver; its frequency is two items (empty slots
//! count, so new links pair at once). A transmitter reads the redstone around it
//! (`RedstoneLinkBlock.getPower`); each receiver gives the strongest transmitter's of its frequency
//! within 256 blocks (`linkRange`), to every side and strongly into the block it is on.
//!
//! Not as Create: a use with an item sets the first empty frequency slot (a third item starts the
//! pair again) instead of the slot clicked; the frequency's items aren't drawn on the link, the
//! goggles show them.
use super::kinetic::Dir;
use horizon_core::position::BlockPos;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub(super) const LINK: &str = "create:redstone_link";
/// `linkRange`.
pub(super) const RANGE: i64 = 256;

/// A link's frequency: its two items, empty when unset.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(super) struct Frequency(pub String, pub String);

impl Frequency {
    /// The frequency after an item is set on it.
    pub fn with(&self, item: &str) -> Frequency {
        match (self.0.is_empty(), self.1.is_empty()) {
            (true, _) => Frequency(item.to_owned(), self.1.clone()),
            (false, true) => Frequency(self.0.clone(), item.to_owned()),
            (false, false) => Frequency(item.to_owned(), String::new()),
        }
    }
}

/// What each receiver gets: the strongest transmitter of its frequency in range.
pub(super) fn received(
    transmitters: &[([i32; 3], Frequency, i32)],
    receivers: &[([i32; 3], Frequency)],
) -> HashMap<[i32; 3], i32> {
    receivers
        .iter()
        .map(|(p, f)| {
            let best = (transmitters.iter())
                .filter(|(q, g, _)| g == f && (0..3).map(|i| i64::from(p[i] - q[i]).pow(2)).sum::<i64>() < RANGE * RANGE)
                .map(|(_, _, n)| *n)
                .max()
                .unwrap_or(0);
            (*p, best)
        })
        .collect()
}

/// The signal a link gives toward `direction` (vanilla's), `direct` through the block it is on.
pub(super) fn signal(property: impl Fn(&str) -> Option<String>, direction: Dir, direct: bool) -> i32 {
    if property("receiver").as_deref() != Some("true") {
        return 0;
    }
    if direct && property("facing").and_then(|f| Dir::parse(&f)) != Some(direction) {
        return 0;
    }
    property("power").and_then(|p| p.parse().ok()).unwrap_or(0)
}

fn core(d: Dir) -> horizon_core::geometry::Direction {
    use horizon_core::geometry::Direction as D;
    match d {
        Dir::Down => D::Down,
        Dir::Up => D::Up,
        Dir::North => D::North,
        Dir::South => D::South,
        Dir::West => D::West,
        Dir::East => D::East,
    }
}

impl super::super::Game<'_> {
    /// The owner's links a tick: the transmitters read, the receivers given.
    pub(super) fn tachyon_links(&mut self) {
        use super::kinetic::Part;
        if self.remote.is_some() {
            return;
        }
        let dimension = self.native_dimension();
        let links: Vec<([i32; 3], Dir, Frequency)> = super::OWNERS.with_borrow_mut(|owners| {
            let Some(o) = owners.get_mut(dimension) else { return Vec::new() };
            let parts = &o.index.parts;
            o.links.retain(|p, _| matches!(parts.get(p), Some(Part::Link(_))));
            (parts.iter())
                .filter_map(|(p, part)| match *part {
                    Part::Link(facing) => Some((*p, facing, o.links.get(p).cloned().unwrap_or_default())),
                    _ => None,
                })
                .collect()
        });
        if links.is_empty() {
            return;
        }
        let mut transmitters = Vec::new();
        let mut receivers = Vec::new();
        for (p, facing, frequency) in links {
            if self.native_block_property(p, "receiver").as_deref() == Some("true") {
                receivers.push((p, frequency));
                continue;
            }
            // RedstoneLinkBlock.getPower: around it, and up out of each side but its back.
            let at = BlockPos::new(p[0], p[1], p[2]);
            let power = self.with_level(|b, level, ticks| {
                let mut power = 0;
                for d in Dir::ALL {
                    let o = d.offset();
                    let q = at.offset(o[0], o[1], o[2]);
                    power = power.max(b.signal(level, ticks, q, core(d)));
                    if d != facing.opposite() {
                        power = power.max(b.signal(level, ticks, q, core(Dir::Up)));
                    }
                }
                power.clamp(0, 15)
            });
            let powered = (power > 0).to_string();
            if self.native_block_property(p, "powered").as_deref() != Some(powered.as_str()) {
                self.tachyon_redstone_set(p, &[("powered", powered)]);
            }
            transmitters.push((p, frequency, power));
        }
        for (p, power) in received(&transmitters, &receivers) {
            let value = power.to_string();
            if self.native_block_property(p, "power").as_deref() != Some(value.as_str()) {
                self.tachyon_redstone_set(p, &[("power", value), ("powered", (power > 0).to_string())]);
            }
        }
    }

    /// A use of a link: a sneaking one with an empty hand swaps its mode, one with an item sets
    /// its frequency.
    pub(super) fn tachyon_use_link(&mut self, pos: [i32; 3], sneaking: bool) -> bool {
        let held = self.held().map(|s| s.item.to_string()).filter(|i| i != "minecraft:air");
        let dimension = self.native_dimension();
        match held {
            None if sneaking => {
                let receiver = self.native_block_property(pos, "receiver").as_deref() == Some("true");
                self.tachyon_redstone_set(
                    pos,
                    &[("receiver", (!receiver).to_string()), ("power", "0".into()), ("powered", "false".into())],
                );
                self.native_chat(if receiver { "Redstone Link: transmitter" } else { "Redstone Link: receiver" });
                true
            }
            Some(item) => {
                let frequency = super::OWNERS.with_borrow_mut(|owners| {
                    let o = owners.entry(dimension).or_default();
                    let f = o.links.get(&pos).cloned().unwrap_or_default().with(&item);
                    o.links.insert(pos, f.clone());
                    o.mills_changed = true;
                    f
                });
                let name = |i: &str| if i.is_empty() { "(empty)".to_owned() } else { self.tachyon_item_name(i) };
                self.native_chat(&format!("Redstone Link: {} + {}", name(&frequency.0), name(&frequency.1)));
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_of_a_frequency_carry_the_strongest_in_range() {
        let f = Frequency::default().with("minecraft:stone").with("minecraft:dirt");
        assert_eq!(f, Frequency("minecraft:stone".into(), "minecraft:dirt".into()));
        assert_eq!(f.with("minecraft:sand"), Frequency("minecraft:sand".into(), String::new()));
        let empty = Frequency::default();
        let t = [([0, 0, 0], f.clone(), 7), ([10, 0, 0], f.clone(), 12), ([0, 0, 0], empty.clone(), 3)];
        let r = received(&t, &[([5, 0, 0], f.clone()), ([300, 0, 0], f.clone()), ([1, 1, 1], empty.clone())]);
        assert_eq!(r[&[5, 0, 0]], 12);
        assert_eq!(r[&[300, 0, 0]], 0);
        assert_eq!(r[&[1, 1, 1]], 3);
        let receiver = |k: &str| match k {
            "receiver" => Some("true".to_owned()),
            "facing" => Some("up".to_owned()),
            "power" => Some("12".to_owned()),
            _ => None,
        };
        assert_eq!(signal(receiver, Dir::North, false), 12);
        assert_eq!(signal(receiver, Dir::Up, true), 12);
        assert_eq!(signal(receiver, Dir::North, true), 0);
        assert_eq!(signal(|_| None, Dir::Up, false), 0);
    }
}
