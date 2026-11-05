//! Desktop tests of the touch HUD and engine snapshots, without Android or a GPU.
#[cfg(test)]
mod tests {
    use horizon_client::gamepad::{Button as PadButton, PadState};
    use horizon_client::hud::{HudAtlas, HudBatch, Rect};
    use horizon_client::touch::hud::{self, Button, Control, Marker, State};
    use horizon_client::touch::{Mode, TouchContext, TouchControls, TouchFeedback, TouchRequest};
    use std::time::Duration;

    fn button(control: Control) -> Button {
        Button {
            control,
            bounds: [40.25, 50.75, 90.0, 90.0],
            opacity: 0.65,
            pressed: false,
        }
    }
    fn batch(window: [u32; 2], scale: f32) -> HudBatch {
        let mut b = HudBatch::new(window);
        b.scale = scale;
        b.width = window[0] as f32 / scale;
        b.height = window[1] as f32 / scale;
        b
    }
    fn rect(u: f32, w: u32, h: u32) -> Rect {
        Rect {
            u0: u,
            v0: 0.1,
            u1: u + 0.02,
            v1: 0.12,
            w,
            h,
        }
    }
    fn near(a: f32, b: f32) {
        assert!((a - b).abs() < 0.0001, "{a} != {b}");
    }
    fn stick(mode: Mode, left: [f32; 2]) -> State {
        State {
            mode,
            pad: PadState {
                left,
                ..Default::default()
            },
            ..Default::default()
        }
    }
    fn paint(
        a: &HudAtlas,
        buttons: &[Button],
        state: &State,
        scale: f32,
        opacity: f32,
    ) -> HudBatch {
        let mut b = batch([400, 300], scale);
        hud::draw(&mut b, a, buttons, state, opacity);
        b
    }

    #[test]
    fn all_controls_have_an_offline_fallback_and_pressed_feedback() {
        let mut a = HudAtlas::loading();
        a.sprites.clear();
        for c in Control::ALL {
            let mut control = button(c);
            let normal = paint(&a, &[control], &State::default(), 2.0, 0.5);
            assert!(!normal.vertices.is_empty(), "{c:?}");
            assert!(normal
                .vertices
                .iter()
                .all(|v| v.pos.iter().chain(&v.uv).all(|v| v.is_finite())));
            assert!(normal.vertices.iter().all(|v| v.color >> 24 <= 83));
            control.pressed = true;
            let pressed = paint(&a, &[control], &State::default(), 2.0, 0.5);
            if c != Control::Move {
                let normal = normal.vertices.iter().map(|v| v.color).collect::<Vec<_>>();
                let pressed = pressed.vertices.iter().map(|v| v.color).collect::<Vec<_>>();
                assert_ne!(normal, pressed, "{c:?}");
            }
        }
        let b = paint(&a, &[button(Control::Jump)], &State::default(), 2.0, 0.0);
        assert!(b.vertices.is_empty());
        let state = stick(Mode::Dpad, [1.0, 1.0]);
        let b = paint(&a, &[button(Control::Move)], &state, 2.0, 1.0);
        assert!(!b.vertices.is_empty());
        assert!(b.vertices.iter().all(|v| v.pos.iter().all(|v| v.is_finite())));
    }

    #[test]
    fn hit_targets_and_drawing_share_physical_bounds_across_gui_scales() {
        let a = HudAtlas::loading();
        let control = button(Control::Offhand);
        let [x, y, w, h] = control.bounds;
        for window in [[401, 901], [1201, 480]] {
            for scale in [1.0, 2.0, 3.0, 4.0] {
                let mut b = batch(window, scale);
                hud::draw(&mut b, &a, &[control], &State::default(), 1.0);
                let first = &b.vertices[..6];
                near(first[0].pos[0] * scale, x);
                near(first[0].pos[1] * scale, y);
                near(first[2].pos[0] * scale, x + w);
                near(first[2].pos[1] * scale, y + h);
                assert_eq!(hud::control_at(&[control], [x, y]), Some(Control::Offhand));
                assert_eq!(
                    hud::control_at(&[control], [x + w, y + h]),
                    Some(Control::Offhand)
                );
                assert_eq!(hud::control_at(&[control], [x - 0.01, y]), None);
            }
        }
        assert_eq!(
            hud::control_at(&[control, button(Control::Use)], [x, y]),
            Some(Control::Use)
        );
        assert_eq!(hud::control_at(&[], [x, y]), None);
        for bounds in [
            [f32::NAN, y, w, h],
            [x, y, 0.0, h],
            [x, y, w, -1.0],
            [f32::MAX, y, f32::MAX, h],
        ] {
            let invalid = Button { bounds, ..control };
            let b = paint(&a, &[invalid], &State::default(), 1.0, 1.0);
            assert!(b.vertices.is_empty());
            assert_eq!(hud::control_at(&[invalid], [x, y]), None);
        }
    }

