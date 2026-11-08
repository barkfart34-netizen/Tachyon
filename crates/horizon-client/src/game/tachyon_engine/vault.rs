//! Create's item vault (ItemVaultBlock, ItemVaultBlockEntity; MIT, THIRD_PARTY_NOTICES.md) on
//! create:item_vault: 20 slots a block (`vaultCapacity`), no window of its own; hoppers, funnels and
//! chutes put into and take from it, and vault blocks lying the same way side by side are one vault
//! (up to 3 wide and 3 times that long, `getMaxWidth`, `getMaxLength`): what goes in fills the
//! first of its blocks with room, what comes out comes from the first that has any. A broken block
//! drops what it held.
//!
//! Not as Create: a vault's blocks keep their own slots rather than one controller's, so a vault
//! that loses a block loses those slots; no connected textures, stacks of 64 whatever the item.
use super::kinetic::{Axis, Dir, Part};
use super::millstone::Held;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

/// `vaultCapacity`: slots a block.
pub(super) const SLOTS: usize = 20;
pub(super) const STACK: u32 = 64;
/// `getMaxWidth` 3 by `getMaxLength` 9.
pub(super) const MAX_BLOCKS: usize = 81;

/// One vault block's slots.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Vault {
    pub slots: Vec<Held>,
}

impl Vault {
    /// How many of `count` `item` it would take.
    pub fn room(&self, item: &str, count: u32) -> u32 {
        let on_top: u32 = (self.slots.iter())
            .filter(|h| h.item == item)
            .map(|h| STACK.saturating_sub(h.count))
            .sum();
        let empty = (SLOTS.saturating_sub(self.slots.len())) as u32 * STACK;
        count.min(on_top + empty)
    }

    /// Puts up to `count` `item` in, onto its stacks first: how many went in.
    pub fn put(&mut self, item: &str, count: u32) -> u32 {
        let mut left = count;
        for h in self.slots.iter_mut().filter(|h| h.item == item) {
            let n = left.min(STACK.saturating_sub(h.count));
            h.count += n;
            left -= n;
        }
        while left > 0 && self.slots.len() < SLOTS {
            let n = left.min(STACK);
            self.slots.push(Held { item: item.to_owned(), count: n });
            left -= n;
        }
        count - left
    }

    /// One item out of its first stack.
    pub fn take_one(&mut self) -> Option<Held> {
        let first = self.slots.first_mut()?;
        first.count -= 1;
        let one = Held { item: first.item.clone(), count: 1 };
        if first.count == 0 {
            self.slots.remove(0);
        }
        Some(one)
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

/// The vault blocks one vault with the one at `pos`: lying the same way, side by side; in order.
pub(super) fn joined(parts: &HashMap<[i32; 3], Part>, pos: [i32; 3]) -> Vec<[i32; 3]> {
    let Some(Part::Vault(axis)) = parts.get(&pos).copied() else {
        return Vec::new();
    };
    let mut seen = HashSet::from([pos]);
    let mut queue = VecDeque::from([pos]);
    while let Some(p) = queue.pop_front() {
        for d in Dir::ALL {
            let o = d.offset();
            let q = [p[0] + o[0], p[1] + o[1], p[2] + o[2]];
            if seen.len() < MAX_BLOCKS && parts.get(&q) == Some(&Part::Vault(axis)) && seen.insert(q) {
                queue.push_back(q);
            }
        }
    }
    let mut out: Vec<[i32; 3]> = seen.into_iter().collect();
    // Along its length first, as ItemVaultBlockEntity's inventory runs.
    let along = |p: &[i32; 3]| match axis {
        Axis::X => (p[0], p[1], p[2]),
        _ => (p[2], p[1], p[0]),
    };
    out.sort_by_key(along);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vault_stacks_in_twenty_slots_and_joins_its_neighbours() {
        let mut v = Vault::default();
        assert_eq!(v.put("minecraft:stone", 100), 100);
        assert_eq!(v.slots.len(), 2);
        assert_eq!(v.room("minecraft:stone", 10_000), 64 * 20 - 100);
        assert_eq!(v.put("minecraft:dirt", 64 * 30), 64 * 18);
        assert_eq!(v.room("minecraft:dirt", 5), 0);
        assert_eq!(v.take_one().map(|h| h.item), Some("minecraft:stone".to_owned()));
        assert_eq!(v.slots[0].count, 63);
        let parts = HashMap::from([
            ([0, 0, 0], Part::Vault(Axis::X)),
            ([1, 0, 0], Part::Vault(Axis::X)),
            ([0, 1, 0], Part::Vault(Axis::X)),
            // Lying the other way: a vault of its own.
            ([2, 0, 0], Part::Vault(Axis::Z)),
        ]);
        assert_eq!(joined(&parts, [1, 0, 0]), vec![[0, 0, 0], [0, 1, 0], [1, 0, 0]]);
        assert_eq!(joined(&parts, [2, 0, 0]), vec![[2, 0, 0]]);
        assert!(joined(&parts, [9, 9, 9]).is_empty());
    }
}
