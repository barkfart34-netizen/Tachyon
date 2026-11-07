//! Backend-free touch tests in the desktop target, selected by CI's `tachyon` filter.

#[cfg(test)]
mod tests {
    use horizon_client::{
        gamepad::{self, Button},
        input,
        tachyon_touch::{Control, Mode, Settings, TouchContext, TouchControls, TouchFrame, TouchRequest},
    };
    use std::time::Duration;

    const ASPECT: f32 = 16.0 / 9.0;
    const DIAGONAL: f32 = std::f32::consts::FRAC_1_SQRT_2;

    fn touch(mode: Mode) -> TouchControls {
        let mut t = TouchControls::default();
        t.settings.mode = mode;
        t.settings = Settings::parse(&t.settings.to_json().unwrap()).unwrap();
        t.viewport(1600, 900);
        t
    }

    fn point(t: &TouchControls, stick: [f32; 2]) -> [f32; 2] {
        let p = t.placement(Control::Move);
        [
            p.center[0] + stick[0] * p.size / 2.0 / ASPECT,
            p.center[1] - stick[1] * p.size / 2.0,
        ]
    }

    fn down(t: &mut TouchControls, id: u64, control: Control, millis: u64) {
        let p = t.placement(control).center;
        assert!(t.down(id, p, Duration::from_millis(millis)));
    }

    fn close(actual: [f32; 2], expected: [f32; 2]) {
        for (a, b) in actual.into_iter().zip(expected) {
            assert!((a - b).abs() < 1e-5, "{actual:?} != {expected:?}");
        }
    }

    #[test]
    fn one_movement_owner_and_button_roles_survive_dragging() {
        let mut t = touch(Mode::Crosshair);
        down(&mut t, 1, Control::Move, 0);
        down(&mut t, 2, Control::Move, 0);
        down(&mut t, 3, Control::Jump, 0);
        assert!(!t.down(4, [0.55, 0.4], Duration::ZERO));
        let forward = point(&t, [0.0, 0.8]);
        t.moved(1, forward);
        t.moved(2, point(&t, [1.0, 0.0]));
        t.moved(3, [0.55, 0.4]);
        t.moved(4, t.placement(Control::Attack).center);
        let frame = t.poll_frame();
        assert_eq!(frame.movement, Some([0.0, 1.0]));
        assert_eq!(frame.edges, [(Button::South, true)]);
        assert_eq!(frame.pad.triggers, [0.0; 2]);
        assert_eq!(frame.look_degrees, [0.0; 2]);
        assert!(frame.aim.is_none() && frame.taps.is_empty() && frame.key_events.is_empty());
        assert!(!frame.holding && !frame.holding_use);
        // Repeated down cannot turn the movement owner into a jump finger.
        down(&mut t, 1, Control::Jump, 0);
        t.up(1);
        assert_eq!(t.poll_frame().movement, None);
        assert!(t.owns_pointer(2));
        t.moved(2, forward);
        assert_eq!(t.poll_frame().movement, None);
        t.up(2);
        t.up(3);
        assert_eq!(t.poll_frame().edges, [(Button::South, false)]);
    }