    #[test]
    fn joystick_displacement_and_fitted_glyphs_follow_the_fork() {
        let mut a = HudAtlas::loading();
        a.sprites
            .insert("gui/bedrock/ui/joystick_frame".into(), rect(0.2, 32, 32));
        a.sprites
            .insert("gui/bedrock/ui/joystick_knob".into(), rect(0.3, 16, 16));
        let control = button(Control::Move);
        let state = stick(Mode::Crosshair, [0.6, 0.8]);
        let mut b = paint(&a, &[control], &state, 2.0, 1.0);
        assert_eq!(b.vertices.len(), 12);
        near(b.vertices[0].uv[0], 0.2 + 0.5 / a.width as f32);
        near(
            b.vertices[6].pos[0] * b.scale,
            control.bounds[0] + 90.0 * (0.33 + 0.6 * 0.25),
        );
        near(
            b.vertices[6].pos[1] * b.scale,
            control.bounds[1] + 90.0 * (0.33 - 0.8 * 0.25),
        );
        near(
            (b.vertices[7].pos[0] - b.vertices[6].pos[0]) * b.scale,
            90.0 * 0.34,
        );
        a.sprites
            .insert("gui/bedrock/ui/more-dots".into(), rect(0.4, 20, 5));
        b.vertices.clear();
        let inventory = button(Control::Inventory);
        hud::draw(&mut b, &a, &[inventory], &State::default(), 1.0);
        let glyph = &b.vertices[b.vertices.len() - 6..];
        near((glyph[1].pos[0] - glyph[0].pos[0]) * b.scale, 90.0 * 0.6);
        near(
            (glyph[2].pos[1] - glyph[1].pos[1]) * b.scale,
            90.0 * 0.6 / 4.0,
        );
        assert!(inventory.contains([inventory.bounds[0], inventory.bounds[1]]));
    }

    #[test]
    fn contextual_icons_keep_swim_fly_mount_and_dpad_priority() {
        let mut a = HudAtlas::loading();
        let cases = [
            (Control::Jump, [true, true, true, true], "controls/waterascend"),
            (Control::Sneak, [true, true, true, true], "controls/waterdescend"),
            (Control::Jump, [true, false, true, true], "controls/flyingascend"),
            (Control::Sneak, [true, false, true, true], "controls/flyingdescend"),
            (Control::Sneak, [true, false, false, true], "controls/dismount"),
            (Control::Jump, [true, false, false, false], "controls/jump_dpad"),
            (Control::Sneak, [true, false, false, false], "controls/sneak_dpad"),
            (Control::Jump, [false; 4], "ui/jump"),
            (Control::Sneak, [false; 4], "ui/sneak"),
        ];
        for (i, (_, _, name)) in cases.iter().enumerate() {
            a.sprites.insert(
                format!("gui/bedrock/{name}"),
                rect(0.2 + i as f32 * 0.02, 16, 16),
            );
            a.sprites.insert(
                format!("gui/bedrock/{name}_pressed"),
                rect(0.5 + i as f32 * 0.02, 16, 16),
            );
        }
        for (i, (c, context, _)) in cases.into_iter().enumerate() {
            let [dpad, swimming, vertical_movement, mounted] = context;
            let state = State {
                mode: if dpad { Mode::Dpad } else { Mode::Crosshair },
                context: TouchContext {
                    swimming,
                    vertical_movement,
                    mounted,
                    sneaking: c == Control::Sneak,
                    ..Default::default()
                },
                ..Default::default()
            };
            let control = Button {
                pressed: c == Control::Jump,
                ..button(c)
            };
            let b = paint(&a, &[control], &state, 1.0, 1.0);
            assert_eq!(b.vertices.len(), 6);
            near(b.vertices[0].uv[0], 0.5 + i as f32 * 0.02 + 0.5 / a.width as f32);
        }
        a.sprites.remove("gui/bedrock/ui/jump_pressed");
        let control = Button {
            pressed: true,
            ..button(Control::Jump)
        };
        let b = paint(&a, &[control], &State::default(), 1.0, 1.0);
        assert_eq!(b.vertices.len(), 6);
        assert_eq!(b.vertices[0].color.to_le_bytes(), [160, 160, 160, 166]);
    }

