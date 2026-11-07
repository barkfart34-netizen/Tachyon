//! Android's owned touch frames, following horizon-android's `game/pad.rs` and
//! `game/android_input.rs` (MIT, THIRD_PARTY_NOTICES.md). Fixed touch actions do
//! not use controller bindings or keyboard toggles. World gestures come later.
use super::{Game, Screen};
use horizon_client::{Result, gamepad::Button, tachyon_touch::TouchScreen};
#[cfg(any(target_os = "android", test))]
use horizon_client::tachyon_touch::{TouchContext, TouchFrame, TouchRequest};
use winit::{event::MouseButton, keyboard::KeyCode};

#[cfg(any(target_os = "android", test))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Engine {
    Zalith,
    #[default]
    Bedrock,
}
#[cfg(any(target_os = "android", test))]
impl Engine {
    /// Bedrock's, unless the player picked Zalith's (`touchEngine:zalith`).
    pub(super) fn preference(value: Option<&str>) -> Self {
        if value == Some("zalith") {
            Self::Zalith
        } else {
            Self::Bedrock
        }
    }
    /// What `touchEngine` keeps for the engine.
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Zalith => "zalith",
            Self::Bedrock => "bedrock",
        }
    }
}

pub(super) type Mapping = std::result::Result<KeyCode, MouseButton>;

/// The source and its owner, before rebinding. TouchControls already shares
/// built-in buttons between fingers; raw custom keys are a later stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Source {
    Keyboard(KeyCode),
    Mouse(MouseButton),
    Pad(Button),
    #[cfg(any(target_os = "android", test))]
    ZalithKey(KeyCode),
    #[cfg(any(target_os = "android", test))]
    ZalithMouse(MouseButton),
    #[cfg(any(target_os = "android", test))]
    ZalithPointer(MouseButton),
    #[cfg(any(target_os = "android", test))]
    ZalithGesture(u64, MouseButton),
    #[cfg(any(target_os = "android", test))]
    ZalithAction(KeyCode),
    #[cfg(any(target_os = "android", test))]
    Touch(Button),
    #[cfg(any(target_os = "android", test))]
    Request(TouchRequest),
}
impl Source {
    /// The device Horizon's held keys (`HeldKeys`) count it under: touch
    /// as a script's input, neither the keyboard nor a controller.
    fn held(self) -> horizon_client::held::Source {
        use horizon_client::held::Source as Held;
        match self {
            Self::Keyboard(_) | Self::Mouse(_) => Held::Keyboard,
            Self::Pad(_) => Held::Pad,
            #[cfg(any(target_os = "android", test))]
            _ => Held::Agent,
        }
    }

    fn fixed(self) -> bool {
        match self {
            #[cfg(any(target_os = "android", test))]
            Self::Touch(_) | Self::Request(_) => true,
            _ => false,
        }
    }

    fn zalith(self) -> bool {
        match self {
            #[cfg(any(target_os = "android", test))]
            Self::ZalithKey(_) | Self::ZalithMouse(_) | Self::ZalithPointer(_) | Self::ZalithGesture(..)
                | Self::ZalithAction(_) => true,
            _ => false,
        }
    }

