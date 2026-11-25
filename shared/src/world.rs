//! Deterministic world generation shared by the server and the browser client.
//! Both sides build the same tile map from `(kind, seed)`, so only the seed crosses the wire.

use crate::Vec2;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldKind { Concourse, Realm, Cathedral }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Tile {
    Void = 0,
    DeepWater,
    Water,
    Sand,
    Grass,
    Meadow,
    Forest,
    Highland,
    Rock,
    Ash,
    Stone,
    StoneWall,
}

impl Tile {
    pub fn from_u8(v: u8) -> Tile {
        use Tile::*;
        [Void, DeepWater, Water, Sand, Grass, Meadow, Forest, Highland, Rock, Ash, Stone, StoneWall][(v as usize).min(11)]
    }
    pub fn walkable(self) -> bool {
        !matches!(self, Tile::Void | Tile::DeepWater | Tile::Water | Tile::Rock | Tile::StoneWall)
    }
    /// Solid tiles stop projectiles and block melee; water does not.
    pub fn solid(self) -> bool { matches!(self, Tile::Void | Tile::Rock | Tile::StoneWall) }
    /// ROTMG-style difficulty band: shore 0 → godlands 4.
    pub fn band(self) -> u8 {
        match self { Tile::Sand => 0, Tile::Grass => 1, Tile::Meadow | Tile::Forest => 2, Tile::Highland | Tile::Rock => 3, Tile::Ash => 4, _ => 0 }
    }
}

pub fn band_name(b: u8) -> &'static str {
    ["The Shore", "The Lowlands", "The Midlands", "The Highlands", "The Godlands"][b.min(4) as usize]
}

// ---------------------------------------------------------------- noise ----

pub fn hash2(x: i32, y: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    h
}

pub fn hash01(x: i32, y: i32, seed: u32) -> f32 { (hash2(x, y, seed) >> 8) as f32 / (1u32 << 24) as f32 }

fn smooth(t: f32) -> f32 { t * t * (3.0 - 2.0 * t) }

pub fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (smooth(x - xi as f32), smooth(y - yi as f32));
    let a = hash01(xi, yi, seed); let b = hash01(xi + 1, yi, seed);
    let c = hash01(xi, yi + 1, seed); let d = hash01(xi + 1, yi + 1, seed);
    let top = a + (b - a) * fx; let bot = c + (d - c) * fx;
    top + (bot - top) * fy
}

pub fn fbm(x: f32, y: f32, seed: u32, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 0.5, 1.0, 0.0);
    for o in 0..octaves {
        sum += value_noise(x * freq, y * freq, seed.wrapping_add(o * 977)) * amp;
        norm += amp; amp *= 0.5; freq *= 2.0;
    }
    sum / norm
}

// ------------------------------------------------------------------ map ----

#[derive(Clone, Debug)]
pub struct TileMap {
    pub kind: WorldKind,
    pub seed: u32,
    pub w: i32,
    pub h: i32,
    pub tiles: Vec<u8>,
    pub spawn: Vec2,
}

#[derive(Clone, Copy, Debug)]
pub struct Prop { pub pos: Vec2, pub radius: f32, pub variant: u8, pub kind: PropKind }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropKind { Tree, Pine, DeadTree, Boulder }

pub const REALM_SIZE: i32 = 320;
pub const NEXUS_W: i32 = 36;
pub const NEXUS_H: i32 = 30;
pub const NEXUS_REALM_PORTAL: Vec2 = Vec2::new(18.0, 5.0);
pub const NEXUS_FOUNTAIN: Vec2 = Vec2::new(18.0, 15.0);

impl TileMap {
    pub fn generate(kind: WorldKind, seed: u32) -> TileMap {
        match kind {
            WorldKind::Concourse => Self::nexus(),
            WorldKind::Realm => Self::realm(seed),
            WorldKind::Cathedral => TileMap { kind, seed, w: 0, h: 0, tiles: Vec::new(), spawn: Vec2::ZERO },
        }
    }

    pub fn get(&self, x: i32, y: i32) -> Tile {
        if x < 0 || y < 0 || x >= self.w || y >= self.h { return Tile::Void; }
        Tile::from_u8(self.tiles[(y * self.w + x) as usize])
    }
    pub fn at(&self, p: Vec2) -> Tile { self.get(p.x.floor() as i32, p.y.floor() as i32) }