    #[test]
    fn dpad_draws_diagonals_only_along_the_active_vertical_row() {
        let mut a = HudAtlas::loading();
        let names = [
            "up", "left", "right", "down", "up_left", "up_right", "down_left", "down_right",
        ];
        for (i, name) in names.iter().enumerate() {
            a.sprites.insert(
                format!("gui/bedrock/controls/{name}"),
                rect(0.2 + i as f32 * 0.02, 16, 16),
            );
            a.sprites.insert(
                format!("gui/bedrock/controls/{name}_pressed"),
                rect(0.5 + i as f32 * 0.02, 16, 16),
            );
        }
        for (left, count, diagonal, pressed) in [
            ([0.0, 0.0], 24, 0.0, 0.0),
            ([1.0, 1.0], 36, 0.28, 0.6),
            ([-1.0, -1.0], 36, 0.62, 0.34),
        ] {
            let state = stick(Mode::Dpad, left);
            let b = paint(&a, &[button(Control::Move)], &state, 1.0, 1.0);
            assert_eq!(b.vertices.len(), count);
            if count == 36 {
                near(b.vertices[24].uv[0], diagonal + 0.5 / a.width as f32);
                near(b.vertices[30].uv[0], pressed + 0.5 / a.width as f32);
            }
        }
    }

    #[test]
    fn mining_fill_is_a_complete_growing_circle_with_a_stationary_rim() {
        let a = HudAtlas::loading();
        let mut rim = Vec::new();
        for (progress, radius) in [
            (0.0, 0.0),
            (0.25, 3.625),
            (0.5, 7.25),
            (1.0, 14.5),
            (2.0, 14.5),
            (f32::NAN, 0.0),
            (-1.0, 0.0),
        ] {
            let state = State {
                marker: Some(Marker {
                    point: [120.0, 100.0],
                    size: 32.0,
                    progress,
                }),
                ..Default::default()
            };
            let b = paint(&a, &[], &state, 2.0, 1.0);
            assert_eq!(b.vertices.len(), if radius == 0.0 { 768 } else { 1536 });
            let positions = b.vertices[..768].iter().map(|v| v.pos).collect::<Vec<_>>();
            if rim.is_empty() {
                rim = positions;
            } else {
                assert_eq!(positions, rim);
            }
            if radius > 0.0 {
                for segment in b.vertices[768..1152].chunks_exact(6) {
                    near(
                        (segment[0].pos[0] - 60.0).hypot(segment[0].pos[1] - 50.0),
                        radius,
                    );
                    assert_eq!(segment[2].pos, [60.0, 50.0]);
                }
                near(b.vertices[768].pos[0], 60.0 + radius);
                near(b.vertices[768 + 32 * 6].pos[0], 60.0 - radius);
            }
        }
        let state = State {
            marker: Some(Marker {
                point: [f32::NAN, 0.0],
                size: 32.0,
                progress: 1.0,
            }),
            ..Default::default()
        };
        let b = paint(&a, &[], &state, 1.0, 1.0);
        assert!(b.vertices.is_empty());
    }

