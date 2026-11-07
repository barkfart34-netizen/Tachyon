//! Backend-free checks for the Android adapter, selected by CI's `tachyon` filter.

#[cfg(test)]
mod tests {
    use super::super::super::touch_input::{Engine, Mapping, Source, TouchInput, button_mapping, request_key};
    use horizon_client::{
        gamepad::Button,
        keymapping::{KeyAction, ToggleKeys},
        tachyon_touch::{Control, Mode, TouchContext, TouchControls, TouchRequest, TouchScreen},
    };
    use std::time::Duration;
    use winit::{event::MouseButton, keyboard::KeyCode};

    const PLAYING: TouchScreen = TouchScreen::Playing;
    const SHIFT: Mapping = Ok(KeyCode::ShiftLeft);

    fn edge(input: &mut TouchInput, source: Source, mapping: Mapping, down: bool, tick: u64)
        -> Vec<(Mapping, TouchScreen)> {
        input.edge(source, vec![mapping], PLAYING, down, false, tick)
    }

    #[test]
    fn starling_is_the_default_engine_and_actions_are_canonical() {
        assert_eq!(Engine::default(), Engine::Starling);
        for value in [None, Some(""), Some("unknown"), Some("Bedrock")] {
            assert_eq!(Engine::preference(value), Engine::Starling);
        }
        assert_eq!(Engine::preference(Some("bedrock")), Engine::Bedrock);
        assert_eq!(Engine::preference(Some("zalith")), Engine::Zalith);
        for engine in [Engine::Zalith, Engine::Bedrock, Engine::Starling] {
            assert_eq!(Engine::preference(Some(engine.name())), engine);
            assert_ne!(engine.next(), engine);
        }
        assert_eq!(Engine::Starling.next().next().next(), Engine::Starling);
        // No controller bindings, deadzones, enabled flag or keyboard rebinding enter this table.
        let mappings = [
            (Button::South, Ok(KeyCode::Space)),
            (Button::RightStick, SHIFT),
            (Button::LeftStick, Ok(KeyCode::ControlLeft)),
            (Button::RightTrigger, Err(MouseButton::Left)),
            (Button::LeftTrigger, Err(MouseButton::Right)),
            (Button::North, Ok(KeyCode::KeyE)),
            (Button::Start, Ok(KeyCode::Escape)),
        ];
        for button in Button::ALL {
            assert_eq!(button_mapping(button), mappings.iter().find(|(b, _)| *b == button).map(|(_, m)| *m));
        }
        assert_eq!(request_key(TouchRequest::Chat), Some(KeyCode::KeyT));
        assert_eq!(request_key(TouchRequest::Perspective), Some(KeyCode::F5));
        assert_eq!(request_key(TouchRequest::Offhand), Some(KeyCode::KeyF));
        assert_eq!(request_key(TouchRequest::Settings), None);
        assert_eq!(request_key(TouchRequest::Debug), None);
    }

    #[test]
    fn owners_share_saved_bindings_and_release_to_the_original_destination() {
        let mut input = TouchInput::default();
        let space = Ok(KeyCode::Space);
        let keyboard = Source::Keyboard(KeyCode::KeyJ);
        let pad = Source::Pad(Button::South);
        let touch = Source::Touch(Button::South);
        assert_eq!(edge(&mut input, keyboard, space, true, 1), [(space, PLAYING)]);
        assert!(edge(&mut input, pad, space, true, 1).is_empty());
        assert_eq!(edge(&mut input, touch, space, true, 1), [(space, PLAYING)]);
        assert_eq!(input.destination(keyboard), Some(PLAYING));
        assert!(input.edge(keyboard, vec![Ok(KeyCode::KeyG)], TouchScreen::Menu, false, false, 2).is_empty());
        assert_eq!(edge(&mut input, touch, space, false, 2), [(space, PLAYING)]);
        assert!(input.held(space, false) && !input.held(space, true));
        assert_eq!(input.edge(pad, Vec::new(), TouchScreen::Inventory, false, false, 2), [(space, PLAYING)]);
        assert!(!input.held(space, false));
        assert_eq!(input.destination(pad), None);
        assert!(edge(&mut input, touch, space, false, 2).is_empty());
    }