    #[test]
    fn built_in_edges_share_owners_and_keep_short_presses_for_one_poll() {
        for (control, button) in [
            (Control::Jump, Button::South),
            (Control::Sneak, Button::RightStick),
            (Control::Sprint, Button::LeftStick),
            (Control::Attack, Button::RightTrigger),
            (Control::Use, Button::LeftTrigger),
            (Control::Shield, Button::LeftTrigger),
            (Control::Inventory, Button::North),
            (Control::Pause, Button::Start),
        ] {
            let mut t = touch(Mode::Crosshair);
            t.settings.shield_button = true;
            t.context.shield_available = true;
            down(&mut t, 1, control, 0);
            down(&mut t, 2, control, 0);
            assert_eq!(t.poll_frame().edges, [(button, true)], "{control:?}");
            t.up(1);
            assert!(t.poll_frame().edges.is_empty());
            t.up(2);
            assert_eq!(t.poll_frame().edges, [(button, false)]);
            down(&mut t, 3, control, 0);
            t.up(3);
            assert_eq!(t.poll_frame().edges, [(button, true)]);
            assert_eq!(t.poll_frame().edges, [(button, false)]);
            assert_eq!(t.poll_frame(), TouchFrame::default());
        }
        let mut t = touch(Mode::Crosshair);
        t.settings.shield_button = true;
        t.context.shield_available = true;
        down(&mut t, 1, Control::Use, 0);
        down(&mut t, 2, Control::Shield, 0);
        assert!(t.shield_held());
        let frame = t.poll_frame();
        assert!(frame.shield_held);
        assert_eq!(frame.edges, [(Button::LeftTrigger, true)]);
        t.up(2);
        assert!(!t.shield_held());
        let frame = t.poll_frame();
        assert!(!frame.shield_held && frame.edges.is_empty());
        t.up(1);
        assert_eq!(t.poll_frame().edges, [(Button::LeftTrigger, false)]);
    }

    #[test]
    fn joystick_is_capped_and_analog_is_opt_in() {
        let mut t = touch(Mode::Tap);
        down(&mut t, 1, Control::Move, 0);
        t.moved(1, point(&t, [0.2, 0.8]));
        close(t.state().left, [0.2, 0.8]);
        assert_eq!(t.poll_frame().movement, Some([0.0, 1.0]));
        t.moved(1, point(&t, [2.0, 2.0]));
        let stick = t.state().left;
        assert!((stick[0].hypot(stick[1]) - 1.0).abs() < 1e-5);
        close(stick, [DIAGONAL; 2]);
        t.clear();
        t.settings.analog_movement = true;
        down(&mut t, 1, Control::Move, 0);
        t.moved(1, point(&t, [0.2, 0.8]));
        close(t.poll_frame().movement.unwrap(), [-0.2, 0.8]);
        t.moved(1, [f32::NAN, f32::INFINITY]);
        close(t.state().left, [0.2, 0.8]);
    }

    #[test]
    fn dpad_uses_a_third_radius_dead_area_and_normalized_diagonals() {
        for split in [false, true] {
            let mut t = touch(Mode::Dpad);
            t.settings.split_controls = split;
            assert!(t.down(1, point(&t, [0.0, 0.8]), Duration::ZERO));
            for (stick, expected) in [
                ([0.0, 0.0], [0.0, 0.0]),
                ([-0.332, 0.332], [0.0, 0.0]),
                ([0.8, 0.0], [1.0, 0.0]),
                ([0.0, -0.8], [0.0, -1.0]),
                ([-0.334, 0.334], [-DIAGONAL, DIAGONAL]),
            ] {
                t.moved(1, point(&t, stick));
                close(t.state().left, expected);
            }
            t.clear();
            t.settings.swap_jump_sneak = true;
            down(&mut t, 2, Control::Jump, 0);
            assert_eq!(t.poll_frame().edges, [(Button::South, true)]);
        }
    }

    #[test]
    fn java_directions_match_keyboard_at_every_magnitude_and_sector() {
        for scale in [0.005, 0.5, 1.0] {
            for x in [-1.0f32, -0.45, 0.0, 0.45, 1.0] {
                for y in [-1.0f32, -0.45, 0.0, 0.45, 1.0] {
                    let expected = input::keyboard_direction(y > 0.0, y < 0.0, x < 0.0, x > 0.0);
                    assert_eq!(
                        gamepad::java_move_vector([x * scale, y * scale], 0.0)
                            .unwrap_or([0.0; 2]),
                        expected
                    );
                }
            }
        }
        let boundary = std::f32::consts::SQRT_2 - 1.0;
        assert_eq!(
            gamepad::java_move_vector([boundary * 0.99, 1.0], 0.0),
            Some([0.0, 1.0])
        );
        assert_eq!(
            gamepad::java_move_vector([boundary * 1.01, 1.0], 0.0),
            Some(input::keyboard_direction(true, false, false, true))
        );
        for stick in [[0.05, 0.05], [f32::NAN, 1.0], [1.0, f32::INFINITY]] {
            assert_eq!(gamepad::java_move_vector(stick, 0.1), None);
        }
    }

