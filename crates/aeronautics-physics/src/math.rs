//! Vector, quaternion, facing and atmosphere primitives for 6-DOF contraptions.

/// Three-dimensional position or vector in world/local space (meters = blocks).
pub type Vec3 = [f64; 3];

/// Unit quaternion stored as `[w, x, y, z]`.
pub type Quat = [f64; 4];

/// Integer block grid coordinates `[x, y, z]`.
pub type Pos = [i32; 3];

pub const IDENTITY_QUAT: Quat = [1.0, 0.0, 0.0, 0.0];
pub const GRAVITY: f64 = 9.81;

/// Cardinal / vertical block orientation matching Minecraft/Horizon facings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Facing {
    #[default]
    North,
    South,
    West,
    East,
    Down,
    Up,
}

impl Facing {
    /// Parses a blockstate `facing` property string.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "north" => Some(Self::North),
            "south" => Some(Self::South),
            "west" => Some(Self::West),
            "east" => Some(Self::East),
            "down" => Some(Self::Down),
            "up" => Some(Self::Up),
            _ => None,
        }
    }

    /// Lowercase blockstate property string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::North => "north",
            Self::South => "south",
            Self::West => "west",
            Self::East => "east",
            Self::Down => "down",
            Self::Up => "up",
        }
    }

    /// Unit direction vector in Minecraft coordinates (`+X` east, `+Y` up, `+Z` south).
    pub fn unit(self) -> Vec3 {
        match self {
            Self::North => [0.0, 0.0, -1.0],
            Self::South => [0.0, 0.0, 1.0],
            Self::West => [-1.0, 0.0, 0.0],
            Self::East => [1.0, 0.0, 0.0],
            Self::Down => [0.0, -1.0, 0.0],
            Self::Up => [0.0, 1.0, 0.0],
        }
    }

    /// Yaw angle in degrees matching Minecraft horizontal orientation.
    pub fn yaw_deg(self) -> f32 {
        match self {
            Self::South => 0.0,
            Self::West => 90.0,
            Self::North => 180.0,
            Self::East => 270.0,
            Self::Up | Self::Down => 0.0,
        }
    }

    /// Rotates a horizontal facing clockwise (viewed from above) by `steps` 90-degree turns.
    pub fn rotate_cw(self, steps: i32) -> Self {
        let s = steps.rem_euclid(4);
        let horiz = [Self::North, Self::East, Self::South, Self::West];
        if let Some(idx) = horiz.iter().position(|&f| f == self) {
            horiz[(idx + s as usize) % 4]
        } else {
            self
        }
    }
}

#[inline]
pub fn v_add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

#[inline]
pub fn v_sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
pub fn v_scale(v: Vec3, s: f64) -> Vec3 {
    [v[0] * s, v[1] * s, v[2] * s]
}

#[inline]
pub fn v_dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
pub fn v_cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[inline]
pub fn v_len_sq(v: Vec3) -> f64 {
    v_dot(v, v)
}

#[inline]
pub fn v_len(v: Vec3) -> f64 {
    v_len_sq(v).sqrt()
}

#[inline]
pub fn v_norm(v: Vec3) -> Vec3 {
    let l = v_len(v);
    if l > 1e-12 { v_scale(v, 1.0 / l) } else { [0.0, 0.0, 0.0] }
}

/// Normalizes a quaternion, returning [`IDENTITY_QUAT`] for degenerate inputs.
pub fn quat_normalize(q: Quat) -> Quat {
    let norm = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if norm > 1e-12 && norm.is_finite() {
        let inv = 1.0 / norm;
        [q[0] * inv, q[1] * inv, q[2] * inv, q[3] * inv]
    } else {
        IDENTITY_QUAT
    }
}

/// Hamilton product `a * b`.
pub fn quat_mul(a: Quat, b: Quat) -> Quat {
    [
        a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
        a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
        a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
        a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
    ]
}

/// Conjugate of a unit quaternion (inverse rotation).
#[inline]
pub fn quat_conj(q: Quat) -> Quat {
    [q[0], -q[1], -q[2], -q[3]]
}