    #[test]
    fn fixed_and_direct_holds_do_not_flip_ordinary_toggles() {
        let mut input = TouchInput::default();
        let mut toggles = ToggleKeys::default();
        let keyboard = Source::Keyboard(KeyCode::ShiftLeft);
        let direct = Source::Mouse(MouseButton::Back);
        let touch = Source::Touch(Button::RightStick);
        edge(&mut input, keyboard, SHIFT, true, 1);
        toggles.key(0, KeyAction::Press, false, true);
        input.toggle_changed(keyboard, SHIFT, false, true);
        assert_eq!(edge(&mut input, direct, SHIFT, true, 1), [(SHIFT, PLAYING)]);
        assert_eq!(edge(&mut input, touch, SHIFT, true, 1), [(SHIFT, PLAYING)]);
        edge(&mut input, keyboard, SHIFT, false, 2);
        toggles.key(0, KeyAction::Release, false, true);
        assert_eq!(input.clear(), [SHIFT]);
        assert!(input.joined(SHIFT, toggles.is_down(0)));
        toggles.0[0].reset();
        assert!(input.joined(SHIFT, false), "direct mouse binding still holds crouch");
        edge(&mut input, direct, SHIFT, false, 2);
        assert!(!input.joined(SHIFT, false));
    }

    #[test]
    fn ordinary_owners_inherit_virtual_latches_but_direct_bindings_do_not() {
        let virtual_key = Source::ZalithKey(KeyCode::ShiftLeft);
        let keyboard = Source::Keyboard(KeyCode::ShiftLeft);
        let mut input = TouchInput::default();
        edge(&mut input, virtual_key, SHIFT, true, 1);
        input.toggle_changed(virtual_key, SHIFT, false, true);
        edge(&mut input, keyboard, SHIFT, true, 1);
        assert!(edge(&mut input, virtual_key, SHIFT, false, 2).is_empty());
        // The physical toggle remains its own even if its key is released before cancel.
        edge(&mut input, keyboard, SHIFT, false, 2);
        assert_eq!(input.clear_zalith(), [SHIFT]);
        assert!(!input.cancel_zalith_toggle(SHIFT));
        for owner in [keyboard, Source::Mouse(MouseButton::Back)] {
            let mut input = TouchInput::default();
            edge(&mut input, virtual_key, SHIFT, true, 1);
            input.toggle_changed(virtual_key, SHIFT, false, true);
            edge(&mut input, owner, SHIFT, true, 1);
            input.clear_zalith();
            assert_eq!(input.cancel_zalith_toggle(SHIFT), owner != keyboard);
            assert!(input.held(SHIFT, false));
        }
    }

    #[test]
    fn short_sampled_presses_survive_one_tick_and_a_repress_cancels_release() {
        for (button, mapping) in [
            (Button::South, Ok(KeyCode::Space)),
            (Button::RightStick, SHIFT),
            (Button::LeftStick, Ok(KeyCode::ControlLeft)),
        ] {
            let source = Source::Touch(button);
            let mut input = TouchInput::default();
            edge(&mut input, source, mapping, true, 7);
            assert!(edge(&mut input, source, mapping, false, 7).is_empty());
            assert!(input.expire(7).is_empty() && input.held(mapping, true));
            assert!(edge(&mut input, source, mapping, true, 7).is_empty());
            assert!(input.expire(8).is_empty() && input.held(mapping, true));
            assert_eq!(edge(&mut input, source, mapping, false, 8), [(mapping, PLAYING)]);
            edge(&mut input, source, mapping, true, 8);
            edge(&mut input, source, mapping, false, 8);
            assert_eq!(input.expire(9), [mapping]);
            assert!(!input.held(mapping, true));
        }
    }