    #[test]
    fn sprint_latch_and_joystick_sprint_clear_without_controller_options() {
        let mut t = touch(Mode::Crosshair);
        t.settings.sprint_toggle = true;
        down(&mut t, 1, Control::Sprint, 0);
        t.up(1);
        assert_eq!(t.poll_frame().edges, [(Button::LeftStick, true)]);
        assert!(t.poll_frame().pad.held(Button::LeftStick));
        down(&mut t, 2, Control::Sprint, 0);
        t.up(2);
        assert_eq!(t.poll_frame().edges, [(Button::LeftStick, false)]);
        down(&mut t, 3, Control::Sprint, 0);
        t.poll_frame();
        assert_eq!(t.clear().edges, [(Button::LeftStick, false)]);
        assert_eq!(t.poll_frame(), TouchFrame::default());
        for mode in Mode::ALL {
            let mut t = touch(mode);
            t.settings.sprint_with_joystick = true;
            assert!(!t.visible(Control::Sprint));
            assert!(t.down(1, point(&t, [0.0, 0.8]), Duration::ZERO));
            t.moved(1, point(&t, [0.0, 1.2]));
            assert_eq!(t.poll_frame().pad.held(Button::LeftStick), mode != Mode::Dpad);
            t.moved(1, point(&t, [0.0, 0.8]));
            assert!(!t.poll_frame().pad.held(Button::LeftStick));
        }
    }

    #[test]
    fn dpad_sneak_uses_an_explicit_clock_but_descend_and_dismount_hold() {
        for elapsed in [299, 300] {
            let mut t = touch(Mode::Dpad);
            down(&mut t, 1, Control::Sneak, 100);
            t.up(1);
            assert!(t.poll_frame().edges.is_empty());
            down(&mut t, 2, Control::Sneak, 100 + elapsed);
            t.up(2);
            let expected = if elapsed < 300 {
                vec![(Button::RightStick, true)]
            } else {
                vec![]
            };
            assert_eq!(t.poll_frame().edges, expected);
            t.clear();
            down(&mut t, 3, Control::Sneak, 0);
            t.up(3);
            assert!(t.poll_frame().edges.is_empty());
            down(&mut t, 4, Control::Sneak, 299);
            assert_eq!(t.poll_frame().edges, [(Button::RightStick, true)]);
            t.up(4);
            assert_eq!(t.poll_frame().edges, [(Button::RightStick, false)]);
        }
        let mut t = touch(Mode::Dpad);
        down(&mut t, 1, Control::Sneak, 100);
        t.up(1);
        down(&mut t, 2, Control::Sneak, 50);
        t.up(2);
        assert!(t.poll_frame().edges.is_empty());
        for context in [
            TouchContext {
                vertical_movement: true,
                ..TouchContext::default()
            },
            TouchContext {
                swimming: true,
                ..TouchContext::default()
            },
            TouchContext {
                mounted: true,
                ..TouchContext::default()
            },
        ] {
            let mut t = touch(Mode::Dpad);
            t.context = context;
            down(&mut t, 1, Control::Sneak, 0);
            assert_eq!(t.poll_frame().edges, [(Button::RightStick, true)]);
            t.context = TouchContext::default();
            t.moved(1, [0.55, 0.4]);
            assert!(t.poll_frame().edges.is_empty());
            t.up(1);
            assert_eq!(t.poll_frame().edges, [(Button::RightStick, false)]);
        }
    }