    #[test]
    fn engine_snapshots_use_existing_layout_visibility_and_hit_geometry() {
        let mut t = TouchControls::default();
        assert!(t.hud_buttons().is_empty());
        for (mode, split, _) in Mode::SCHEMES {
            t.settings.mode = mode;
            t.settings.split_controls = split;
            t.settings.offhand_button = true;
            t.settings.shield_button = true;
            t.settings.button_visibility[Control::Chat as usize] = 2;
            for window in [[401, 901], [1201, 480]] {
                t.viewport(window[0], window[1]);
                let [w, h] = window.map(|v| v as f32);
                for mirrored in [false, true] {
                    t.settings.left_handed = mirrored;
                    t.settings.swap_jump_sneak = mirrored;
                    t.settings.button_scale = if mirrored { 1.5 } else { 1.0 };
                    let before = t.settings.clone();
                    let buttons = t.hud_buttons();
                    let expected = Control::ALL
                        .into_iter()
                        .filter(|c| t.visible(*c))
                        .collect::<Vec<_>>();
                    assert_eq!(buttons.iter().map(|b| b.control).collect::<Vec<_>>(), expected);
                    assert!(
                        !buttons
                            .iter()
                            .any(|b| matches!(b.control, Control::Chat | Control::Shield))
                    );
                    for b in &buttons {
                        let p = t.placement(b.control);
                        let r = p.rect(w / h);
                        let bounds = [r[0] * w, r[1] * h, r[2] * w, r[3] * h];
                        for (actual, expected) in b.bounds.into_iter().zip(bounds) {
                            near(actual, expected);
                        }
                        assert_eq!(
                            hud::control_at(&buttons, [p.center[0] * w, p.center[1] * h]),
                            t.control_at(p.center)
                        );
                    }
                    let offhand = *buttons
                        .iter()
                        .find(|b| b.control == Control::Offhand)
                        .unwrap();
                    let a = HudAtlas::loading();
                    for scale in [1.0, 2.0, 3.0, 4.0] {
                        let mut b = HudBatch::new(window);
                        b.scale = scale;
                        hud::draw(&mut b, &a, &[offhand], &State::default(), 1.0);
                        near(b.vertices[0].pos[0] * scale / w, offhand.bounds[0] / w);
                        near(b.vertices[0].pos[1] * scale / h, offhand.bounds[1] / h);
                        near(
                            b.vertices[2].pos[0] * scale / w,
                            (offhand.bounds[0] + offhand.bounds[2]) / w,
                        );
                        near(
                            b.vertices[2].pos[1] * scale / h,
                            (offhand.bounds[1] + offhand.bounds[3]) / h,
                        );
                    }
                    assert_eq!(t.settings, before);
                }
            }
        }
    }

    #[test]
    fn snapshots_keep_held_feedback_and_do_not_consume_input() {
        let mut t = TouchControls::default();
        t.viewport(1600, 900);
        t.settings.mode = Mode::Tap;
        t.context.swimming = true;
        t.context.held_item = Some("minecraft:bow".into());
        let jump = t.placement(Control::Jump).center;
        assert!(t.down(1, jump, Duration::ZERO));
        t.moved(1, [0.55, 0.4]);
        let chat = t.placement(Control::Chat).center;
        assert!(t.down(2, chat, Duration::ZERO));
        let buttons = t.hud_buttons();
        for control in [Control::Jump, Control::Chat] {
            assert!(buttons.iter().find(|b| b.control == control).unwrap().pressed);
        }
        let feedback = TouchFeedback {
            break_progress: 0.5,
            ..Default::default()
        };
        let state = t.hud_state(Some([0.2, 0.3]), feedback);
        let marker = state.marker.unwrap();
        near(marker.point[0], 320.0);
        near(marker.point[1], 270.0);
        assert_eq!(marker.size, t.settings.circle_size);
        assert_eq!(marker.progress, feedback.break_progress);
        t.context.held_item = None;
        assert_eq!(state.context.held_item.as_deref(), Some("minecraft:bow"));
        assert!(state.context.swimming && state.pad.held(PadButton::South));
        let a = HudAtlas::loading();
        let mut b = batch([1600, 900], 3.0);
        t.draw(&mut b, &a, Some([0.2, 0.3]), feedback, 1.0);
        assert!(!b.vertices.is_empty());
        let frame = t.poll_frame();
        assert_eq!(frame.edges, [(PadButton::South, true)]);
        assert_eq!(frame.requests, [TouchRequest::Chat]);
        t.clear();
        assert!(t.hud_buttons().iter().all(|b| !b.pressed));
        t.settings.mode = Mode::Crosshair;
        assert!(t.hud_state(Some([0.2, 0.3]), feedback).marker.is_none());
        t.settings.mode = Mode::Dpad;
        t.settings.split_controls = true;
        assert!(t.hud_state(Some([0.2, 0.3]), feedback).marker.is_none());
        t.settings.split_controls = false;
        t.context.force_crosshair = true;
        assert!(t.hud_state(Some([0.2, 0.3]), feedback).marker.is_none());
        t.context.force_crosshair = false;
        assert!(t.hud_state(None, feedback).marker.is_none());
    }
}