    #[test]
    fn deferred_release_waits_for_the_last_fixed_owner() {
        let mut input = TouchInput::default();
        let space = Ok(KeyCode::Space);
        let first = Source::Touch(Button::South);
        let other = Source::Request(TouchRequest::Chat);
        edge(&mut input, first, space, true, 7);
        edge(&mut input, other, space, true, 7);
        edge(&mut input, first, space, false, 7);
        assert!(input.expire(8).is_empty());
        assert!(input.held(space, true));
        assert_eq!(edge(&mut input, other, space, false, 8), [(space, PLAYING)]);
    }

    #[test]
    fn cancel_discards_owned_clicks_even_after_release_or_consumption() {
        let mut input = TouchInput::default();
        let attack = Err(MouseButton::Left);
        let touch = Source::Touch(Button::RightTrigger);
        let physical = Source::Mouse(MouseButton::Left);
        let swap = Source::Request(TouchRequest::Offhand);
        edge(&mut input, physical, attack, true, 1);
        edge(&mut input, touch, attack, true, 1);
        input.queue(touch, attack);
        edge(&mut input, swap, Ok(KeyCode::KeyF), true, 1);
        input.queue(swap, Ok(KeyCode::KeyF));
        edge(&mut input, touch, attack, false, 1);
        input.movement = Some([0.0, 1.0]);
        assert_eq!(input.clear(), [attack, Ok(KeyCode::KeyF)]);
        assert!(input.take_clicks().is_empty() && input.movement.is_none());
        assert!(input.held(attack, false));
        edge(&mut input, touch, attack, true, 2);
        input.queue(touch, attack);
        assert_eq!(input.take_clicks(), [attack]);
        edge(&mut input, touch, attack, false, 3);
        assert_eq!(input.clear(), [attack], "consumed use/mining still needs cleanup after up");
        assert!(input.clear().is_empty() && input.held(attack, false));
        input.queue(Source::ZalithAction(KeyCode::KeyQ), Ok(KeyCode::KeyQ));
        input.queue(touch, attack);
        input.clear();
        assert_eq!(input.take_clicks(), [Ok(KeyCode::KeyQ)]);
        for source in [Source::ZalithMouse(MouseButton::Right), Source::ZalithGesture(3, MouseButton::Right),
            Source::ZalithPointer(MouseButton::Left)] {
            input.queue(source, attack);
        }
        input.clear_zalith();
        assert!(input.take_clicks().is_empty());
    }

    #[test]
    fn sneak_policy_is_snapshotted_and_contextual_hold_replaces_the_touch_latch() {
        let mut touch = TouchControls::default();
        touch.settings.mode = Mode::Dpad;
        touch.viewport(1600, 900);
        let point = touch.placement(Control::Sneak).center;
        assert!(touch.down(1, point, Duration::ZERO));
        touch.up(1);
        assert!(touch.down(2, point, Duration::from_millis(100)));
        touch.up(2);
        touch.context.vertical_movement = true;
        let frame = touch.poll_frame();
        assert!(frame.sneak_toggle);
        assert_eq!(frame.edges, [(Button::RightStick, true)]);
        let mut input = TouchInput::default();
        assert_eq!(input.sneak_edge(true, frame.sneak_toggle), Some(true));
        assert_eq!(input.sneak_edge(false, false), None);
        assert_eq!(input.sneak_edge(true, true), Some(false));
        assert_eq!(input.sneak_edge(false, false), None);
        assert_eq!(input.sneak_edge(true, true), Some(true));
        assert_eq!(input.sneak_edge(true, false), Some(true));
        assert_eq!(input.sneak_edge(false, true), Some(false));
        input.clear();
        assert_eq!(input.sneak_edge(true, true), Some(true));
        for context in [
            TouchContext { vertical_movement: true, ..TouchContext::default() },
            TouchContext { swimming: true, ..TouchContext::default() },
            TouchContext { mounted: true, ..TouchContext::default() },
        ] {
            touch.clear();
            touch.context = context;
            assert!(touch.down(3, point, Duration::ZERO));
            touch.context = TouchContext::default();
            assert!(!touch.poll_frame().sneak_toggle);
            touch.up(3);
            assert!(!touch.poll_frame().sneak_toggle);
        }
    }
}
