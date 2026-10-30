// The viewer's edges in the game (not in the Cargo library): the game's
// tables and events converted to the viewer's types, and what the viewer
// asks for done through the native hook points (API 2).
use super::recipes as m;
use crate::game::Game;
use crate::game::native_hooks::{
    NativeItem, NativePanelEvent, NativeRecipe, NativeRecipeKind, NativeRecipeLayout,
    NativeRecipeStack, NativeRecipeView, NativeStack,
};

fn item(i: &NativeItem) -> m::Item {
    m::Item {
        id: i.id.clone(),
        name: i.name.clone(),
        tags: i.tags.clone(),
    }
}

fn ingredient(s: &NativeRecipeStack) -> m::Ingredient {
    m::Ingredient {
        item: s.item,
        count: s.count,
        components: s.components.clone(),
    }
}

fn recipe(r: &NativeRecipe) -> m::Recipe {
    let kind = match r.kind {
        NativeRecipeKind::CraftingShaped => m::Kind::CraftingShaped,
        NativeRecipeKind::CraftingShapeless => m::Kind::CraftingShapeless,
        NativeRecipeKind::Smelting => m::Kind::Smelting,
        NativeRecipeKind::Blasting => m::Kind::Blasting,
        NativeRecipeKind::Smoking => m::Kind::Smoking,
        NativeRecipeKind::Campfire => m::Kind::Campfire,
        NativeRecipeKind::Stonecutting => m::Kind::Stonecutting,
        NativeRecipeKind::Smithing => m::Kind::Smithing,
        NativeRecipeKind::Brewing => m::Kind::Brewing,
    };
    m::Recipe {
        kind,
        width: r.width,
        height: r.height,
        slots: r
            .slots
            .iter()
            .map(|s| s.iter().map(ingredient).collect())
            .collect(),
        results: r.results.iter().map(ingredient).collect(),
        cooking_time: r.cooking_time,
        experience: r.experience,
        book_id: r.book_id,
        chances: Vec::new(),
    }
}

/// Tachyon: the millstone's, the press's and the mixer's recipes (the
/// content engine's) whose items the tables know.
fn processing(items: &[NativeItem]) -> Vec<m::Recipe> {
    let index: std::collections::HashMap<&str, u32> = (items.iter().enumerate())
        .map(|(i, it)| (it.id.as_str(), i as u32))
        .collect();
    let one = |id: &str, count: u32| {
        Some(m::Ingredient {
            item: *index.get(id)?,
            count,
            components: String::new(),
        })
    };
    let mut out = Vec::new();
    let [milling, pressing, mixing] = crate::game::tachyon_engine::processing();
    let kinds = (milling.into_iter().map(|r| (m::Kind::Milling, r)))
        .chain(pressing.into_iter().map(|r| (m::Kind::Pressing, r)))
        .chain(mixing.into_iter().map(|r| (m::Kind::Mixing, r)));
    for (kind, (inputs, time, outputs)) in kinds {
        let slots: Option<Vec<Vec<m::Ingredient>>> = (inputs.iter())
            .map(|(item, count)| one(item, *count).map(|s| vec![s]))
            .collect();
        let Some(slots) = slots.filter(|s| !s.is_empty()) else {
            continue;
        };
        let (mut results, mut chances) = (Vec::new(), Vec::new());
        for (item, count, chance) in outputs {
            if let Some(r) = one(&item, count) {
                results.push(r);
                chances.push(chance);
            }
        }
        if results.is_empty() {
            continue;
        }
        out.push(m::Recipe {
            kind,
            width: 1,
            height: 1,
            slots,
            results,
            cooking_time: time.round() as u32,
            experience: 0.0,
            book_id: -1,
            chances,
        });
    }
    out
}

fn native(s: &m::Stack) -> NativeStack {
    NativeStack {
        item: s.item.clone(),
        count: s.count,
        components: s.components.clone(),
    }
}

fn stack(s: &NativeStack) -> m::Stack {
    m::Stack {
        item: s.item.clone(),
        count: s.count,
        components: s.components.clone(),
    }
}

