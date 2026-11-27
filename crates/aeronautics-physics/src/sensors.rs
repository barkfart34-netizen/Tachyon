//! Redstone sensor rules and burner variant mapping for Create Aeronautics / Simulated.

/// Computes 0..=15 redstone signal strength for `simulated:altitude_sensor`.
pub fn altitude_sensor_signal(world_y: f64, min_y: f64, max_y: f64) -> u8 {
    if !world_y.is_finite() || !min_y.is_finite() || !max_y.is_finite() || max_y <= min_y {
        return 0;
    }
    let t = ((world_y - min_y) / (max_y - min_y)).clamp(0.0, 1.0);
    (t * 15.0).round() as u8
}

/// Computes 0..=15 redstone signal strength for `simulated:velocity_sensor`.
pub fn velocity_sensor_signal(speed_mps: f64, max_speed: f64) -> u8 {
    if !speed_mps.is_finite() || !max_speed.is_finite() || max_speed <= 0.0 {
        return 0;
    }
    let t = (speed_mps.abs() / max_speed).clamp(0.0, 1.0);
    (t * 15.0).round() as u8
}

/// Maps burner heat level (0..=15) to `(powered, variant)` for `aeronautics:adjustable_burner`.
pub fn burner_state_for_heat(heat: u8) -> (bool, &'static str) {
    match heat {
        0 => (false, "fire"),
        1..=10 => (true, "fire"),
        _ => (true, "soulful"),
    }
}

/// Maps burner heat level (0..=15) to the blockstate `variant` (`"fire"` or `"soulful"`)
/// of `aeronautics:adjustable_burner`.
pub fn burner_variant_for_heat(heat: u8) -> &'static str {
    burner_state_for_heat(heat).1
}
