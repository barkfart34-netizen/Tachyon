//! Physical classification of `aeronautics:*`, `simulated:*`, `offroad:*`, `create:*` and vanilla blocks.

/// Wheel tire sizes available on `offroad:wheel_mount`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TireKind {
    Small,
    Standard,
    Large,
    Monstrous,
}

impl TireKind {
    /// Parses an offroad tire item ID (`offroad:small_tire`, etc.).
    pub fn from_item_id(id: &str) -> Option<Self> {
        match id {
            "offroad:small_tire" => Some(Self::Small),
            "offroad:tire" => Some(Self::Standard),
            "offroad:large_tire" => Some(Self::Large),
            "offroad:monstrous_tire" => Some(Self::Monstrous),
            _ => None,
        }
    }

    /// Wheel contact radius in blocks.
    pub fn radius(self) -> f64 {
        match self {
            Self::Small => 0.55,
            Self::Standard => 0.85,
            Self::Large => 1.25,
            Self::Monstrous => 1.85,
        }
    }

    /// Unloaded raycast suspension length below the mount center.
    pub fn rest_length(self) -> f64 {
        self.radius() + 0.45
    }

    /// Vertical spring stiffness (N/m per compression meter).
    pub fn stiffness(self) -> f64 {
        match self {
            Self::Small => 9_000.0,
            Self::Standard => 14_000.0,
            Self::Large => 22_000.0,
            Self::Monstrous => 36_000.0,
        }
    }

    /// Vertical shock damper coefficient.
    pub fn damping(self) -> f64 {
        match self {
            Self::Small => 1_800.0,
            Self::Standard => 2_600.0,
            Self::Large => 4_000.0,
            Self::Monstrous => 6_200.0,
        }
    }

    /// Maximum longitudinal tractive force per wheel (Newtons).
    pub fn max_drive_force(self) -> f64 {
        match self {
            Self::Small => 5_500.0,
            Self::Standard => 9_500.0,
            Self::Large => 16_000.0,
            Self::Monstrous => 28_000.0,
        }
    }
}

/// Functional role of a block inside a 6-DOF contraption.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockRole {
    /// Normal structural block.
    Hull,
    /// Airtight / hot-air balloon envelope fabric (`aeronautics:*_envelope`).
    Envelope,
    /// Adjustable or blaze heat source (`aeronautics:adjustable_burner`, `create:blaze_burner`).
    Burner,
    /// Anti-gravity block (`aeronautics:levitite`, `aeronautics:pearlescent_levitite`).
    Levitite,
    /// Forward/directional propeller thrust producer.
    Propeller,
    /// Gyroscopic stabilized propeller (adds attitude control authority).
    GyroPropeller,
    /// Steam jet vent (`aeronautics:steam_vent`).
    SteamVent,
    /// Aerodynamic wing/sail surface (`simulated:*_symmetric_sail`, `create:*_sail`, `create:sail_frame`).
    Sail,
    /// Portable combustion/rotational engine (`simulated:*_portable_engine`).
    Engine,
    /// Pilot helm (`simulated:steering_wheel`) or throttle (`simulated:throttle_lever`).
    Helm,
    /// Ground wheel mount (`offroad:wheel_mount`).
    WheelMount,
    /// Physics assembler core (`simulated:physics_assembler`).
    Assembler,
    /// Telemetry sensor (`simulated:altitude_sensor`, `simulated:velocity_sensor`).
    Sensor,
    /// Pilot seat (`create:*_seat`, `simulated:*_seat`).
    Seat,
}

/// Static physical metadata derived from a block's ID.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockMass {
    pub mass_kg: f64,
    pub drag_area: f64,
    pub buoyant_lift_n: f64,
    pub role: BlockRole,
}

/// Returns `true` if a block must never be absorbed into a flying/driving contraption.
pub fn is_unmovable(id: &str) -> bool {
    matches!(
        id,
        "minecraft:air"
            | "minecraft:cave_air"
            | "minecraft:void_air"
            | "minecraft:water"
            | "minecraft:lava"
            | "minecraft:bedrock"
            | "minecraft:obsidian"
            | "minecraft:crying_obsidian"
            | "minecraft:barrier"
            | "minecraft:end_portal"
            | "minecraft:end_portal_frame"
            | "minecraft:nether_portal"
            | "minecraft:command_block"
            | "minecraft:chain_command_block"
            | "minecraft:repeating_command_block"
            | "minecraft:structure_block"
            | "minecraft:jigsaw"
            | "minecraft:reinforced_deepslate"
    )
}

/// Returns `true` if `id` is any Create Aeronautics envelope block.
pub fn is_envelope(id: &str) -> bool {
    let Some(name) = id.strip_prefix("aeronautics:") else {
        return false;
    };
    name.ends_with("_envelope") || name.ends_with("_envelope_encased_shaft")
}

/// Returns `true` if `id` is an aerodynamic sail or wing block.
pub fn is_sail(id: &str) -> bool {
    if let Some(name) = id.strip_prefix("simulated:") {
        return name.ends_with("_symmetric_sail");
    }
    if let Some(name) = id.strip_prefix("create:") {
        return name.ends_with("_sail") || name == "sail_frame";
    }
    false
}

