//! Desktop checks for touch formats and value contracts.
use horizon_client::{
    gamepad::PadState,
    touch::{
        Control, CustomButton, Mode, Placement, Settings, TouchContext, TouchFeedback, TouchFrame,
        TouchRequest, TouchScreen, TouchTap,
        layout::{FORMAT_VERSION, contains},
    },
};
use std::collections::VecDeque;

#[test]
fn touch_defaults_and_four_schemes_share_three_layouts() {
    let mut s = Settings::parse("{}").unwrap();
    assert_eq!(s, Settings::default());
    assert_eq!(s.version, FORMAT_VERSION);
    assert!(!s.crosshair_gestures && !s.analog_movement && !s.virtual_mouse);
    assert!(s.sprint_button && !s.sprint_toggle && !s.sprint_with_joystick);
    assert!(!s.offhand_button && !s.shield_button && !s.contextual_interact);
    assert_eq!(Mode::ALL.len(), 3);
    assert_eq!(Control::ALL.len(), 14);
    for ((mode, split, _), crosshair) in Mode::SCHEMES.into_iter().zip([true, false, false, true]) {
        s.mode = mode;
        s.split_controls = split;
        assert_eq!(s.crosshair(), crosshair);
        assert_eq!(s.visible(Control::Attack), crosshair);
        assert_eq!(s.visible(Control::Use), crosshair);
    }
    let before = s.placement(Control::Move);
    s.split_controls = false;
    assert_eq!(s.placement(Control::Move), before);
    s.button_visibility[Control::Attack as usize] = 1;
    s.button_visibility[Control::Chat as usize] = 2;
    assert!(s.visible(Control::Attack));
    assert!(!s.visible(Control::Chat));
}

#[test]
fn touch_settings_and_custom_buttons_roundtrip() {
    let mut s = Settings {
        mode: Mode::Dpad,
        split_controls: true,
        left_handed: true,
        swap_jump_sneak: true,
        sensitivity_horizontal: 2.0,
        virtual_mouse: true,
        custom_buttons: vec![
            CustomButton::new("key.keyboard.unknown".into(), 0),
            CustomButton::new("key.keyboard.z".into(), 1),
            CustomButton::new("action:key.jump".into(), 2),
        ],
        ..Settings::default()
    };
    s.layouts[0][6].center = [0.25, 0.2];
    s.custom_buttons[1].layouts[2].opacity = 0.35;
    s.button_visibility[Control::Attack as usize] = 2;
    assert_eq!(Settings::parse(&s.to_json().unwrap()).unwrap(), s);
    for token in [
        "GLFW_KEY_Z",
        "key.mouse.left",
        "action:no-such-action",
        "key.keyboard.no-such-key",
    ] {
        assert!(!CustomButton::valid_token(token));
    }
}

#[test]
fn touch_legacy_rows_append_defaults_without_moving_saved_controls() {
    let defaults = Settings::default();
    for count in [10, 12, 14] {
        let mut json = serde_json::to_value(&defaults).unwrap();
        json.as_object_mut().unwrap().remove("version");
        json["button_visibility"]
            .as_array_mut()
            .unwrap()
            .truncate(12);
        for row in json["layouts"].as_array_mut().unwrap() {
            row.as_array_mut().unwrap().truncate(count);
            row[6]["center"] = serde_json::json!([0.25, 0.2]);
            row[6]["opacity"] = serde_json::json!(0.35);
        }
        let s = Settings::parse(&json.to_string()).unwrap();
        for (row, default) in s.layouts.iter().zip(defaults.layouts) {
            assert_eq!(row[6].center, [0.25, 0.2]);
            assert_eq!(row[6].opacity, 0.35);
            assert_eq!(&row[count..], &default[count..]);
        }
        assert_eq!(&s.button_visibility[12..], &[0u8, 0]);
        assert_eq!(Settings::parse(&s.to_json().unwrap()).unwrap(), s);
    }
}

#[test]
fn touch_malformed_shapes_and_unknown_versions_are_errors() {
    for bad in [
        r#"{"version":0}"#,
        r#"{"version":2}"#,
        r#"{"mode":"unknown"}"#,
        r#"{"layouts":[]}"#,
        r#"{"button_visibility":[0]}"#,
        "null",
    ] {
        assert!(Settings::parse(bad).is_err(), "{bad}");
    }
    for count in [9, 11, 13, 15] {
        let mut json = serde_json::to_value(Settings::default()).unwrap();
        let row = json["layouts"][0][0].clone();
        json["layouts"][0] = serde_json::json!(vec![row; count]);
        assert!(Settings::parse(&json.to_string()).is_err());
    }
}

