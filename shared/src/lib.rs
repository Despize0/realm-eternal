pub mod combat;
pub mod world;

use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign, Mul, Sub};

pub const PROTOCOL_VERSION: u16 = 2;
pub const TICK_HZ: f32 = 60.0;
pub const SNAPSHOT_HZ: f32 = 30.0;
/// Asymmetric three-quarter view: an affine oblique projection. World x maps to screen x,
/// world y is foreshortened, and height rises straight up the screen. The camera never rotates.
pub const CAMERA_Y_SCALE: f32 = 0.72;
pub const CAMERA_HEIGHT_SCALE: f32 = 0.95;
/// The player sits below screen centre; the client also leads the camera toward the aim.
pub const PLAYER_ANCHOR_Y: f32 = 0.62;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };
    pub const fn new(x: f32, y: f32) -> Self { Self { x, y } }
    pub fn length(self) -> f32 { (self.x * self.x + self.y * self.y).sqrt() }
    pub fn normalized(self) -> Self {
        let len = self.length();
        if len > 0.0001 { self * (1.0 / len) } else { Self::ZERO }
    }
    pub fn distance(self, other: Self) -> f32 { (self - other).length() }
    pub fn lerp(self, other: Self, t: f32) -> Self { self + (other - self) * t }
}

impl Add for Vec2 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self { Self::new(self.x + rhs.x, self.y + rhs.y) }
}
impl AddAssign for Vec2 {
    fn add_assign(&mut self, rhs: Self) { *self = *self + rhs; }
}
impl Sub for Vec2 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self { Self::new(self.x - rhs.x, self.y - rhs.y) }
}
impl Mul<f32> for Vec2 {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self { Self::new(self.x * rhs, self.y * rhs) }
}

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub position: Vec2,
    pub zoom: f32,
    pub viewport: Vec2,
}

impl Camera {
    pub fn anchor(self) -> Vec2 {
        Vec2::new(self.viewport.x * 0.5, self.viewport.y * PLAYER_ANCHOR_Y)
    }
    /// World offset -> camera-aligned axes. The oblique camera is axis-aligned, so this is the identity;
    /// it is kept so screen-relative input and facing go through one place.
    pub fn to_view(d: Vec2) -> Vec2 { d }
    /// Camera-aligned direction -> world direction.
    pub fn from_view(v: Vec2) -> Vec2 { v }
    /// Depth key for painter's sorting: larger is nearer the camera (lower on screen).
    pub fn depth(world: Vec2) -> f32 { world.y }
    pub fn project(self, world: Vec2, height: f32) -> Vec2 {
        let a = self.anchor();
        let v = Self::to_view(world - self.position);
        Vec2::new(
            v.x * self.zoom + a.x,
            v.y * self.zoom * CAMERA_Y_SCALE + a.y - height * self.zoom * CAMERA_HEIGHT_SCALE,
        )
    }
    pub fn unproject_ground(self, screen: Vec2) -> Vec2 {
        let a = self.anchor();
        let v = Vec2::new((screen.x - a.x) / self.zoom, (screen.y - a.y) / (self.zoom * CAMERA_Y_SCALE));
        self.position + Self::from_view(v)
    }
}

pub const BTN_LIGHT: u16 = 1;
pub const BTN_HEAVY: u16 = 2;
pub const BTN_DODGE: u16 = 4;
pub const BTN_JUMP: u16 = 8;
pub const BTN_JUMP_HELD: u16 = 16;
pub const BTN_FLASK: u16 = 32;
pub const BTN_INTERACT: u16 = 64;
pub const BTN_HUB: u16 = 128;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Hello { version: u16, name: String },
    /// Movement and aim in world metres; `b` holds button bits (presses since the last input).
    Input { seq: u32, mx: f32, my: f32, ax: f32, ay: f32, b: u16 },
    Ping { client_ms: f64 },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ActorNet {
    pub u: u32,
    pub k: u8,
    pub x: f32, pub y: f32, pub z: f32,
    pub fx: f32, pub fy: f32,
    pub vx: f32, pub vy: f32,
    pub hp: f32, pub mhp: f32, pub ch: f32,
    pub po: f32,
    pub s: u8,
    pub t: f32,
    /// Attack index into `combat::ALL_ATTACKS`, or -1.
    pub a: i16,
    pub ax: f32, pub ay: f32,
    pub r: f32, pub d: f32,
    pub fl: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub n: Option<String>,
    pub lv: u8,
    pub band: u8,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProjNet { pub i: u32, pub l: u8, pub x: f32, pub y: f32, pub z: f32, pub dx: f32, pub dy: f32 }

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PortalNet { pub i: u32, pub x: f32, pub y: f32, pub to: u8, pub ttl: f32 }

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MeNet { pub uid: u32, pub name: String, pub stamina: f32, pub flasks: u8, pub level: u32, pub xp: u32, pub next: u32 }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "e", rename_all = "snake_case")]
pub enum EventNet {
    Dmg { x: f32, y: f32, z: f32, v: i32, p: bool },
    Hurt,
    Msg { text: String, secs: f32 },
    Shake { a: f32 },
    Stagger { x: f32, y: f32 },
    Level { lv: u32 },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub tick: u64,
    pub me: MeNet,
    pub actors: Vec<ActorNet>,
    pub projs: Vec<ProjNet>,
    pub portals: Vec<PortalNet>,
    pub events: Vec<EventNet>,
    pub stage: String,
    pub gate: bool,
    pub mist: bool,
    pub phase2: bool,
    pub online: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FallenNet { pub name: String, pub level: u32, pub place: String }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Welcome { id: u32, tick_hz: f32 },
    Enter { world: world::WorldKind, seed: u32, instance: u64 },
    Snap(Snapshot),
    /// Permadeath: the character is gone and has been laid in the memorial hall.
    Died { name: String, level: u32, place: String },
    Memorial { fallen: Vec<FallenNet> },
    Pong { client_ms: f64 },
    Error { message: String },
}

pub fn validate_step(current: Vec2, direction: Vec2, speed: f32, dt: f32) -> Vec2 {
    current + direction.normalized() * speed * dt.min(1.0 / 10.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_round_trip_on_ground() {
        let camera = Camera {
            position: Vec2::new(44.0, -25.0),
            zoom: 1.7,
            viewport: Vec2::new(1280.0, 720.0),
        };
        let point = Vec2::new(125.0, 390.0);
        let back = camera.unproject_ground(camera.project(point, 0.0));
        assert!((point.x - back.x).abs() < 0.001);
        assert!((point.y - back.y).abs() < 0.001);
    }

    #[test]
    fn screen_up_is_camera_relative() {
        // W (screen up) must move the knight straight up the screen.
        let cam = Camera { position: Vec2::ZERO, zoom: 1.0, viewport: Vec2::new(800.0, 600.0) };
        let w = Camera::from_view(Vec2::new(0.0, -1.0));
        let a = cam.project(Vec2::ZERO, 0.0);
        let b = cam.project(w * 10.0, 0.0);
        assert!((a.x - b.x).abs() < 0.001 && b.y < a.y);
        let v = Camera::to_view(Camera::from_view(Vec2::new(0.3, -0.8)));
        assert!((v.x - 0.3).abs() < 1e-5 && (v.y + 0.8).abs() < 1e-5);
    }

    #[test]
    fn diagonal_movement_is_not_faster() {
        let p = validate_step(Vec2::ZERO, Vec2::new(1.0, 1.0), 100.0, 0.1);
        assert!((p.length() - 10.0).abs() < 0.001);
    }
}