/// Returns `true` if `id` is a portable engine block.
pub fn is_engine(id: &str) -> bool {
    id.strip_prefix("simulated:")
        .is_some_and(|name| name.ends_with("_portable_engine"))
        || id == "create:creative_motor"
}

/// Returns `true` if `id` is a vehicle/contraption seat block.
pub fn is_seat(id: &str) -> bool {
    id.strip_prefix("create:")
        .or_else(|| id.strip_prefix("simulated:"))
        .is_some_and(|name| name.ends_with("_seat"))
}

/// Computes physical properties for a block ID.
pub fn classify(id: &str) -> BlockMass {
    if is_envelope(id) {
        return BlockMass {
            mass_kg: 12.0,
            drag_area: 0.7,
            buoyant_lift_n: 0.0,
            role: BlockRole::Envelope,
        };
    }
    if is_sail(id) {
        return BlockMass {
            mass_kg: 15.0,
            drag_area: 1.4,
            buoyant_lift_n: 0.0,
            role: BlockRole::Sail,
        };
    }
    if is_engine(id) {
        return BlockMass {
            mass_kg: 180.0,
            drag_area: 0.9,
            buoyant_lift_n: 0.0,
            role: BlockRole::Engine,
        };
    }
    if is_seat(id) {
        return BlockMass {
            mass_kg: 35.0,
            drag_area: 0.4,
            buoyant_lift_n: 0.0,
            role: BlockRole::Seat,
        };
    }
    match id {
        "aeronautics:adjustable_burner" | "create:blaze_burner" => BlockMass {
            mass_kg: 95.0,
            drag_area: 0.8,
            buoyant_lift_n: 0.0,
            role: BlockRole::Burner,
        },
        "aeronautics:levitite" | "aeronautics:pearlescent_levitite" => BlockMass {
            mass_kg: 30.0,
            drag_area: 0.8,
            // Strong upward anti-gravity lift per block (~350 kg equivalent payload).
            buoyant_lift_n: 3_500.0,
            role: BlockRole::Levitite,
        },
        "aeronautics:propeller_bearing"
        | "aeronautics:wooden_propeller"
        | "aeronautics:andesite_propeller"
        | "aeronautics:smart_propeller" => BlockMass {
            mass_kg: 85.0,
            drag_area: 0.5,
            buoyant_lift_n: 0.0,
            role: BlockRole::Propeller,
        },
        "aeronautics:gyroscopic_propeller_bearing" => BlockMass {
            mass_kg: 110.0,
            drag_area: 0.5,
            buoyant_lift_n: 0.0,
            role: BlockRole::GyroPropeller,
        },
        "aeronautics:steam_vent" => BlockMass {
            mass_kg: 45.0,
            drag_area: 0.6,
            buoyant_lift_n: 0.0,
            role: BlockRole::SteamVent,
        },
        "simulated:steering_wheel" | "simulated:throttle_lever" => BlockMass {
            mass_kg: 25.0,
            drag_area: 0.35,
            buoyant_lift_n: 0.0,
            role: BlockRole::Helm,
        },
        "offroad:wheel_mount" | "offroad:borehead_bearing" => BlockMass {
            mass_kg: 130.0,
            drag_area: 0.65,
            buoyant_lift_n: 0.0,
            role: BlockRole::WheelMount,
        },
        "simulated:physics_assembler" => BlockMass {
            mass_kg: 140.0,
            drag_area: 0.9,
            buoyant_lift_n: 0.0,
            role: BlockRole::Assembler,
        },
        "simulated:altitude_sensor" | "simulated:velocity_sensor" => BlockMass {
            mass_kg: 20.0,
            drag_area: 0.4,
            buoyant_lift_n: 0.0,
            role: BlockRole::Sensor,
        },
        _ => BlockMass {
            mass_kg: default_mass_kg(id),
            drag_area: 0.85,
            buoyant_lift_n: 0.0,
            role: BlockRole::Hull,
        },
    }
}

fn default_mass_kg(id: &str) -> f64 {
    if id.contains("netherite") || id.contains("anvil") || id.contains("iron_block") {
        450.0
    } else if id.contains("copper") || id.contains("brass") || id.contains("zinc") || id.contains("steel") {
        220.0
    } else if id.contains("stone") || id.contains("andesite") || id.contains("brick") || id.contains("deepslate") {
        160.0
    } else if id.contains("glass") || id.contains("window") {
        50.0
    } else if id.contains("wool") || id.contains("carpet") || id.contains("plank") || id.contains("wood") || id.contains("oak") || id.contains("spruce") || id.contains("birch") {
        45.0
    } else {
        80.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_create_and_simulated_seats() {
        assert!(is_seat("create:white_seat"));
        assert!(is_seat("create:red_seat"));
        assert_eq!(classify("create:white_seat").role, BlockRole::Seat);
        assert_eq!(classify("create:red_seat").role, BlockRole::Seat);
    }
}