    fn family(self, mapping: Mapping) -> Family {
        if self.fixed() {
            Family::Fixed
        } else if matches!(self, Self::Mouse(_)) && mapping.is_ok() {
            Family::Direct
        } else {
            Family::Bound
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Family { Bound, Direct, Fixed }

struct Press {
    source: Source,
    mapping: Mapping,
    screen: TouchScreen,
    tick: u64,
    releasing: bool,
}

#[derive(Default)]
pub(super) struct TouchInput {
    pub(super) movement: Option<[f32; 2]>,
    presses: Vec<Press>,
    #[cfg(target_os = "android")]
    pub(super) keyboard_keys: std::collections::HashSet<KeyCode>,
    #[cfg(target_os = "android")]
    pub(super) zalith_keys: std::collections::HashSet<KeyCode>,
    /// Menu mouse events are virtual, not releases of a hardware button.
    #[cfg(target_os = "android")]
    pub(super) window_mouse: Option<Source>,
    /// Clicks stay owned until a tick, so cancel can discard unconsumed work.
    pending: Vec<(Source, Mapping)>,
    /// Also remember consumed mouse presses: a release can precede suspension
    /// before the next tick has stopped mining or released item use.
    active: Vec<(Mapping, bool)>,
    /// The ordinary source that last changed each mapping's down state. Fixed
    /// touch never enters this array; cancellation preserves physical toggles.
    latched: [Option<Source>; 4],
    #[cfg(any(target_os = "android", test))]
    sneaking: bool,
    #[cfg(any(target_os = "android", test))]
    sneak_toggle: bool,
}
impl TouchInput {
    pub(super) fn held(&self, mapping: Mapping, fixed: bool) -> bool {
        self.presses
            .iter()
            .any(|p| p.mapping == mapping && p.source.fixed() == fixed)
    }

    /// Mouse-to-key rebinding historically bypasses keyboard toggles.
    pub(super) fn direct_held(&self, key: KeyCode) -> bool {
        self.presses.iter().any(|p| matches!(p.source, Source::Mouse(_)) && p.mapping == Ok(key))
    }

    /// Join holds without feeding fixed touch or mouse-to-key input into toggles.
    pub(super) fn joined(&self, mapping: Mapping, ordinary: bool) -> bool {
        ordinary || self.held(mapping, true) || mapping.is_ok_and(|key| self.direct_held(key))
    }

    fn family_held(&self, mapping: Mapping, family: Family) -> bool {
        self.presses.iter().any(|p| p.mapping == mapping && p.source.family(mapping) == family)
    }

    pub(super) fn destination(&self, source: Source) -> Option<TouchScreen> {
        self.presses.iter().find(|p| p.source == source).map(|p| p.screen)
    }

    /// Release the mapping and destination saved at press, even after rebinding.
    /// Only the last owner in each source family emits a release.
    pub(super) fn edge(
        &mut self,
        source: Source,
        mappings: Vec<Mapping>,
        screen: TouchScreen,
        down: bool,
        repeat: bool,
        tick: u64,
    ) -> Vec<(Mapping, TouchScreen)> {
        if self.presses.iter().any(|p| p.source == source) && down {
            for p in self.presses.iter_mut().filter(|p| p.source == source && p.releasing) {
                p.releasing = false;
                p.tick = tick;
            }
            return if repeat {
                self.presses.iter().filter(|p| p.source == source)
                    .map(|p| (p.mapping, p.screen)).collect()
            } else {
                Vec::new()
            };
        }
        let mut edges = Vec::new();
        if down {
            for mapping in mappings {
                if !self.presses.iter().any(|p| p.source == source && p.mapping == mapping) {
                    if !self.family_held(mapping, source.family(mapping)) {
                        edges.push((mapping, screen));
                    }
                    self.presses.push(Press { source, mapping, screen, tick, releasing: false });
                    let active = (mapping, source.fixed());
                    if (source.fixed() || source.zalith()) && !self.active.contains(&active) {
                        self.active.push(active);
                    }
                }
            }
        } else {
            for p in &mut self.presses {
                if p.source == source {
                    p.releasing = true;
                }
            }
            let removed: Vec<_> = self.presses.extract_if(.., |p| p.source == source
                && (!source.fixed() || p.tick != tick || !sampled(p.mapping))).collect();
            for p in removed {
                if !self.family_held(p.mapping, source.family(p.mapping)) {
                    edges.push((p.mapping, p.screen));
                }
            }
            for (slot, owner) in self.latched.iter_mut().enumerate() {
                if *owner == Some(source)
                    && let Some(p) = self.presses.iter().find(|p| p.source != source
                        && toggle_slot(p.mapping) == Some(slot)
                        && p.source.family(p.mapping) == Family::Bound) {
                    *owner = Some(p.source);
                }
            }
        }
        edges
    }

    pub(super) fn expire(&mut self, tick: u64) -> Vec<Mapping> {
        let removed: Vec<_> = self.presses.extract_if(.., |p| p.source.fixed()
            && p.releasing && p.tick != tick).collect();
        let mut released = Vec::new();
        for p in removed {
            if !self.family_held(p.mapping, Family::Fixed) && !released.contains(&p.mapping) {
                released.push(p.mapping);
            }
        }
        released
    }

    pub(super) fn clear(&mut self) -> Vec<Mapping> {
        self.movement = None;
        self.pending.retain(|(source, _)| !source.fixed());
        #[cfg(any(target_os = "android", test))]
        {
            self.sneaking = false;
            self.sneak_toggle = false;
        }
        self.presses.retain(|p| !p.source.fixed());
        self.active.extract_if(.., |(_, fixed)| *fixed).map(|(mapping, _)| mapping).collect()
    }

    pub(super) fn queue(&mut self, source: Source, mapping: Mapping) {
        self.pending.push((source, mapping));
    }

    pub(super) fn take_clicks(&mut self) -> Vec<Mapping> {
        std::mem::take(&mut self.pending).into_iter().map(|(_, mapping)| mapping).collect()
    }

    pub(super) fn toggle_changed(&mut self, source: Source, mapping: Mapping, before: bool, after: bool) {
        if !source.fixed() && before != after && let Some(slot) = toggle_slot(mapping) {
            self.latched[slot] = after.then_some(source);
        }
    }

    /// A virtual toggle is cancelled, unless a bound physical owner remains.
    #[cfg(any(target_os = "android", test))]
    pub(super) fn cancel_zalith_toggle(&mut self, mapping: Mapping) -> bool {
        let Some(slot) = toggle_slot(mapping) else { return false };
        if !self.latched[slot].is_some_and(Source::zalith) { return false; }
        self.latched[slot] = self.presses.iter().find(|p| p.mapping == mapping
            && p.source.family(mapping) == Family::Bound).map(|p| p.source);
        self.latched[slot].is_none()
    }

    #[cfg(any(target_os = "android", test))]
    pub(super) fn clear_zalith(&mut self) -> Vec<Mapping> {
        self.pending.retain(|(source, _)| !source.zalith());
        self.presses.retain(|p| !p.source.zalith());
        self.active.extract_if(.., |(_, fixed)| !*fixed).map(|(mapping, _)| mapping).collect()
    }

    #[cfg(any(target_os = "android", test))]
    pub(super) fn sneak_edge(&mut self, down: bool, toggle: bool) -> Option<bool> {
        if down {
            self.sneak_toggle = toggle;
            self.sneaking = toggle && !self.sneaking;
            Some(!toggle || self.sneaking)
        } else if self.sneak_toggle {
            None
        } else {
            Some(false)
        }
    }

    pub(super) fn sources(&self) -> Vec<Source> {
        let mut sources = Vec::new();
        for p in &self.presses {
            if !sources.contains(&p.source) {
                sources.push(p.source);
            }
        }
        sources
    }
}

fn sampled(mapping: Mapping) -> bool {
    matches!(mapping, Ok(KeyCode::Space | KeyCode::ShiftLeft | KeyCode::ControlLeft))
}

fn toggle_slot(mapping: Mapping) -> Option<usize> {
    match mapping {
        Ok(KeyCode::ShiftLeft) => Some(0),
        Ok(KeyCode::ControlLeft) => Some(1),
        Err(MouseButton::Left) => Some(2),
        Err(MouseButton::Right) => Some(3),
        _ => None,
    }
}

#[cfg(any(target_os = "android", test))]
pub(super) fn button_mapping(button: Button) -> Option<Mapping> {
    Some(match button {
        Button::South => Ok(KeyCode::Space),
        Button::RightStick => Ok(KeyCode::ShiftLeft),
        Button::LeftStick => Ok(KeyCode::ControlLeft),
        Button::RightTrigger => Err(MouseButton::Left),
        Button::LeftTrigger => Err(MouseButton::Right),
        Button::North => Ok(KeyCode::KeyE),
        Button::Start => Ok(KeyCode::Escape),
        _ => return None,
    })
}

#[cfg(any(target_os = "android", test))]
pub(super) fn request_key(request: TouchRequest) -> Option<KeyCode> {
    match request {
        TouchRequest::Chat => Some(KeyCode::KeyT),
        TouchRequest::Perspective => Some(KeyCode::F5),
        TouchRequest::Offhand => Some(KeyCode::KeyF),
        TouchRequest::Settings | TouchRequest::Debug => None,
    }
}

impl Game<'_> {
    pub(super) fn touch_playing(&self) -> bool {
        self.options.execution_role.is_client() && self.focused
            && self.screen == Screen::Playing && self.title.is_none() && self.player.is_some()
            && !self.screen_over_playing() && self.pending_entry.is_none()
            && self.startup_loading.is_none()
             && self.integrated_starting.is_none() && !self.remote_loading()
             && self.remote_touch_ready()
            && self.pending_save.is_none() && self.pack_reload.is_none()
    }

    #[cfg(any(target_os = "android", test))]
    pub(super) fn touch_context(&self) -> TouchContext {
        TouchContext {
            screen: if !self.focused || !self.options.execution_role.is_client() {
                TouchScreen::Inactive
            } else if self.touch_playing() {
                TouchScreen::Playing
            } else if matches!(self.screen, Screen::Inventory | Screen::Creative) {
                TouchScreen::Inventory
            } else {
                TouchScreen::Menu
            },
            vertical_movement: self.player.as_ref().is_some_and(|p| p.flying),
            swimming: self.player.as_ref().is_some_and(|p| p.swimming),
            mounted: self.riding.is_some(),
            sneaking: self.keys.contains(&KeyCode::ShiftLeft),
            shield_available: self.held().into_iter().chain(self.offhand.as_ref())
                .any(|s| self.blocks_attacks(s).is_some()),
            ..TouchContext::default()
        }
    }

    /// Whether a world hold uses the held item rather than mining, as
    /// Bedrock's touch controls do: eating and drinking, drawing a bow or a
    /// crossbow, raising a shield, a trident, a spyglass, a horn, a brush.
    pub(super) fn touch_hold_uses(&self) -> bool {
        let Some(stack) = self.held() else { return false };
        let Some(info) = self.items().get(&stack.item) else { return false };
        let id = info.id.trim_start_matches("minecraft:");
        info.consumable.is_some()
            || info.blocks_attacks.is_some()
            || id.ends_with("_spear")
            || matches!(id, "bow" | "crossbow" | "trident" | "spyglass" | "goat_horn" | "brush")
    }

    pub(super) fn set_touch_aim(&mut self, point: Option<[f32; 2]>) {
        self.touch_aim = point;
    }

    pub(super) fn input_move(&self) -> Option<[f32; 2]> {
        self.touch_input.movement.filter(|_| self.touch_playing()).or_else(|| self.pad_move())
    }

    pub(super) fn input_from(
        &mut self,
        source: Source,
        mappings: Vec<Mapping>,
        down: bool,
        repeat: bool,
    ) -> Result<()> {
        let screen = if self.screen == Screen::Playing && self.title.is_none()
            && !self.screen_over_playing() {
            TouchScreen::Playing
        } else {
            TouchScreen::Menu
        };
        let edges = self.touch_input.edge(source, mappings, screen, down, repeat, self.ticks);
        for (mapping, destination) in edges {
            if source.fixed() {
                self.sync_touch_mapping(mapping);
                if down && destination == TouchScreen::Playing && self.touch_playing() {
                    match mapping {
                        Err(_) | Ok(KeyCode::KeyE | KeyCode::KeyF) => self.touch_input.queue(source, mapping),
                        Ok(KeyCode::Escape) => self.pad_back()?,
                        Ok(key) => self.key_pressed(key, key)?,
                    }
                }
            } else {
                let toggle = toggle_slot(mapping).map(|slot| (slot, self.toggles.is_down(slot)));
                match mapping {
                    Err(button) if destination == TouchScreen::Playing => {
                        // A repeat belongs to the current screen, but release
                        // always cleans up the mapping saved at press.
                        let action = if !down { super::KeyAction::Release }
                            else if repeat { super::KeyAction::Repeat }
                            else { super::KeyAction::Press };
                        let playing = self.screen == Screen::Playing && self.title.is_none()
                            && !self.screen_over_playing();
                        if let Some(pressed) = super::keyboard_button(!down || playing, action) {
                            if source.zalith() {
                                if self.sleeping.is_none() || !pressed {
                                    self.set_mouse_input(button, pressed);
                                    if pressed { self.touch_input.queue(source, mapping); }
                                }
                            } else {
                                self.play_mouse(button, pressed)?;
                            }
                        }
                    }
                    Err(_) => {}
                    Ok(KeyCode::ShiftLeft) if matches!(source, Source::Pad(_))
                        && self.settings.controller.toggle_sneak && !self.settings.toggle_crouch => {
                        if down {
                            self.mapped_key(KeyCode::ShiftLeft, !self.toggles.is_down(0), false, KeyCode::ShiftLeft, source.held())?;
                        } else if !self.touch_input.latched[0].is_some_and(|s| matches!(s, Source::Pad(_))) {
                            self.mapped_key(KeyCode::ShiftLeft, false, false, KeyCode::ShiftLeft, source.held())?;
                        }
                    }
                    Ok(key) if matches!(source, Source::Mouse(_)) => {
                        if down {
                            self.keys.insert(key);
                            self.key_pressed(key, key)?;
                        } else {
                            self.sync_touch_mapping(mapping);
                        }
                    }
                    Ok(key @ (KeyCode::KeyE | KeyCode::KeyF | KeyCode::KeyQ))
                        if source.zalith() && down && self.touch_playing() => {
                        self.keys.insert(key);
                        self.touch_input.queue(source, mapping);
                    }
                    Ok(key) => self.mapped_key(key, down, repeat, key, source.held())?,
                }
                if let Some((slot, was_down)) = toggle {
                    self.touch_input.toggle_changed(source, mapping, was_down, self.toggles.is_down(slot));
                }
            }
        }
        Ok(())
    }

    fn sync_touch_mapping(&mut self, mapping: Mapping) {
        match mapping {
            Ok(KeyCode::ShiftLeft) => self.sync_toggle(0),
            Ok(KeyCode::ControlLeft) => self.sync_toggle(1),
            Ok(key) => {
                if self.touch_input.held(mapping, true) || self.touch_input.held(mapping, false) {
                    self.keys.insert(key);
                } else { self.keys.remove(&key); }
            }
            Err(MouseButton::Left) => self.left = self.touch_input.joined(mapping, self.toggles.is_down(2)),
            Err(MouseButton::Right) => self.right = self.touch_input.joined(mapping, self.toggles.is_down(3)),
            Err(_) => {}
        }
    }

    pub(super) fn cancel_touch_input(&mut self) {
        let released = self.touch_input.clear();
        for mapping in &released { self.sync_touch_mapping(*mapping); }
        self.finish_touch_cancel(&released);
    }

    #[cfg(any(target_os = "android", test))]
    pub(super) fn cancel_zalith_input(&mut self) {
        for source in self.touch_input.sources().into_iter().filter(|s| s.zalith()) {
            if let Err(e) = self.input_from(source, Vec::new(), false, false) {
                log::warn!("Tachyon: touch release: {e}");
            }
        }
        let released = self.touch_input.clear_zalith();
        for mapping in &released {
            if self.touch_input.cancel_zalith_toggle(*mapping)
                && let Some(slot) = toggle_slot(*mapping) {
                self.toggles.0[slot].should_restore_state_on_screen_closed(false);
                self.toggles.0[slot].reset();
            }
            self.sync_touch_mapping(*mapping);
        }
        self.finish_touch_cancel(&released);
    }

    fn finish_touch_cancel(&mut self, released: &[Mapping]) {
        if released.contains(&Err(MouseButton::Left)) && !self.left {
            let clicks = self.attack_clicks;
            self.stop_mining();
            self.attack_clicks = clicks;
            self.stop_remote_mining();
        }
        if released.contains(&Err(MouseButton::Right)) && !self.right && self.using.is_some()
            && self.remote.is_some() { self.remote_release_using(); }
    }

    /// Called outside the overlay borrow, before the game requests its next tick.
    #[cfg(any(target_os = "android", test))]
    pub(super) fn apply_touch_frame(&mut self, frame: TouchFrame) -> Result<bool> {
        if !self.touch_playing() { self.cancel_touch_input(); return Ok(false); }
        self.touch_input.movement = frame.movement;
        if frame.movement.is_some() || !frame.edges.is_empty() || !frame.requests.is_empty() {
            self.last_input = std::time::Instant::now();
        }
        for (button, mut down) in frame.edges {
            if !self.touch_playing() { break; }
            if button == Button::RightStick {
                let Some(held) = self.touch_input.sneak_edge(down, frame.sneak_toggle) else { continue };
                down = held;
            }
            if let Some(mapping) = button_mapping(button) {
                self.input_from(Source::Touch(button), vec![mapping], down, false)?;
            }
        }
        for request in frame.requests {
            if !self.touch_playing() { break; }
            match request {
                TouchRequest::Settings => { self.cancel_touch_input(); return Ok(true); }
                TouchRequest::Debug => {
                    self.debug = !self.debug;
                    continue;
                }
                _ => {}
            }
            let Some(key) = request_key(request) else { continue };
            self.input_from(Source::Request(request), vec![Ok(key)], true, false)?;
            self.input_from(Source::Request(request), Vec::new(), false, false)?;
        }
        if !self.touch_playing() { self.cancel_touch_input(); }
        Ok(false)
    }

    pub(super) fn tick_touch_input(&mut self) -> Result<()> {
        if !self.touch_playing() { self.cancel_touch_input(); return Ok(()); }
        for mapping in self.touch_input.expire(self.ticks) { self.sync_touch_mapping(mapping); }
        for mapping in self.touch_input.take_clicks() {
            if !self.touch_playing() { self.cancel_touch_input(); break; }
            match mapping {
                Err(button) => self.click_mouse(button, true)?,
                Ok(key) => self.key_pressed(key, key)?,
            }
        }
        Ok(())
    }
}