#[test]
fn touch_sanitize_bounds_nonfinite_values_and_limits_buttons() {
    let mut s = Settings {
        sensitivity: f32::NAN,
        sensitivity_horizontal: -1.0,
        sensitivity_vertical: 9.0,
        button_scale: f32::INFINITY,
        circle_size: f32::NEG_INFINITY,
        virtual_mouse_sensitivity: 9.0,
        horizontal_margin: f32::NAN,
        vertical_margin: 1.0,
        custom_buttons: vec![CustomButton::new("action:key.attack".into(), 0); 40],
        ..Settings::default()
    };
    s.custom_buttons[0].key = "GLFW_KEY_Z".into();
    let bad = Placement {
        center: [f32::NAN, f32::INFINITY],
        size: f32::NAN,
        opacity: f32::NEG_INFINITY,
    };
    s.layouts[0][0] = bad;
    s.custom_buttons[1].layouts[2] = bad;
    s.button_visibility[0] = 255;
    let encoded = s.to_json().unwrap();
    assert!(s.sensitivity.is_nan());
    s.sanitize();
    assert_eq!(
        [s.sensitivity, s.sensitivity_horizontal, s.sensitivity_vertical],
        [1.0, 0.1, 3.0]
    );
    assert_eq!(
        [s.button_scale, s.circle_size, s.virtual_mouse_sensitivity],
        [1.0, 32.0, 3.0]
    );
    assert_eq!([s.horizontal_margin, s.vertical_margin], [0.0, 0.12]);
    assert_eq!(s.custom_buttons.len(), 31);
    assert_eq!(s.button_visibility[0], 0);
    assert_eq!(s.layouts[0][0], s.custom_buttons[0].layouts[2]);
    assert_eq!(
        s.layouts[0][0],
        Placement {
            center: [0.5; 2],
            size: 0.15,
            opacity: 0.65,
        }
    );
    assert_eq!(Settings::parse(&encoded).unwrap(), s);
    let mut p = Placement {
        center: [-1.0, 2.0],
        size: 9.0,
        opacity: -1.0,
    };
    p.sanitize();
    assert_eq!(p.center, [0.025, 0.975]);
    assert_eq!([p.size, p.opacity], [0.45, 0.0]);
}

#[test]
fn touch_square_geometry_and_mirroring_leave_stored_layouts_alone() {
    let mut s = Settings {
        mode: Mode::Dpad,
        ..Settings::default()
    };
    let stored = s.layouts;
    for aspect in [16.0 / 9.0, 9.0 / 16.0] {
        let p = s.placement(Control::Jump);
        let rect = p.rect(aspect);
        assert!((rect[2] * aspect - rect[3]).abs() < 0.00001);
        assert!(contains(rect, p.center));
        assert!(!contains(rect, [0.0; 2]));
    }
    s.left_handed = true;
    s.swap_jump_sneak = true;
    s.button_scale = 2.0;
    let jump = s.placement(Control::Jump);
    assert_eq!(
        jump.center,
        [1.0 - stored[2][3].center[0], stored[2][3].center[1]]
    );
    assert_eq!(jump.size, stored[2][3].size);
    assert_eq!(s.layouts, stored);
    s.mode = Mode::Tap;
    s.split_controls = true;
    s.sanitize();
    assert_eq!(s.mode, Mode::Crosshair);
    assert!(!s.split_controls && s.crosshair_gestures);
}

#[test]
fn touch_default_contracts_are_inactive_and_neutral() {
    assert_eq!(TouchContext::default().screen, TouchScreen::Inactive);
    let frame = TouchFrame::default();
    assert_eq!(frame.pad, PadState::default());
    assert!(frame.movement.is_none() && frame.edges.is_empty());
    assert_eq!(frame.look_degrees, [0.0; 2]);
    assert!(frame.aim.is_none() && frame.taps.is_empty());
    assert!(frame.key_events.is_empty() && frame.requests.is_empty());
    assert!(!frame.holding && !frame.holding_use && !frame.shield_held);
    assert_eq!(
        TouchFeedback::default(),
        TouchFeedback {
            latch_use: false,
            break_progress: 0.0,
        }
    );
}

#[test]
fn touch_frame_owns_two_taps_and_consumes_each_aim_and_intent_once() {
    let mut context = TouchContext {
        screen: TouchScreen::Playing,
        held_item: Some("minecraft:bread".into()),
        hold_uses_item: true,
        ..TouchContext::default()
    };
    let snapshot = context.clone();
    let first = TouchTap {
        aim: [0.2, 0.3],
        held_use: context.hold_uses_item,
    };
    context.held_item = None;
    context.hold_uses_item = false;
    let second = TouchTap {
        aim: [0.7, 0.8],
        held_use: context.hold_uses_item,
    };
    let mut frame = TouchFrame {
        taps: VecDeque::from([first, second]),
        key_events: vec![("action:key.jump".into(), true)],
        requests: vec![TouchRequest::Settings],
        ..TouchFrame::default()
    };
    frame.aim = Some([0.5; 2]);
    assert_eq!(snapshot.held_item.as_deref(), Some("minecraft:bread"));
    assert!(context.held_item.is_none());
    assert_eq!(frame.taps.pop_front(), Some(first));
    assert_eq!(frame.taps.pop_front(), Some(second));
    assert_eq!(frame.taps.pop_front(), None);
    assert_eq!(frame.taps.pop_front(), None);
    assert_eq!(
        std::mem::take(&mut frame.key_events),
        [("action:key.jump".to_owned(), true)]
    );
    assert!(frame.key_events.is_empty());
}