    #[test]
    fn schemes_visibility_and_shared_geometry_do_not_rewrite_layouts() {
        for ((mode, split, _), crosshair) in Mode::SCHEMES.into_iter().zip([true, false, false, true]) {
            let mut t = touch(mode);
            t.settings.split_controls = split;
            assert_eq!(t.visible(Control::Attack), crosshair);
            assert_eq!(t.visible(Control::Use), crosshair);
            t.settings.button_visibility[Control::Attack as usize] = 1;
            assert!(t.visible(Control::Attack));
            t.settings.button_visibility[Control::Chat as usize] = 2;
            assert_eq!(t.control_at(t.placement(Control::Chat).center), None);
            assert!(!t.visible(Control::Shield));
        }
        let mut t = touch(Mode::Crosshair);
        let stored = t.settings.layouts;
        let before = t.placement(Control::Jump);
        let outside = [before.center[0] + before.size / ASPECT * 0.7, before.center[1]];
        assert_eq!(t.control_at(outside), None);
        t.settings.button_scale = 1.5;
        assert_eq!(t.control_at(outside), Some(Control::Jump));
        assert_eq!(t.settings.layouts, stored);
        t.settings.button_scale = 1.0;
        t.settings.left_handed = true;
        assert!((t.placement(Control::Move).center[0] - 0.87).abs() < 1e-5);
        for (w, h) in [(1920, 1080), (1080, 1920), (0, 0)] {
            t.viewport(w, h);
            let aspect = w.max(1) as f32 / h.max(1) as f32;
            for control in Control::ALL {
                let [x, y, width, height] = t.placement(control).rect(aspect);
                assert!(x >= 0.0 && y >= 0.0 && x + width <= 1.0001 && y + height <= 1.0001);
                assert!((width * aspect - height).abs() < 1e-5);
            }
        }
        let p = &mut t.settings.layouts[0][Control::Jump as usize];
        p.center = [f32::NAN, f32::INFINITY];
        p.size = f32::NAN;
        p.opacity = f32::NAN;
        t.settings.button_scale = f32::INFINITY;
        let p = t.placement(Control::Jump);
        assert_eq!(p.center, [0.5; 2]);
        assert_eq!(p.size, 0.15);
        assert_eq!(p.opacity, 0.65);
        assert!(!t.down(99, [f32::NAN, 0.0], Duration::ZERO));
    }

    #[test]
    fn requests_are_queued_and_clear_discards_unpolled_work() {
        let mut t = touch(Mode::Crosshair);
        t.settings.offhand_button = true;
        let requests = [
            (Control::Chat, TouchRequest::Chat),
            (Control::Settings, TouchRequest::Settings),
            (Control::Debug, TouchRequest::Debug),
            (Control::Perspective, TouchRequest::Perspective),
            (Control::Offhand, TouchRequest::Offhand),
        ];
        for (id, (control, _)) in requests.into_iter().enumerate() {
            down(&mut t, id as u64, control, 0);
            down(&mut t, id as u64, control, 0);
            t.up(id as u64);
        }
        assert_eq!(t.poll_frame().requests, requests.map(|(_, request)| request));
        assert_eq!(t.poll_frame(), TouchFrame::default());
        down(&mut t, 10, Control::Move, 0);
        down(&mut t, 11, Control::Jump, 0);
        down(&mut t, 12, Control::Attack, 0);
        t.moved(10, point(&t, [0.0, 0.8]));
        assert_eq!(
            t.poll_frame().edges,
            [(Button::South, true), (Button::RightTrigger, true)]
        );
        down(&mut t, 13, Control::Chat, 0);
        down(&mut t, 14, Control::Use, 0);
        down(&mut t, 15, Control::Offhand, 0);
        let released = t.clear();
        assert_eq!(
            released.edges,
            [(Button::South, false), (Button::RightTrigger, false)]
        );
        assert!(released.requests.is_empty());
        assert_eq!(released.movement, None);
        assert_eq!(t.poll_frame(), TouchFrame::default());
        for id in 10..=15 {
            assert!(!t.owns_pointer(id));
            t.up(id);
        }
    }
}