fn view(v: &m::View) -> NativeRecipeView {
    NativeRecipeView {
        layout: match v.layout {
            m::Layout::Grid => NativeRecipeLayout::Grid,
            m::Layout::Cooking => NativeRecipeLayout::Cooking,
            m::Layout::Single => NativeRecipeLayout::Single,
            m::Layout::Smithing => NativeRecipeLayout::Smithing,
            m::Layout::Brewing => NativeRecipeLayout::Brewing,
        },
        width: v.width,
        height: v.height,
        slots: v
            .slots
            .iter()
            .map(|s| s.iter().map(native).collect())
            .collect(),
        results: v.results.iter().map(native).collect(),
        note: v.note.clone(),
        action: v.action,
    }
}

/// Loads the world's tables once a world is open, and drops them (and the
/// panel) when it closes or the player turns the mod off.
pub(super) fn frame(game: &mut Game<'_>) {
    let tables = if game.native_on(m::ID) {
        game.native_recipes()
    } else {
        None
    };
    match (tables, m::loaded()) {
        (Some(t), false) => {
            let mut recipes: Vec<m::Recipe> = t.recipes.iter().map(recipe).collect();
            recipes.extend(processing(&t.items));
            let (viewer, out) = m::Viewer::new(t.items.iter().map(item).collect(), recipes);
            m::set_viewer(Some(viewer));
            apply(game, out);
        }
        (None, true) => {
            m::set_viewer(None);
            game.native_panel_recipes(m::ID, "", Vec::new(), 0, Vec::new());
            game.native_panel_items(m::ID, Vec::new());
        }
        _ => {}
    }
}

/// The player's action on the panel, as the WebAssembly mod handled it.
pub(super) fn event(game: &mut Game<'_>, event: &NativePanelEvent) {
    let event = match event {
        NativePanelEvent::Search(text) => m::Event::Search(text.clone()),
        NativePanelEvent::Clicked { index, right } => m::Event::Clicked {
            index: *index as usize,
            right: *right,
        },
        NativePanelEvent::Key { key, stack: s } => {
            let [[recipe, _, recipe_default], [uses, _, uses_default]] = m::KEYS;
            let uses = if *key == game.native_mod_key(m::ID, recipe, recipe_default) {
                false
            } else if *key == game.native_mod_key(m::ID, uses, uses_default) {
                true
            } else {
                return;
            };
            m::Event::Key {
                uses,
                stack: stack(s),
            }
        }
        NativePanelEvent::SlotClicked {
            stack: s, right, ..
        } => m::Event::SlotClicked {
            stack: stack(s),
            right: *right,
        },
        NativePanelEvent::Tab(i) => m::Event::Tab(*i as usize),
        NativePanelEvent::Action(i) => m::Event::Action(*i as usize),
        NativePanelEvent::Back => m::Event::Back,
        _ => return,
    };
    let give = m::gives(game.native_setting(m::ID, "give"));
    let known = game.native_known_recipes();
    let outs = m::with_viewer(|v| v.handle(event.clone(), give, true, &known)).unwrap_or_default();
    for out in outs {
        if let m::Out::Give { item, count } = &out {
            // Not in creative mode: the click shows the item's page, as with the setting off.
            if game.native_give(item, *count).is_err() {
                let again = m::with_viewer(|v| v.handle(event.clone(), false, false, &known));
                for out in again.unwrap_or_default() {
                    apply(game, out);
                }
            }
            continue;
        }
        apply(game, out);
    }
}

fn apply(game: &mut Game<'_>, out: m::Out) {
    match out {
        m::Out::Items(items) => game.native_panel_items(m::ID, items.iter().map(native).collect()),
        m::Out::Recipes {
            title,
            tabs,
            tab,
            views,
        } => game.native_panel_recipes(
            m::ID,
            &title,
            tabs.iter().map(native).collect(),
            tab as u32,
            views.iter().map(view).collect(),
        ),
        m::Out::Give { item, count } => {
            let _ = game.native_give(&item, count);
        }
        m::Out::Place(book) => {
            if let Err(e) = game.native_place_recipe(book, false) {
                game.native_chat(&e);
            }
        }
    }
}