/// Rotates a local vector into world space (`q * [0, v] * q*`).
pub fn quat_rotate(q: Quat, v: Vec3) -> Vec3 {
    let u = [q[1], q[2], q[3]];
    let s = q[0];
    let uv = v_cross(u, v);
    let uuv = v_cross(u, uv);
    v_add(v, v_add(v_scale(uv, 2.0 * s), v_scale(uuv, 2.0)))
}

/// Rotates a world vector into local body space.
#[inline]
pub fn quat_rotate_inv(q: Quat, v: Vec3) -> Vec3 {
    quat_rotate(quat_conj(q), v)
}

/// Builds a quaternion from intrinsic Y (yaw) -> X (pitch) -> Z (roll) radians.
pub fn quat_from_euler(yaw_rad: f64, pitch_rad: f64, roll_rad: f64) -> Quat {
    let (sy, cy) = (0.5 * yaw_rad).sin_cos();
    let (sp, cp) = (0.5 * pitch_rad).sin_cos();
    let (sr, cr) = (0.5 * roll_rad).sin_cos();
    let qy = [cy, 0.0, sy, 0.0];
    let qx = [cp, sp, 0.0, 0.0];
    let qz = [cr, 0.0, 0.0, sr];
    quat_normalize(quat_mul(quat_mul(qy, qx), qz))
}

/// Extracts `(yaw_deg, pitch_deg, roll_deg)` from an orientation quaternion.
pub fn quat_to_yaw_pitch_roll(q: Quat) -> (f64, f64, f64) {
    let q = quat_normalize(q);
    let forward = quat_rotate(q, [0.0, 0.0, -1.0]);
    let up = quat_rotate(q, [0.0, 1.0, 0.0]);
    let yaw = (-forward[0]).atan2(-forward[2]).to_degrees();
    let horiz = (forward[0] * forward[0] + forward[2] * forward[2]).sqrt();
    let pitch = forward[1].atan2(horiz).to_degrees();
    let right_flat = v_norm(v_cross(forward, [0.0, 1.0, 0.0]));
    let roll = if v_len_sq(right_flat) > 1e-8 {
        v_dot(up, right_flat).clamp(-1.0, 1.0).asin().to_degrees()
    } else {
        0.0
    };
    (yaw, pitch, roll)
}

/// Integrates orientation by local angular velocity `omega_local` (rad/s) over `dt`.
pub fn quat_integrate_omega(q: Quat, omega_local: Vec3, dt: f64) -> Quat {
    let angle = v_len(omega_local) * dt;
    if angle < 1e-12 || !angle.is_finite() {
        return quat_normalize(q);
    }
    let axis = v_scale(omega_local, dt / angle);
    let (s, c) = (0.5 * angle).sin_cos();
    let dq = [c, axis[0] * s, axis[1] * s, axis[2] * s];
    quat_normalize(quat_mul(q, dq))
}

/// Spherical linear interpolation between two unit quaternions.
pub fn quat_slerp(a: Quat, b: Quat, t: f64) -> Quat {
    let t = t.clamp(0.0, 1.0);
    let mut dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    let mut target = b;
    if dot < 0.0 {
        dot = -dot;
        target = [-b[0], -b[1], -b[2], -b[3]];
    }
    if dot > 0.9995 {
        let res = [
            a[0] + (target[0] - a[0]) * t,
            a[1] + (target[1] - a[1]) * t,
            a[2] + (target[2] - a[2]) * t,
            a[3] + (target[3] - a[3]) * t,
        ];
        return quat_normalize(res);
    }
    let theta_0 = dot.clamp(-1.0, 1.0).acos();
    let theta = theta_0 * t;
    let sin_theta = theta.sin();
    let sin_theta_0 = theta_0.sin();
    let s0 = (theta.cos()) - dot * sin_theta / sin_theta_0;
    let s1 = sin_theta / sin_theta_0;
    quat_normalize([
        s0 * a[0] + s1 * target[0],
        s0 * a[1] + s1 * target[1],
        s0 * a[2] + s1 * target[2],
        s0 * a[3] + s1 * target[3],
    ])
}

/// Atmospheric air density multiplier relative to sea level (`y = 64`).
/// Drops exponentially with altitude so hot-air balloons settle at an equilibrium ceiling.
pub fn air_density(y: f64) -> f64 {
    (-((y - 64.0).max(-64.0)) / 220.0).exp().clamp(0.15, 1.35)
}
