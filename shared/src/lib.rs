pub mod combat;

use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign, Mul, Sub};

pub const PROTOCOL_VERSION: u16 = 1;
pub const TICK_HZ: f32 = 30.0;
/// Angled isometric camera: the world is turned 45 degrees around and viewed
/// about 32 degrees down, so the floor is squashed and heights stand up.
pub const CAMERA_YAW_COS: f32 = std::f32::consts::FRAC_1_SQRT_2;
pub const CAMERA_Y_SCALE: f32 = 0.56;
pub const CAMERA_HEIGHT_SCALE: f32 = 0.85;
pub const PLAYER_ANCHOR_Y: f32 = 0.58;

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
    /// World offset -> camera-aligned axes (x across the screen, y into it).
    pub fn to_view(d: Vec2) -> Vec2 {
        Vec2::new((d.x - d.y) * CAMERA_YAW_COS, (d.x + d.y) * CAMERA_YAW_COS)
    }
    /// Camera-aligned direction -> world direction (inverse of `to_view`).
    pub fn from_view(v: Vec2) -> Vec2 {
        Vec2::new((v.x + v.y) * CAMERA_YAW_COS, (v.y - v.x) * CAMERA_YAW_COS)
    }
    /// Depth key for painter's sorting: larger is nearer the camera.
    pub fn depth(world: Vec2) -> f32 { world.x + world.y }
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Hello { version: u16, name: String },
    Input { sequence: u32, dx: f32, dy: f32, aim_x: f32, aim_y: f32, firing: bool },
    Ping { client_ms: f64 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlayerSnapshot {
    pub id: u64,
    pub name: String,
    pub position: Vec2,
    pub hp: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Welcome { id: u64, tick_hz: f32 },
    Snapshot { tick: u64, acknowledged: u32, players: Vec<PlayerSnapshot> },
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
