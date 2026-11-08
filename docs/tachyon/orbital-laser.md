# Orbital Laser in Tachyon

Orbital Laser (github.com/Stink-O/orbital-laser, MIT, Copyright (c) 2026 Stink-O) is built into Tachyon from its
native bundle, version 0.5.10 (fifteen lasers; the Fold of 0.5.11 needs the fork's renderer and is not in it). The
bundle came from the release v0.5.11's `orbital.hnmod` (sha256 f860b61f2c54cc4efd6e6373054858768b0fd829afc0b59c60893a556ef4da63),
pinned to upstream Horizon e57b1d34, an ancestor of Tachyon.

Where its parts are, as horizon-mixins would have woven them:

| Bundle | Tachyon |
|---|---|
| `engine/` (the mod's code on its native SDK) | `crates/orbital-laser` (unchanged; a dependency of horizon-client) |
| `glue.rs` (module `orbital_glue` of game.rs) | `crates/horizon-client/src/game/orbital_glue.rs` |
| `post.rs` (module `orbital_post` of the render crate) | `crates/horizon-client-render/src/orbital_post.rs` |
| `carry.rs` (module `orbital_carry` of the client library) | `crates/horizon-client/src/orbital_carry.rs` |
| the injections on named hooks | one `super::orbital_glue::…` line at the top of each `native_*` hook point in `native_hooks.rs` |
| `world_drawn`, `render_needs` | `native_render.rs` hook points call `crate::orbital_post::{draw, needs}` |
| `render_frame` (HEAD of `Renderer::render`) | first line of `Renderer::render` in `gfx.rs` |
| `carry_tick` (cancellable HEAD) | top of `MotionWorld::controlled_simulation_tick_with_sprint_window` in `motion.rs` |
| `content_find` | not needed: the content (`horizon-mod.toml`, `assets/`, `data/`) is built-in content, `tachyon/content/orbital` |

To update: unpack the new `orbital.hnmod`, replace the files above, and diff its `horizon-mixin.toml` against this
table for new or changed injections.

Playing: `/orbital give` (or the Designator in the creative tab "Tachyon"), R with the Designator in hand for the
laser wheel (`/orbital wheel` on a phone), hold use on a target to fire. `/orbital terrain off` makes strikes light and
sound only. Strikes with terrain on wreck the land for good.
