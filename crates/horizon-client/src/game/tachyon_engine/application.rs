//! Create's items used on blocks in the world (MIT, THIRD_PARTY_NOTICES.md):
//! `create:item_application` recipes (ManualApplicationRecipe: andesite
//! alloy on a stripped log makes andesite casing...) and encasing
//! (EncasableBlock: a casing on a shaft or cogwheel makes its encased
//! block). The block keeps the properties both share (its axis).
use horizon_client::items::ItemStack;

/// `EncasingRegistry`: the encased block a casing makes of a bare one.
fn encased(block: &str, held: &str) -> Option<&'static str> {
    Some(match (block, held) {
        ("create:shaft", "create:andesite_casing") => "create:andesite_encased_shaft",
        ("create:shaft", "create:brass_casing") => "create:brass_encased_shaft",
        ("create:cogwheel", "create:andesite_casing") => "create:andesite_encased_cogwheel",
        ("create:cogwheel", "create:brass_casing") => "create:brass_encased_cogwheel",
        ("create:large_cogwheel", "create:andesite_casing") => {
            "create:andesite_encased_large_cogwheel"
        }
        ("create:large_cogwheel", "create:brass_casing") => "create:brass_encased_large_cogwheel",
        _ => return None,
    })
}

impl super::super::Game<'_> {
    pub(in crate::game) fn tachyon_apply(
        &mut self,
        stack: &ItemStack,
        pos: [i32; 3],
        claimed: &mut bool,
    ) {
        if *claimed || !self.may_build() || self.block_action_restricted(pos) {
            return;
        }
        let held = stack.item.to_string();
        let block = self.native_block(pos);
        // A bracket on the bare block comes back with it encased.
        let bracket = self
            .native_block_property(pos, "bracket")
            .filter(|b| b != "none");
        let (result, extra, keep) = match encased(&block, &held) {
            Some(result) => (result.to_owned(), Vec::new(), false),
            None => {
                if !block.starts_with("minecraft:") && !block.starts_with("create:") {
                    return;
                }
                let recipes = self.tachyon_create_recipes();
                let Some(recipe) = recipes.application(&block, &held) else {
                    return;
                };
                let extra: Vec<(String, u32, f32)> = recipe
                    .extra
                    .iter()
                    .map(|o| (o.item.clone(), o.count, o.chance))
                    .collect();
                (recipe.result.clone(), extra, recipe.keep)
            }
        };
        let registry = &self.pack.assets.registry;
        let old = self.world.block(pos[0], pos[1], pos[2]);
        let placed = registry.default_state(&result).and_then(|default| {
            let mut next = registry.state(default)?.clone();
            if let Some(before) = registry.state(old) {
                for (key, value) in before.properties.iter() {
                    if !next.properties.contains_key(key.as_str()) {
                        continue;
                    }
                    let mut tried = next.clone();
                    tried.properties.insert(key.to_string(), value.to_string());
                    if registry.id(&tried).is_some() {
                        next = tried;
                    }
                }
            }
            registry.id(&next)
        });
        let Some(id) = placed else { return };
        let dimension = self.native_dimension();
        if !self.native_set_block(dimension, pos, id, 3) {
            return;
        }
        *claimed = true;
        if self.creative {
            return;
        }
        if !keep {
            self.native_take_items(&held, 1);
        }
        if let Some(item) = bracket.and_then(|b| {
            let material = b.split('_').next()?.to_owned();
            Some(format!("create:{material}_bracket"))
        }) {
            self.native_give_items(&item, 1);
        }
        for (item, count, chance) in extra {
            if chance >= 1.0 || self.tachyon_roll() < chance {
                self.native_give_items(&item, count);
            }
        }
    }

    /// A roll in 0..1 from the world's random.
    fn tachyon_roll(&mut self) -> f32 {
        use horizon_core::random::RandomSource;
        self.world.random.next_float()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn casings_encase_bare_shafts_and_cogwheels() {
        assert_eq!(
            encased("create:shaft", "create:andesite_casing"),
            Some("create:andesite_encased_shaft")
        );
        assert_eq!(
            encased("create:large_cogwheel", "create:brass_casing"),
            Some("create:brass_encased_large_cogwheel")
        );
        assert_eq!(encased("create:shaft", "create:copper_casing"), None);
        assert_eq!(encased("create:gearbox", "create:andesite_casing"), None);
    }
}