    fn nexus() -> TileMap {
        let (w, h) = (NEXUS_W, NEXUS_H);
        let mut tiles = vec![Tile::Stone as u8; (w * h) as usize];
        for y in 0..h { for x in 0..w {
            let edge = x == 0 || y == 0 || x == w - 1 || y == h - 1;
            let i = (y * w + x) as usize;
            if edge { tiles[i] = Tile::StoneWall as u8; }
            let d = Vec2::new(x as f32 + 0.5, y as f32 + 0.5).distance(NEXUS_FOUNTAIN);
            if d < 2.2 { tiles[i] = Tile::Water as u8; }
        }}
        TileMap { kind: WorldKind::Concourse, seed: 1, w, h, tiles, spawn: Vec2::new(18.0, 24.0) }
    }

    /// Elevation field: an island that rises from the shore toward the godlands at its heart.
    pub fn realm_elevation(x: f32, y: f32, seed: u32) -> f32 {
        let c = REALM_SIZE as f32 * 0.5;
        let dx = (x - c) / c; let dy = (y - c) / c;
        let radial = 1.0 - (dx * dx + dy * dy).sqrt();
        let n = fbm(x / 38.0, y / 38.0, seed, 5);
        radial * 1.15 + (n - 0.5) * 0.55
    }

    fn realm(seed: u32) -> TileMap {
        let (w, h) = (REALM_SIZE, REALM_SIZE);
        let mut tiles = vec![0u8; (w * h) as usize];
        for y in 0..h { for x in 0..w {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let e = Self::realm_elevation(fx, fy, seed);
            let moist = fbm(fx / 25.0, fy / 25.0, seed ^ 0x51ed, 4);
            let ridge = 1.0 - (fbm(fx / 14.0, fy / 14.0, seed ^ 0x77, 3) - 0.5).abs() * 2.0;
            let t = if e < 0.04 { Tile::DeepWater }
                else if e < 0.12 { Tile::Water }
                else if e < 0.19 { Tile::Sand }
                else if e < 0.40 { if moist > 0.6 { Tile::Meadow } else { Tile::Grass } }
                else if e < 0.60 { if moist > 0.47 { Tile::Forest } else { Tile::Meadow } }
                else if e < 0.80 { if ridge > 0.93 { Tile::Rock } else { Tile::Highland } }
                else { if ridge > 0.95 { Tile::Rock } else { Tile::Ash } };
            tiles[(y * w + x) as usize] = t as u8;
        }}
        // Lakes inland: small pockets of water in the lowlands.
        let mut map = TileMap { kind: WorldKind::Realm, seed, w, h, tiles, spawn: Vec2::ZERO };
        map.spawn = map.find_beach_spawn(0);
        map
    }

    /// A sand tile on the southern shore (players wash up on the beach, as in ROTMG).
    pub fn find_beach_spawn(&self, salt: u32) -> Vec2 {
        let c = self.w / 2;
        let start_angle = 1.2 + (hash01(salt as i32, 7, self.seed) - 0.5) * 1.6;
        for r in (20..self.w / 2).rev() {
            let p = Vec2::new(c as f32 + start_angle.cos() * r as f32, c as f32 + start_angle.sin() * r as f32);
            if self.at(p) == Tile::Sand && self.walkable(p, 0.5) { return Vec2::new(p.x.floor() + 0.5, p.y.floor() + 0.5); }
        }
        Vec2::new(c as f32, c as f32)
    }

    /// Trees, pines, dead trees and boulders are deterministic per tile.
    pub fn prop_at(&self, x: i32, y: i32) -> Option<Prop> {
        if self.kind != WorldKind::Realm { return None; }
        let t = self.get(x, y);
        let r = hash01(x, y, self.seed ^ 0xbeef);
        let (density, kind) = match t {
            Tile::Forest => (0.30, if hash01(x, y, 3) < 0.55 { PropKind::Pine } else { PropKind::Tree }),
            Tile::Meadow => (0.05, PropKind::Tree),
            Tile::Grass => (0.025, PropKind::Tree),
            Tile::Highland => (0.06, if hash01(x, y, 5) < 0.6 { PropKind::Boulder } else { PropKind::Pine }),
            Tile::Ash => (0.05, if hash01(x, y, 9) < 0.5 { PropKind::DeadTree } else { PropKind::Boulder }),
            Tile::Sand => (0.008, PropKind::Boulder),
            _ => (0.0, PropKind::Tree),
        };
        if r >= density { return None; }
        let ox = hash01(x, y, self.seed ^ 11) * 0.5 + 0.25;
        let oy = hash01(x, y, self.seed ^ 13) * 0.5 + 0.25;
        let radius = match kind { PropKind::Boulder => 0.42, PropKind::DeadTree => 0.22, _ => 0.32 };
        Some(Prop { pos: Vec2::new(x as f32 + ox, y as f32 + oy), radius, variant: (hash2(x, y, 17) % 3) as u8, kind })
    }

    pub fn props_near(&self, p: Vec2, rad: f32) -> impl Iterator<Item = Prop> + '_ {
        let (x0, x1) = ((p.x - rad).floor() as i32, (p.x + rad).floor() as i32);
        let (y0, y1) = ((p.y - rad).floor() as i32, (p.y + rad).floor() as i32);
        (y0..=y1).flat_map(move |y| (x0..=x1).filter_map(move |x| self.prop_at(x, y)))
    }

    pub fn walkable(&self, p: Vec2, r: f32) -> bool {
        let (x0, x1) = ((p.x - r).floor() as i32, (p.x + r).floor() as i32);
        let (y0, y1) = ((p.y - r).floor() as i32, (p.y + r).floor() as i32);
        for y in y0..=y1 { for x in x0..=x1 {
            if self.get(x, y).walkable() { continue; }
            // Circle vs tile box.
            let cx = p.x.clamp(x as f32, x as f32 + 1.0);
            let cy = p.y.clamp(y as f32, y as f32 + 1.0);
            if Vec2::new(cx, cy).distance(p) < r { return false; }
        }}
        for pr in self.props_near(p, r + 0.6) {
            if pr.pos.distance(p) < pr.radius + r { return false; }
        }
        true
    }

    /// Clear line for shots and melee: no solid tile or prop on the segment.
    pub fn line_clear(&self, a: Vec2, b: Vec2) -> bool {
        let d = b - a;
        let len = d.length();
        let steps = (len / 0.25).ceil().max(1.0) as i32;
        for i in 1..steps {
            let p = a + d * (i as f32 / steps as f32);
            if self.at(p).solid() { return false; }
        }
        let mid = (a + b) * 0.5;
        for pr in self.props_near(mid, len * 0.5 + 0.6) {
            if pr.kind == PropKind::DeadTree { continue; }
            let t = if len < 1e-6 { 0.0 } else { (((pr.pos - a).x * d.x + (pr.pos - a).y * d.y) / (len * len)).clamp(0.0, 1.0) };
            if (a + d * t).distance(pr.pos) < pr.radius * 0.8 { return false; }
        }
        true
    }

    pub fn band_at(&self, p: Vec2) -> u8 { self.at(p).band() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_deterministic_and_has_every_band() {
        let a = TileMap::generate(WorldKind::Realm, 4242);
        let b = TileMap::generate(WorldKind::Realm, 4242);
        assert_eq!(a.tiles, b.tiles);
        let mut seen = [0usize; 12];
        for &t in &a.tiles { seen[t as usize] += 1; }
        for t in [Tile::Water, Tile::Sand, Tile::Grass, Tile::Forest, Tile::Highland, Tile::Ash] {
            assert!(seen[t as usize] > 50, "{t:?} only {} tiles", seen[t as usize]);
        }
    }

    #[test]
    fn beach_spawn_is_walkable_sand() {
        for seed in [1u32, 77, 4242, 99991] {
            let m = TileMap::generate(WorldKind::Realm, seed);
            assert_eq!(m.at(m.spawn), Tile::Sand, "seed {seed}");
            assert!(m.walkable(m.spawn, 0.38));
        }
    }

    #[test]
    fn nexus_has_walls_and_open_plaza() {
        let m = TileMap::generate(WorldKind::Concourse, 0);
        assert!(!m.walkable(Vec2::new(0.5, 10.0), 0.3));
        assert!(m.walkable(m.spawn, 0.38));
        assert!(m.walkable(NEXUS_REALM_PORTAL, 0.38));
        assert!(!m.walkable(NEXUS_FOUNTAIN, 0.3));
    }
}
