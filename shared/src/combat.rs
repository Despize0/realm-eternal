//! Souls-style combat simulation for Realm Eternal, ported from the Ashen Vigil
//! prototype (Godot/GDScript) into plain Rust so it runs in WASM and in native tests.
//!
//! Units are metres and seconds. The client multiplies positions by [`PX_PER_M`]
//! before projecting them. The simulation always advances in fixed [`TICK`] steps.

use crate::world::{TileMap, WorldKind, NEXUS_REALM_PORTAL};
use crate::Vec2;
use std::collections::BTreeMap;

pub const PX_PER_M: f32 = 32.0;
pub const TICK: f32 = 1.0 / 60.0;
pub const INPUT_BUFFER: f32 = 0.12;
pub const LOW_CLEARANCE: f32 = 0.9;

// ---------------------------------------------------------------- data ----

#[derive(Debug)]
pub struct ProjSpec {
    pub name: &'static str,
    pub speed: f32,
    pub radius: f32,
    pub range: f32,
    pub damage: f32,
    pub poise: f32,
    pub knockback: f32,
    /// 0 = cinder bolt (crimson), 1 = knell (violet), 2 = grave wave (orange)
    pub look: u8,
}

#[derive(Debug)]
pub struct Attack {
    pub id: &'static str,
    pub name: &'static str,
    pub windup: f32,
    pub active: f32,
    pub recovery: f32,
    pub stamina: f32,
    pub damage: f32,
    pub poise: f32,
    pub knockback: f32,
    pub lunge: f32,
    /// Hitbox: width across, depth along facing, centre `fwd` metres ahead,
    /// spanning heights `bot..top` above the floor.
    pub w: f32,
    pub d: f32,
    pub fwd: f32,
    pub bot: f32,
    pub top: f32,
    pub tell: &'static str,
    pub heavy: bool,
    pub proj: Option<&'static ProjSpec>,
    pub count: u8,
    pub spread_deg: f32,
    pub ring: bool,
    pub muzzle_fwd: f32,
    pub muzzle_h: f32,
}

impl Attack {
    pub fn total(&self) -> f32 { self.windup + self.active + self.recovery }
    pub fn is_melee(&self) -> bool { self.w > 0.0 && self.d > 0.0 }
    pub fn reach(&self) -> f32 { self.fwd + self.d * 0.5 }
    pub fn is_low(&self) -> bool { self.top <= LOW_CLEARANCE + 0.001 }
}

const fn melee(
    id: &'static str, name: &'static str, windup: f32, active: f32, recovery: f32,
    stamina: f32, damage: f32, poise: f32, knockback: f32, lunge: f32,
    w: f32, d: f32, fwd: f32, bot: f32, top: f32, tell: &'static str, heavy: bool,
) -> Attack {
    Attack { id, name, windup, active, recovery, stamina, damage, poise, knockback, lunge,
        w, d, fwd, bot, top, tell, heavy, proj: None, count: 0, spread_deg: 0.0, ring: false,
        muzzle_fwd: 0.0, muzzle_h: 0.0 }
}

pub static CINDER_BOLT_P: ProjSpec = ProjSpec { name: "Cinder Bolt", speed: 9.0, radius: 0.22, range: 14.0, damage: 14.0, poise: 12.0, knockback: 2.0, look: 0 };
pub static KNELL_P: ProjSpec = ProjSpec { name: "Knell", speed: 7.5, radius: 0.28, range: 12.0, damage: 18.0, poise: 22.0, knockback: 3.0, look: 1 };
pub static WAVE_P: ProjSpec = ProjSpec { name: "Grave Wave", speed: 6.0, radius: 0.35, range: 7.5, damage: 20.0, poise: 25.0, knockback: 4.0, look: 2 };

// Knight (player) — Vigil Longsword
pub static VIGIL_CUT: Attack = melee("knight_vigil_cut", "Vigil Cut", 0.2, 0.12, 0.32, 20.0, 18.0, 18.0, 2.5, 2.0, 1.6, 1.5, 0.95, 0.4, 1.6, "", false);
pub static ASHFALL_CLEAVE: Attack = melee("knight_ashfall_cleave", "Ashfall Cleave", 0.45, 0.16, 0.52, 35.0, 34.0, 40.0, 5.0, 3.0, 1.1, 2.0, 1.15, 0.2, 1.8, "", true);
// Ashen Husk
pub static RAKE: Attack = melee("husk_rake", "Rake", 0.55, 0.14, 0.7, 0.0, 11.0, 12.0, 2.0, 2.0, 1.4, 1.4, 0.9, 0.4, 1.6, "RAKE", false);
pub static HACK: Attack = melee("husk_hack", "Hack", 0.75, 0.16, 0.95, 0.0, 18.0, 18.0, 3.0, 2.5, 1.0, 1.5, 0.95, 0.3, 1.7, "HACK", true);
// Ashen Duelist
pub static CUTTING_DRAW: Attack = melee("duelist_cutting_draw", "Cutting Draw", 0.4, 0.14, 0.5, 0.0, 16.0, 22.0, 3.0, 2.5, 1.5, 1.5, 0.95, 0.4, 1.6, "DRAW", false);
pub static LUNGING_THRUST: Attack = melee("duelist_lunging_thrust", "Lunging Thrust", 0.65, 0.18, 0.75, 0.0, 28.0, 30.0, 4.0, 6.0, 0.7, 1.9, 1.15, 0.5, 1.5, "THRUST", true);
// Pyreguard
pub static HEW: Attack = melee("pyre_hew", "Hew", 0.65, 0.16, 0.8, 0.0, 22.0, 30.0, 4.0, 2.5, 1.9, 1.6, 1.0, 0.4, 1.6, "HEW", false);
pub static PYRE_CLEAVE: Attack = melee("pyre_cleave", "Pyre Cleave", 0.95, 0.18, 1.15, 0.0, 36.0, 45.0, 6.0, 3.0, 1.2, 1.9, 1.15, 0.25, 1.75, "CLEAVE", true);
// Cinder Cultist
pub static CINDER_BOLT: Attack = Attack { proj: Some(&CINDER_BOLT_P), count: 1, spread_deg: 0.0, muzzle_fwd: 0.6, muzzle_h: 1.35, ring: false,
    ..melee("cultist_cinder_bolt", "Cinder Bolt", 0.75, 0.1, 0.9, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, "!", false) };
pub static ASH_VOLLEY: Attack = Attack { proj: Some(&CINDER_BOLT_P), count: 3, spread_deg: 26.0, muzzle_fwd: 0.6, muzzle_h: 1.35, ring: false,
    ..melee("cultist_ash_volley", "Ash Volley", 1.0, 0.1, 1.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, "!!", false) };
// The Bellwarden
pub static TOLLING_SWEEP: Attack = melee("bellwarden_tolling_sweep", "Tolling Sweep", 0.85, 0.18, 1.0, 0.0, 30.0, 40.0, 6.0, 1.5, 4.6, 3.0, 1.7, 0.0, 0.9, "LOW", true);
pub static TOLLING_SWEEP_LEAD: Attack = melee("bellwarden_tolling_sweep_lead", "Tolling Sweep", 0.85, 0.18, 0.35, 0.0, 30.0, 40.0, 6.0, 1.5, 4.6, 3.0, 1.7, 0.0, 0.9, "LOW", true);
pub static RETURN_SWEEP: Attack = melee("bellwarden_return_sweep", "Return Sweep", 0.6, 0.18, 1.1, 0.0, 28.0, 40.0, 6.0, 1.5, 4.6, 3.0, 1.7, 0.0, 0.9, "LOW", true);
pub static BELL_SLAM: Attack = melee("bellwarden_bell_slam", "Bell Slam", 1.05, 0.14, 1.4, 0.0, 38.0, 60.0, 7.0, 2.5, 2.2, 2.6, 2.0, 0.0, 2.8, "SLAM", true);
pub static GRAVE_SLAM: Attack = Attack { proj: Some(&WAVE_P), count: 14, ring: true, muzzle_fwd: 2.0, muzzle_h: 0.35, spread_deg: 0.0,
    ..melee("bellwarden_grave_slam", "Grave Slam", 1.05, 0.14, 1.6, 0.0, 38.0, 60.0, 7.0, 2.5, 2.2, 2.6, 2.0, 0.0, 2.8, "SLAM + RING", true) };
pub static KNELL_FAN: Attack = Attack { proj: Some(&KNELL_P), count: 5, spread_deg: 64.0, muzzle_fwd: 1.1, muzzle_h: 1.6, ring: false,
    ..melee("bellwarden_knell_fan", "Knell Fan", 0.95, 0.12, 0.9, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, "FAN", true) };
pub static GRAVE_TOLL: Attack = Attack { proj: Some(&WAVE_P), count: 18, ring: true, muzzle_fwd: 0.0, muzzle_h: 0.35, spread_deg: 0.0,
    ..melee("bellwarden_grave_toll", "Grave Toll", 1.4, 0.1, 1.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, "RING", true) };

pub static PHASE1_COMBOS: [&[&Attack]; 3] = [&[&TOLLING_SWEEP], &[&BELL_SLAM], &[&KNELL_FAN]];
pub static PHASE2_COMBOS: [&[&Attack]; 4] = [
    &[&TOLLING_SWEEP_LEAD, &RETURN_SWEEP], // Double Toll
    &[&TOLLING_SWEEP_LEAD, &GRAVE_SLAM],   // Sweep into Grave Slam
    &[&KNELL_FAN, &GRAVE_SLAM],            // Knell then Grave Slam
    &[&GRAVE_SLAM],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind { Knight, Husk, Duelist, Cultist, Pyreguard, Bellwarden }

#[derive(Clone, Copy, Debug)]
pub struct Stats { pub hp: f32, pub poise: f32, pub speed: f32, pub hurt_r: f32, pub height: f32,
    pub poise_regen: f32, pub poise_delay: f32, pub stagger: f32 }

impl Kind {
    pub fn stats(self) -> Stats {
        let base = Stats { hp: 100.0, poise: 20.0, speed: 4.2, hurt_r: 0.38, height: 1.8, poise_regen: 20.0, poise_delay: 1.2, stagger: 0.5 };
        match self {
            Kind::Knight => base,
            Kind::Husk => Stats { hp: 48.0, poise: 14.0, speed: 3.0, ..base },
            Kind::Duelist => Stats { hp: 140.0, poise: 45.0, speed: 3.6, ..base },
            Kind::Cultist => Stats { hp: 70.0, poise: 15.0, speed: 2.6, ..base },
            Kind::Pyreguard => Stats { hp: 190.0, poise: 70.0, speed: 2.6, hurt_r: 0.48, height: 2.0, ..base },
            Kind::Bellwarden => Stats { hp: 420.0, poise: 90.0, speed: 1.95, hurt_r: 0.85, height: 3.1, poise_regen: 12.0, poise_delay: 2.5, stagger: 1.2 },
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Kind::Knight => "Knight", Kind::Husk => "Ashen Husk", Kind::Duelist => "Ashen Duelist",
            Kind::Cultist => "Cinder Cultist", Kind::Pyreguard => "Pyreguard", Kind::Bellwarden => "The Bellwarden",
        }
    }
}

// -------------------------------------------------------------- world ----

pub const NAVE_MIN: Vec2 = Vec2::new(-11.0, -8.0);
pub const NAVE_MAX: Vec2 = Vec2::new(11.0, 8.0);
pub const GATE_HALF_W: f32 = 1.6;
pub const CORRIDOR_END_Y: f32 = -11.5;
pub const BELFRY_CENTER: Vec2 = Vec2::new(0.0, -19.0);
pub const BELFRY_R: f32 = 8.0;
pub const NAVE_ALTAR: Vec2 = Vec2::new(0.0, 6.0);
pub const BELFRY_ALTAR: Vec2 = Vec2::new(0.0, -12.6);
pub const ALTAR_REACH: f32 = 1.6;
pub const PILLARS: [Vec2; 6] = [
    Vec2::new(-6.5, -4.0), Vec2::new(-6.5, 0.5), Vec2::new(-6.5, 5.0),
    Vec2::new(6.5, -4.0), Vec2::new(6.5, 0.5), Vec2::new(6.5, 5.0),
];
pub const PILLAR_R: f32 = 0.42;
pub const BELFRY_PILLARS: [Vec2; 4] = [
    Vec2::new(-5.0, -22.0), Vec2::new(5.0, -22.0), Vec2::new(-5.0, -16.0), Vec2::new(5.0, -16.0),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area { Nave, Belfry }

pub fn all_pillars() -> impl Iterator<Item = Vec2> { PILLARS.iter().chain(BELFRY_PILLARS.iter()).copied() }

// --------------------------------------------------------------- types ----

pub static ALL_ATTACKS: [&Attack; 17] = [
    &VIGIL_CUT, &ASHFALL_CLEAVE, &RAKE, &HACK, &CUTTING_DRAW, &LUNGING_THRUST, &HEW, &PYRE_CLEAVE,
    &CINDER_BOLT, &ASH_VOLLEY, &TOLLING_SWEEP, &TOLLING_SWEEP_LEAD, &RETURN_SWEEP, &BELL_SLAM, &GRAVE_SLAM,
    &KNELL_FAN, &GRAVE_TOLL,
];
pub static ALL_PROJECTILES: [&ProjSpec; 3] = [&CINDER_BOLT_P, &KNELL_P, &WAVE_P];

pub fn attack_index(a: &Attack) -> u8 { ALL_ATTACKS.iter().position(|x| std::ptr::eq(*x, a)).unwrap_or(0) as u8 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State { Idle, Attack, Dodge, Stagger, Heal, Dead }

impl State {
    pub fn to_u8(self) -> u8 { self as u8 }
    pub fn from_u8(v: u8) -> State {
        [State::Idle, State::Attack, State::Dodge, State::Stagger, State::Heal, State::Dead][(v as usize).min(5)]
    }
}

impl Kind {
    pub fn to_u8(self) -> u8 { self as u8 }
    pub fn from_u8(v: u8) -> Kind {
        [Kind::Knight, Kind::Husk, Kind::Duelist, Kind::Cultist, Kind::Pyreguard, Kind::Bellwarden][(v as usize).min(5)]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase { Windup, Active, Recovery }

pub fn phase_of(atk: &Attack, t: f32) -> Phase {
    if t < atk.windup { Phase::Windup } else if t < atk.windup + atk.active { Phase::Active } else { Phase::Recovery }
}

#[derive(Clone, Debug)]
pub struct AttackRun {
    pub atk: &'static Attack,
    pub hit: Vec<u32>,
    pub released: bool,
    /// Where a projectile attack is aimed (the target's position when it began).
    pub aim: Vec2,
}

#[derive(Clone, Debug)]
pub struct Actor {
    pub uid: u32,
    pub kind: Kind,
    pub player: bool,
    pub pos: Vec2,
    pub vel: Vec2,
    pub knock: Vec2,
    pub z: f32,
    pub vz: f32,
    pub facing: Vec2,
    pub hp: f32,
    pub chip: f32,
    pub chip_hold: f32,
    pub stamina: f32,
    pub stamina_wait: f32,
    pub poise: f32,
    pub poise_wait: f32,
    pub state: State,
    pub t: f32,
    pub run: Option<AttackRun>,
    pub dodge_dir: Vec2,
    pub backstep: bool,
    pub flasks: u8,
    pub healed: bool,
    pub flash: f32,
    pub freeze: f32,
    pub rise: f32,
    pub drop: f32,
    pub cooldown: f32,
    pub strafe: f32,
    pub dmg_mult: f32,
    pub target: Option<u32>,
    pub home: Vec2,
    pub band: u8,
    pub stats: Stats,
}

impl Actor {
    pub fn new(uid: u32, kind: Kind, pos: Vec2, facing: Vec2) -> Self {
        let s = kind.stats();
        Actor { uid, kind, player: kind == Kind::Knight, pos, vel: Vec2::ZERO, knock: Vec2::ZERO, z: 0.0, vz: 0.0,
            facing: facing.normalized(), hp: s.hp, chip: s.hp, chip_hold: 0.0, stamina: 100.0, stamina_wait: 0.0,
            poise: s.poise, poise_wait: 0.0, state: State::Idle, t: 0.0, run: None, dodge_dir: Vec2::ZERO,
            backstep: false, flasks: 3, healed: false, flash: 0.0, freeze: 0.0, rise: 0.0, drop: 0.0, cooldown: 0.6,
            strafe: if uid % 2 == 0 { 1.0 } else { -1.0 }, dmg_mult: 1.0, target: None, home: pos, band: 0, stats: s }
    }
    pub fn alive(&self) -> bool { self.state != State::Dead }
    pub fn grounded(&self) -> bool { self.z <= 0.0001 && self.vz <= 0.0 }
    pub fn active(&self) -> bool { self.alive() && self.rise <= 0.0 && self.drop <= 0.0 }
    pub fn phase(&self) -> Option<Phase> { self.run.as_ref().map(|r| phase_of(r.atk, self.t)) }
    pub fn invulnerable(&self) -> bool {
        // Dodge i-frames quantised to 60 Hz ticks: 0.067 .. 0.233 s.
        self.state == State::Dodge && self.t >= 0.06 && self.t < 0.24
    }
    fn regen_state(&self) -> bool { self.state == State::Idle }
}

#[derive(Clone, Debug)]
pub struct Projectile {
    pub id: u32,
    pub spec: &'static ProjSpec,
    pub pos: Vec2,
    pub z: f32,
    pub dir: Vec2,
    pub left: f32,
    pub owner: u32,
    pub from_player: bool,
    pub dmg_mult: f32,
    pub passed: Vec<u32>,
    pub alive: bool,
}

#[derive(Clone, Debug)]
pub enum Event {
    Damage { pos: Vec2, z: f32, amount: i32, to_player: bool },
    Hurt { uid: u32 },
    Message { to: Option<u32>, text: String, secs: f32 },
    Shake { to: Option<u32>, amount: f32 },
    Stagger { pos: Vec2 },
    LevelUp { uid: u32, level: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage { Wave(u8), GateOpen, BossArrival, Boss, Victory }

impl Stage {
    pub fn label(self) -> String {
        match self {
            Stage::Wave(n) => format!("wave:{n}"),
            Stage::GateOpen => "gate".into(),
            Stage::BossArrival => "arrival".into(),
            Stage::Boss => "boss".into(),
            Stage::Victory => "victory".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Input {
    pub mv: Vec2,
    pub aim: Vec2,
    pub light: bool,
    pub heavy: bool,
    pub dodge: bool,
    pub jump: bool,
    pub jump_held: bool,
    pub flask: bool,
    /// E: rest at an altar or step through a portal.
    pub interact: bool,
    /// R: return to the Nexus.
    pub nexus: bool,
}

impl Input {
    /// Merge a newer input into a pending one, keeping button presses until a tick consumes them.
    pub fn merge(&mut self, newer: &Input) {
        self.mv = newer.mv; self.aim = newer.aim; self.jump_held = newer.jump_held;
        self.light |= newer.light; self.heavy |= newer.heavy; self.dodge |= newer.dodge; self.jump |= newer.jump;
        self.flask |= newer.flask; self.interact |= newer.interact; self.nexus |= newer.nexus;
    }
    fn clear_presses(&mut self) {
        self.light = false; self.heavy = false; self.dodge = false; self.jump = false;
        self.flask = false; self.interact = false; self.nexus = false;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action { Light, Heavy, Dodge, Jump, Flask, Interact }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortalTo { Realm, Nexus, Cathedral }

#[derive(Clone, Debug)]
pub struct Portal {
    pub id: u32,
    pub pos: Vec2,
    pub to: PortalTo,
    /// Seconds left; negative means permanent.
    pub ttl: f32,
    /// Server-assigned instance a cathedral portal leads to.
    pub instance: Option<u64>,
}

pub const PORTAL_REACH: f32 = 1.4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transfer { Nexus, Realm, Portal(u32), /** The character fell: permadeath. */ Died }

#[derive(Clone, Debug)]
pub struct PlayerInfo {
    pub name: String,
    pub level: u32,
    pub xp: u32,
    pub input: Input,
    buffer: Option<(Action, f32)>,
    pub dead_t: f32,
    pub turn_gap: f32,
}

pub const MAX_LEVEL: u32 = 20;
pub fn xp_to_next(level: u32) -> u32 { if level >= MAX_LEVEL { 0 } else { (50.0 * (level as f32).powf(1.55)) as u32 + 50 } }
pub fn level_hp(level: u32) -> f32 { 100.0 + 8.0 * (level.saturating_sub(1)) as f32 }
pub fn level_dmg(level: u32) -> f32 { 1.0 + 0.05 * (level.saturating_sub(1)) as f32 }

#[derive(Clone, Debug)]
pub struct BossBrain {
    bag: Vec<usize>,
    combo: Option<(usize, usize)>,
    pub phase2: bool,
    toll_pending: bool,
    gap: f32,
}

impl BossBrain { fn new() -> Self { BossBrain { bag: Vec::new(), combo: None, phase2: false, toll_pending: false, gap: 1.0 } } }

#[derive(Clone, Debug)]
pub enum Geo { Map(TileMap), Cathedral }

pub struct Instance {
    pub id: u64,
    pub kind: WorldKind,
    pub seed: u32,
    pub geo: Geo,
    pub actors: Vec<Actor>,
    pub projectiles: Vec<Projectile>,
    pub events: Vec<Event>,
    pub portals: Vec<Portal>,
    pub players: BTreeMap<u32, PlayerInfo>,
    pub transfers: Vec<(u32, Transfer)>,
    pub stage: Stage,
    pub stage_t: f32,
    pub gate_open: bool,
    pub mist: bool,
    pub boss_defeated: bool,
    pub time: f32,
    pub boss: BossBrain,
    next_uid: u32,
    rng: u32,
    wave_clear_t: f32,
    spawn_t: f32,
    pub empty_for: f32,
}

pub static WAVES: [&[Kind]; 3] = [
    &[Kind::Husk, Kind::Husk, Kind::Cultist],
    &[Kind::Husk, Kind::Duelist, Kind::Cultist, Kind::Husk],
    &[Kind::Duelist, Kind::Pyreguard, Kind::Cultist, Kind::Husk, Kind::Husk],
];

/// Which foes roam each difficulty band of the realm, with relative weights.
pub fn band_roster(band: u8) -> &'static [(Kind, u32)] {
    match band {
        0 => &[(Kind::Husk, 1)],
        1 => &[(Kind::Husk, 3), (Kind::Cultist, 1)],
        2 => &[(Kind::Husk, 2), (Kind::Duelist, 2), (Kind::Cultist, 2)],
        3 => &[(Kind::Duelist, 3), (Kind::Pyreguard, 2), (Kind::Cultist, 2)],
        _ => &[(Kind::Pyreguard, 3), (Kind::Duelist, 2), (Kind::Cultist, 2)],
    }
}
pub fn band_hp_mult(band: u8) -> f32 { [0.55, 0.8, 1.0, 1.25, 1.6][band.min(4) as usize] }
pub fn band_dmg_mult(band: u8) -> f32 { [0.55, 0.75, 1.0, 1.2, 1.45][band.min(4) as usize] }
fn base_xp(kind: Kind) -> f32 {
    match kind { Kind::Husk => 8.0, Kind::Cultist => 12.0, Kind::Duelist => 26.0, Kind::Pyreguard => 40.0, Kind::Bellwarden => 900.0, Kind::Knight => 0.0 }
}

// ---------------------------------------------------------- helpers ----

fn rot(v: Vec2, a: f32) -> Vec2 { let (s, c) = a.sin_cos(); Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c) }
fn dot(a: Vec2, b: Vec2) -> f32 { a.x * b.x + a.y * b.y }
fn perp(v: Vec2) -> Vec2 { Vec2::new(-v.y, v.x) }

fn turn_toward(facing: Vec2, target: Vec2, max: f32) -> Vec2 {
    let t = target.normalized();
    if t.length() < 0.5 { return facing; }
    let cross = facing.x * t.y - facing.y * t.x;
    let ang = cross.atan2(dot(facing, t));
    rot(facing, ang.clamp(-max, max)).normalized()
}

pub fn area_of(p: Vec2) -> Area { if p.y < CORRIDOR_END_Y { Area::Belfry } else { Area::Nave } }

/// Segment-circle intersection: does the segment a→b pass within r of c?
fn seg_hits_circle(a: Vec2, b: Vec2, c: Vec2, r: f32) -> Option<f32> {
    let d = b - a;
    let len2 = dot(d, d);
    let t = if len2 < 1e-9 { 0.0 } else { (dot(c - a, d) / len2).clamp(0.0, 1.0) };
    let p = a + d * t;
    if p.distance(c) <= r { Some(t) } else { None }
}

fn cathedral_line_clear(a: Vec2, b: Vec2) -> bool { all_pillars().all(|p| seg_hits_circle(a, b, p, PILLAR_R).is_none()) }

/// Cathedral walkability; the gate and the mist change it.
pub fn cathedral_walkable(p: Vec2, r: f32, player: bool, gate_open: bool, mist: bool) -> bool {
    let in_nave = p.x >= NAVE_MIN.x + r && p.x <= NAVE_MAX.x - r && p.y >= NAVE_MIN.y + r && p.y <= NAVE_MAX.y - r;
    let in_corr = gate_open && p.x.abs() <= GATE_HALF_W - r && p.y <= NAVE_MIN.y + 0.5 && p.y >= CORRIDOR_END_Y - 0.5;
    let in_belfry = p.distance(BELFRY_CENTER) <= BELFRY_R - r;
    if player && mist {
        // The mist wall seals the belfry while the Bellwarden lives.
        return in_belfry && p.y < CORRIDOR_END_Y + 0.6 - r;
    }
    in_nave || in_corr || in_belfry
}

impl Geo {
    pub fn obstacles_near(&self, p: Vec2, rad: f32) -> Vec<(Vec2, f32)> {
        match self {
            Geo::Cathedral => all_pillars().filter(|q| q.distance(p) < rad + PILLAR_R).map(|q| (q, PILLAR_R)).collect(),
            Geo::Map(m) => m.props_near(p, rad).map(|pr| (pr.pos, pr.radius)).collect(),
        }
    }
}

impl Instance {
    fn base(id: u64, kind: WorldKind, seed: u32, geo: Geo) -> Instance {
        Instance {
            id, kind, seed, geo, actors: Vec::new(), projectiles: Vec::new(), events: Vec::new(), portals: Vec::new(),
            players: BTreeMap::new(), transfers: Vec::new(), stage: Stage::Victory, stage_t: 0.0, gate_open: false,
            mist: false, boss_defeated: false, time: 0.0, boss: BossBrain::new(), next_uid: 1_000_000,
            rng: 0x9E37_79B9 ^ (id as u32).wrapping_mul(2654435761), wave_clear_t: 0.0, spawn_t: 0.0, empty_for: 0.0,
        }
    }

    pub fn new(id: u64, kind: WorldKind, seed: u32) -> Instance {
        match kind {
            WorldKind::Concourse => {
                let mut i = Self::base(id, kind, seed, Geo::Map(TileMap::generate(kind, seed)));
                i.portals.push(Portal { id: 1, pos: NEXUS_REALM_PORTAL, to: PortalTo::Realm, ttl: -1.0, instance: None });
                i
            }
            WorldKind::Realm => Self::base(id, kind, seed, Geo::Map(TileMap::generate(kind, seed))),
            WorldKind::Cathedral => {
                let mut i = Self::base(id, kind, seed, Geo::Cathedral);
                i.start_wave(0);
                i
            }
        }
    }

    pub fn map(&self) -> Option<&TileMap> { if let Geo::Map(m) = &self.geo { Some(m) } else { None } }

    pub fn walkable(&self, p: Vec2, r: f32, player: bool) -> bool {
        match &self.geo {
            Geo::Cathedral => cathedral_walkable(p, r, player, self.gate_open, self.mist),
            Geo::Map(m) => m.walkable(p, r),
        }
    }

    pub fn line_clear(&self, a: Vec2, b: Vec2) -> bool {
        match &self.geo { Geo::Cathedral => cathedral_line_clear(a, b), Geo::Map(m) => m.line_clear(a, b) }
    }

    fn rand(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13; x ^= x >> 17; x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    fn uid(&mut self) -> u32 { self.next_uid += 1; self.next_uid }

    fn idx(&self, uid: u32) -> Option<usize> { self.actors.iter().position(|a| a.uid == uid) }
    pub fn actor(&self, uid: u32) -> Option<&Actor> { self.actors.iter().find(|a| a.uid == uid) }
    pub fn boss_actor(&self) -> Option<&Actor> { self.actors.iter().find(|a| a.kind == Kind::Bellwarden) }
    pub fn enemies_alive(&self) -> usize { self.actors.iter().filter(|a| !a.player && a.alive()).count() }
    pub fn player_count(&self) -> usize { self.players.len() }

    fn say(&mut self, to: Option<u32>, msg: &str, secs: f32) {
        self.events.push(Event::Message { to, text: msg.to_string(), secs });
    }

    // ------------------------------------------------------ players ----

    pub fn spawn_point(&mut self) -> Vec2 {
        let (r1, r2) = (self.rand(), self.rand());
        match &self.geo {
            Geo::Map(m) if self.kind == WorldKind::Realm => m.find_beach_spawn((r1 * 1000.0) as u32),
            Geo::Map(m) => m.spawn + Vec2::new((r1 - 0.5) * 3.0, (r2 - 0.5) * 1.5),
            Geo::Cathedral => {
                if matches!(self.stage, Stage::BossArrival | Stage::Boss) { BELFRY_ALTAR + Vec2::new(0.0, -1.2) }
                else { NAVE_ALTAR + Vec2::new((r1 - 0.5) * 2.0, -1.2) }
            }
        }
    }

    pub fn add_player(&mut self, uid: u32, name: &str, level: u32, xp: u32) {
        let pos = self.spawn_point();
        let mut a = Actor::new(uid, Kind::Knight, pos, Vec2::new(0.0, -1.0));
        Self::apply_level(&mut a, level);
        a.hp = a.stats.hp; a.chip = a.hp;
        self.actors.push(a);
        self.players.insert(uid, PlayerInfo { name: name.to_string(), level, xp, input: Input::default(), buffer: None, dead_t: 0.0, turn_gap: 0.0 });
        let welcome = match self.kind {
            WorldKind::Concourse => "THE NEXUS".to_string(),
            WorldKind::Realm => "THE ASHEN REALM".to_string(),
            WorldKind::Cathedral => "THE CATHEDRAL OF THE VIGIL".to_string(),
        };
        self.say(Some(uid), &welcome, 2.2);
    }

    fn apply_level(a: &mut Actor, level: u32) {
        a.stats.hp = level_hp(level);
        a.dmg_mult = level_dmg(level);
    }

    pub fn remove_player(&mut self, uid: u32) -> Option<PlayerInfo> {
        if let Some(i) = self.idx(uid) { self.actors.remove(i); }
        for a in self.actors.iter_mut() { if a.target == Some(uid) { a.target = None; } }
        self.players.remove(&uid)
    }

    pub fn set_input(&mut self, uid: u32, input: &Input) {
        if let Some(p) = self.players.get_mut(&uid) { p.input.merge(input); }
    }

    // ------------------------------------------------------ cathedral ----

    fn start_wave(&mut self, n: u8) {
        self.actors.retain(|a| a.player);
        self.projectiles.retain(|p| p.from_player);
        self.stage = Stage::Wave(n);
        self.stage_t = 0.0;
        let spots = [Vec2::new(-4.0, -5.0), Vec2::new(4.0, -5.0), Vec2::new(0.0, -6.0), Vec2::new(-8.5, -2.0), Vec2::new(8.5, -2.0)];
        let scale = 1.0 + 0.35 * (self.players.len().max(1) - 1) as f32;
        for (i, &k) in WAVES[n as usize].iter().enumerate() {
            let mut p = spots[i % spots.len()];
            if k == Kind::Cultist { p = Vec2::new(if i % 2 == 0 { -9.0 } else { 9.0 }, -6.5); }
            let uid = self.uid();
            let mut a = Actor::new(uid, k, p, Vec2::new(0.0, 1.0));
            a.stats.hp *= scale; a.hp = a.stats.hp; a.chip = a.hp;
            a.cooldown = 0.8 + i as f32 * 0.35;
            a.band = 2;
            if n > 0 { a.rise = 1.2; }
            self.actors.push(a);
        }
        self.say(None, &format!("WAVE {} OF 3", n + 1), 1.8);
    }

    fn spawn_boss(&mut self) {
        self.actors.retain(|a| a.player);
        self.projectiles.clear();
        let uid = self.uid();
        let mut b = Actor::new(uid, Kind::Bellwarden, BELFRY_CENTER + Vec2::new(0.0, -2.5), Vec2::new(0.0, 1.0));
        let scale = 1.0 + 0.5 * (self.players.len().max(1) - 1) as f32;
        b.stats.hp *= scale; b.hp = b.stats.hp; b.chip = b.hp;
        b.drop = 2.2;
        b.band = 2;
        self.actors.push(b);
        self.boss = BossBrain::new();
        self.stage = Stage::BossArrival;
        self.stage_t = 0.0;
    }

    fn revive(&mut self, uid: u32, at: Vec2) {
        let level = self.players.get(&uid).map_or(1, |p| p.level);
        if let Some(i) = self.idx(uid) {
            let mut a = Actor::new(uid, Kind::Knight, at, Vec2::new(0.0, -1.0));
            Self::apply_level(&mut a, level);
            a.hp = a.stats.hp; a.chip = a.hp;
            self.actors[i] = a;
        }
        if let Some(p) = self.players.get_mut(&uid) { p.dead_t = 0.0; }
    }

    // ------------------------------------------------------------ step ----

    pub fn step(&mut self) {
        self.time += TICK;
        self.stage_t += TICK;
        if self.players.is_empty() { self.empty_for += TICK; } else { self.empty_for = 0.0; }

        let uids: Vec<u32> = self.players.keys().copied().collect();
        for uid in &uids { self.update_player(*uid); }
        self.update_enemies();
        for i in 0..self.actors.len() { self.tick_actor(i); }
        self.resolve_bodies();
        self.release_projectiles();
        self.fly_projectiles();
        self.melee_hits();
        self.cleanup();
        self.update_portals();
        match self.kind {
            WorldKind::Cathedral => self.update_cathedral(),
            WorldKind::Realm => { self.update_spawns(); self.update_deaths(); }
            WorldKind::Concourse => self.update_deaths(),
        }
        for p in self.players.values_mut() { p.input.clear_presses(); p.turn_gap -= TICK; }
    }

    fn spend(a: &mut Actor, cost: f32) {
        a.stamina = (a.stamina - cost).max(0.0);
        a.stamina_wait = 0.7;
    }

    fn begin_attack(a: &mut Actor, atk: &'static Attack, aim: Vec2) {
        a.state = State::Attack;
        a.t = 0.0;
        a.run = Some(AttackRun { atk, hit: Vec::new(), released: false, aim });
        a.vel = Vec2::ZERO;
    }

    fn near_portal(&self, p: Vec2) -> Option<u32> {
        self.portals.iter().find(|q| q.pos.distance(p) < PORTAL_REACH).map(|q| q.id)
    }

    fn update_player(&mut self, uid: u32) {
        let Some(i) = self.idx(uid) else { return };
        let (input, mut buffer) = { let p = &self.players[&uid]; (p.input, p.buffer) };
        let now = self.time;
        let action = if input.dodge { Some(Action::Dodge) } else if input.light { Some(Action::Light) }
            else if input.heavy { Some(Action::Heavy) } else if input.jump { Some(Action::Jump) }
            else if input.flask { Some(Action::Flask) } else if input.interact { Some(Action::Interact) } else { None };
        if let Some(a) = action { buffer = Some((a, now)); }
        if let Some((_, t)) = buffer { if now - t > INPUT_BUFFER { buffer = None; } }

        if input.nexus && self.kind != WorldKind::Concourse && self.actors[i].alive() {
            self.transfers.push((uid, Transfer::Nexus));
        }
        let safe = self.kind == WorldKind::Concourse;
        let portal = self.near_portal(self.actors[i].pos);
        let rest_ok = self.rest_status(self.actors[i].pos);
        let p = &mut self.actors[i];
        if !p.alive() { self.players.get_mut(&uid).unwrap().buffer = None; return; }
        let aim_dir = (input.aim - p.pos).normalized();
        let mv = input.mv.normalized();
        let free = p.state == State::Idle;
        if free && aim_dir.length() > 0.5 { p.facing = aim_dir; }

        let mut rest = false;
        let mut denied = false;
        if let Some((action, _)) = buffer {
            let grounded = p.grounded();
            let mut consumed = false;
            match action {
                Action::Light | Action::Heavy if free && grounded && p.stamina > 0.0 && !safe => {
                    let atk: &'static Attack = if action == Action::Light { &VIGIL_CUT } else { &ASHFALL_CLEAVE };
                    Self::spend(p, atk.stamina);
                    if aim_dir.length() > 0.5 { p.facing = aim_dir; }
                    Self::begin_attack(p, atk, input.aim);
                    consumed = true;
                }
                Action::Dodge if free && grounded && p.stamina > 0.0 => {
                    Self::spend(p, 25.0);
                    p.state = State::Dodge;
                    p.t = 0.0;
                    p.backstep = mv.length() < 0.5;
                    p.dodge_dir = if p.backstep { p.facing * -1.0 } else { mv };
                    if !p.backstep { p.facing = mv; }
                    consumed = true;
                }
                Action::Jump if free && grounded => { p.vz = 8.0; consumed = true; }
                Action::Flask if free && grounded => {
                    if p.flasks > 0 { p.state = State::Heal; p.t = 0.0; p.healed = false; p.flasks -= 1; }
                    consumed = true;
                }
                Action::Interact if free && grounded => {
                    consumed = true;
                    if let Some(pid) = portal { self.transfers.push((uid, Transfer::Portal(pid))); }
                    else { match rest_ok { Some(true) => rest = true, Some(false) => denied = true, None => {} } }
                }
                _ => {}
            }
            if consumed { buffer = None; }
        }
        let p = &mut self.actors[i];
        if !input.jump_held && p.vz > 3.0 { p.vz = 3.0; }
        let speed = p.stats.speed * match p.state { State::Heal => 0.35, _ => 1.0 };
        if p.state == State::Idle || p.state == State::Heal {
            let want = mv * speed;
            let accel = if p.grounded() { if want.length() > 0.01 { 32.0 } else { 40.0 } } else { 5.0 };
            let diff = want - p.vel;
            let step = accel * TICK;
            p.vel = if diff.length() <= step { want } else { p.vel + diff.normalized() * step };
        }
        if rest {
            p.hp = p.stats.hp; p.chip = p.hp; p.flasks = 3; p.stamina = 100.0;
            self.say(Some(uid), "YOU REST AT THE VIGIL ALTAR", 2.0);
        }
        if denied { self.say(Some(uid), "You cannot rest while the Bellwarden lives", 2.0); }
        self.players.get_mut(&uid).unwrap().buffer = buffer;
    }

    fn rest_status(&self, p: Vec2) -> Option<bool> {
        if self.kind != WorldKind::Cathedral { return None; }
        if p.distance(BELFRY_ALTAR) < ALTAR_REACH { Some(self.boss_defeated) }
        else if p.distance(NAVE_ALTAR) < ALTAR_REACH { Some(self.enemies_alive() == 0 || self.boss_defeated) }
        else { None }
    }

    fn update_enemies(&mut self) {
        let players: Vec<(u32, Vec2, bool)> = self.actors.iter().filter(|a| a.player).map(|a| (a.uid, a.pos, a.alive())).collect();
        let aggro = if self.kind == WorldKind::Cathedral { 60.0 } else { 11.0 };
        // Attack turns per target: at most two melee fighters and one caster on one player at once.
        let mut busy: BTreeMap<u32, (u32, u32)> = BTreeMap::new();
        for a in self.actors.iter().filter(|a| !a.player && a.state == State::Attack && a.kind != Kind::Bellwarden) {
            if let Some(t) = a.target {
                let e = busy.entry(t).or_insert((0, 0));
                if a.kind == Kind::Cultist { e.1 += 1 } else { e.0 += 1 }
            }
        }
        let boss_proj_alive = self.projectiles.iter().any(|p| !p.from_player && p.spec.look != 0);
        let n = self.actors.len();
        for i in 0..n {
            if self.actors[i].player || !self.actors[i].active() { continue; }
            // Pick a target: keep the current one while in leash range, else the nearest in aggro range.
            let me = self.actors[i].pos;
            let cur = self.actors[i].target.and_then(|t| players.iter().find(|p| p.0 == t && p.2 && p.1.distance(me) < aggro * 1.6));
            let tgt = cur.copied().or_else(|| players.iter().filter(|p| p.2 && p.1.distance(me) < aggro)
                .min_by(|a, b| a.1.distance(me).total_cmp(&b.1.distance(me))).copied());
            self.actors[i].target = tgt.map(|t| t.0);

            let mut push = Vec2::ZERO;
            for j in 0..n {
                if i == j || self.actors[j].player || !self.actors[j].alive() { continue; }
                let d = self.actors[i].pos - self.actors[j].pos;
                if d.length() > 3.0 { continue; }
                let min = self.actors[i].stats.hurt_r + self.actors[j].stats.hurt_r + 0.6;
                let l = d.length();
                if l < min && l > 0.001 { push += d.normalized() * (min - l); }
            }
            let r1 = self.rand();
            let r2 = self.rand();
            let line = tgt.map_or(false, |t| self.line_clear(me, t.1));
            let gap_ok = tgt.map_or(false, |t| self.players.get(&t.0).map_or(true, |p| p.turn_gap <= 0.0));
            let time = self.time;
            let a = &mut self.actors[i];
            a.cooldown -= TICK;

            if a.state == State::Attack {
                let turn = if a.kind == Kind::Bellwarden { 2.0 } else { 3.5 };
                if let (Some(Phase::Windup), Some(t)) = (a.phase(), tgt) {
                    if a.kind != Kind::Cultist { a.facing = turn_toward(a.facing, t.1 - a.pos, turn * TICK); }
                }
                continue;
            }
            if a.state != State::Idle { continue; }
            let Some((tuid, ppos, _)) = tgt else {
                // No one near: drift around home.
                let wander = Vec2::new((time * 0.4 + a.uid as f32).cos(), (time * 0.33 + a.uid as f32 * 1.7).sin());
                let home_pull = (a.home - a.pos) * 0.15;
                let want = (wander * 0.35 + home_pull).normalized() * a.stats.speed * 0.3;
                a.vel = a.vel.lerp(want + push, 1.0 - (-4.0 * TICK).exp());
                if want.length() > 0.05 { a.facing = turn_toward(a.facing, want, 3.0 * TICK); }
                continue;
            };
            let to = ppos - a.pos;
            let dist = to.length();
            let dir = to.normalized();
            a.facing = turn_toward(a.facing, to, 6.0 * TICK);
            let speed = a.stats.speed;
            let (mb, cb) = busy.get(&tuid).copied().unwrap_or((0, 0));

            if a.kind == Kind::Bellwarden {
                let brain = &mut self.boss;
                brain.gap -= TICK;
                if !brain.phase2 && a.hp <= a.stats.hp * 0.5 {
                    brain.phase2 = true; brain.toll_pending = true; brain.bag.clear(); brain.combo = None;
                }
                let combos: &[&[&Attack]] = if brain.phase2 { &PHASE2_COMBOS } else { &PHASE1_COMBOS };
                if !brain.toll_pending && brain.combo.is_none() && brain.gap <= 0.0 {
                    if brain.bag.is_empty() {
                        let len = combos.len();
                        let mut v: Vec<usize> = (0..len).collect();
                        let mut seed = (time * 1000.0) as u32 | 1;
                        for k in (1..len).rev() { seed ^= seed << 13; seed ^= seed >> 17; seed ^= seed << 5; v.swap(k, seed as usize % (k + 1)); }
                        brain.bag = v;
                    }
                    let c = brain.bag.pop().unwrap();
                    brain.combo = Some((c, 0));
                }
                let next: Option<&'static Attack> = if brain.toll_pending { Some(&GRAVE_TOLL) }
                    else if let Some((c, s)) = brain.combo { Some(combos[c][s]) } else { None };
                let mut walk = true;
                if let Some(atk) = next {
                    let mid_combo = matches!(brain.combo, Some((_, s)) if s > 0);
                    let in_range = if atk.is_melee() { dist <= atk.reach() + 0.4 || mid_combo } else { dist <= 10.0 };
                    let proj_ok = atk.proj.is_none() || !boss_proj_alive || atk.is_melee();
                    if in_range && proj_ok {
                        Self::begin_attack(a, atk, ppos);
                        a.facing = turn_toward(a.facing, to, 0.6);
                        if brain.toll_pending { brain.toll_pending = false; }
                        else if let Some((c, s)) = brain.combo {
                            brain.combo = if s + 1 < combos[c].len() { Some((c, s + 1)) } else { None };
                            if brain.combo.is_none() { brain.gap = 0.5 + r1 * 0.6; }
                        }
                        walk = false;
                    }
                }
                if walk {
                    let want = if dist > 2.4 { dir * speed } else if dist < 1.6 { dir * -speed * 0.6 } else { Vec2::ZERO };
                    a.vel = a.vel.lerp(want, 1.0 - (-6.0 * TICK).exp());
                }
                continue;
            }

            if a.kind == Kind::Cultist {
                let want = if dist < 2.6 { dir * -speed } else if dist > 8.5 { dir * speed } else { perp(dir) * a.strafe * speed * 0.35 };
                if a.cooldown <= 0.0 && dist >= 2.6 && dist <= 13.0 && cb == 0 && gap_ok && line {
                    let atk: &'static Attack = if r1 < 0.62 { &CINDER_BOLT } else { &ASH_VOLLEY };
                    Self::begin_attack(a, atk, ppos);
                    a.facing = dir;
                    busy.entry(tuid).or_insert((0, 0)).1 += 1;
                    if let Some(p) = self.players.get_mut(&tuid) { p.turn_gap = 0.45; }
                    continue;
                }
                a.vel = a.vel.lerp(want + push * 2.0, 1.0 - (-8.0 * TICK).exp());
                continue;
            }

            let (light, heavy): (&'static Attack, &'static Attack) = match a.kind {
                Kind::Husk => (&RAKE, &HACK),
                Kind::Duelist => (&CUTTING_DRAW, &LUNGING_THRUST),
                _ => (&HEW, &PYRE_CLEAVE),
            };
            let pick: &'static Attack = if a.kind == Kind::Duelist { if dist > 2.0 { heavy } else { light } }
                else if r2 < 0.35 { heavy } else { light };
            let reach = pick.reach() + if pick.lunge > 4.0 { 1.0 } else { 0.2 };
            let can_turn = mb < 2 && gap_ok;
            if a.cooldown <= 0.0 && dist <= reach && can_turn && dot(a.facing, dir) > 0.8 && line {
                Self::begin_attack(a, pick, ppos);
                busy.entry(tuid).or_insert((0, 0)).0 += 1;
                if let Some(p) = self.players.get_mut(&tuid) { p.turn_gap = 0.45; }
                continue;
            }
            let hold = light.reach() + 0.3;
            let mut want = if dist > hold + 0.2 { dir * speed } else if dist < hold - 0.6 { dir * -speed * 0.5 } else { perp(dir) * a.strafe * speed * 0.4 };
            if !can_turn && dist < 3.5 { want = perp(dir) * a.strafe * speed * 0.5 + dir * -0.6; }
            if r1 < 0.004 { a.strafe = -a.strafe; }
            a.vel = a.vel.lerp(want + push * 2.0, 1.0 - (-8.0 * TICK).exp());
        }
    }

    fn tick_actor(&mut self, i: usize) {
        let rnd = self.rand();
        let mut shake = false;
        {
            let a = &mut self.actors[i];
            a.flash = (a.flash - TICK).max(0.0);
            if a.chip > a.hp { a.chip_hold -= TICK; if a.chip_hold <= 0.0 { a.chip = (a.chip - 60.0 * TICK).max(a.hp); } } else { a.chip = a.hp; }
            if a.rise > 0.0 { a.rise -= TICK; return; }
            if a.drop > 0.0 { a.drop -= TICK; if a.drop <= 0.0 { shake = true; } }
        }
        if shake { self.events.push(Event::Shake { to: None, amount: 1.0 }); }
        let a = &mut self.actors[i];
        if a.drop > 0.0 { return; }
        // Hit stop: a short freeze on the two fighters involved, not the whole world.
        if a.freeze > 0.0 { a.freeze -= TICK; return; }
        a.t += TICK;

        if a.z > 0.0 || a.vz > 0.0 {
            a.z += a.vz * TICK;
            a.vz -= 24.0 * TICK;
            if a.z <= 0.0 { a.z = 0.0; a.vz = 0.0; }
        }

        match a.state {
            State::Attack => {
                let atk = a.run.as_ref().unwrap().atk;
                if a.phase() == Some(Phase::Active) && atk.lunge > 0.0 { a.vel = a.facing * atk.lunge; } else { a.vel = a.vel * 0.7; }
                if a.t >= atk.total() {
                    a.state = State::Idle; a.t = 0.0; a.run = None;
                    a.cooldown = if a.kind == Kind::Cultist { 1.2 + rnd * 0.8 } else { 0.9 + rnd * 1.0 };
                }
            }
            State::Dodge => {
                let dur = 0.40;
                let k = 1.0 - (a.t / dur).min(1.0);
                let sp = 8.5 * (0.35 + 0.65 * k) * if a.backstep { 0.6 } else { 1.0 };
                a.vel = a.dodge_dir * sp;
                if a.t >= dur { a.state = State::Idle; a.t = 0.0; a.vel = a.vel * 0.3; }
            }
            State::Stagger => {
                a.vel = a.vel * 0.85;
                if a.t >= a.stats.stagger { a.state = State::Idle; a.t = 0.0; a.cooldown = a.cooldown.max(0.4); }
            }
            State::Heal => {
                if !a.healed && a.t >= 0.6 { a.healed = true; a.hp = (a.hp + 45.0).min(a.stats.hp); a.chip = a.hp; }
                if a.t >= 1.0 { a.state = State::Idle; a.t = 0.0; }
            }
            State::Dead => { a.vel = Vec2::ZERO; }
            State::Idle => {}
        }

        if a.stamina_wait > 0.0 { a.stamina_wait -= TICK; }
        else if a.regen_state() { a.stamina = (a.stamina + 25.0 * TICK).min(100.0); }
        if a.poise_wait > 0.0 { a.poise_wait -= TICK; }
        else { a.poise = (a.poise + a.stats.poise_regen * TICK).min(a.stats.poise); }
        a.knock = a.knock * (-9.0 * TICK).exp();

        let a = &self.actors[i];
        let delta = (a.vel + a.knock) * TICK;
        if delta.length() < 1e-6 { return; }
        let (pos, r, player) = (a.pos, a.stats.hurt_r.min(0.6), a.player);
        let mut np = pos + delta;
        if !self.walkable(np, r, player) {
            let nx = Vec2::new(pos.x + delta.x, pos.y);
            let ny = Vec2::new(pos.x, pos.y + delta.y);
            np = if self.walkable(nx, r, player) { nx } else if self.walkable(ny, r, player) { ny } else { pos };
        }
        if let Geo::Cathedral = self.geo {
            for (pl, pr) in self.geo.obstacles_near(np, r + 0.6) {
                let d = np - pl;
                let min = pr + r;
                if d.length() < min && d.length() > 1e-5 { np = pl + d.normalized() * min; }
            }
            if !self.walkable(np, r, player) { np = pos; }
        }
        self.actors[i].pos = np;
    }

    fn resolve_bodies(&mut self) {
        let n = self.actors.len();
        for pi in 0..n {
            if !self.actors[pi].player || !self.actors[pi].alive() { continue; }
            let p = self.actors[pi].clone();
            for i in 0..n {
                if i == pi || self.actors[i].player { continue; }
                let e = &self.actors[i];
                if !e.active() { continue; }
                let d = p.pos - e.pos;
                let min = p.stats.hurt_r + e.stats.hurt_r * 0.8;
                let l = d.length();
                if l < min && l > 0.0001 && p.z < 0.9 {
                    let np = self.actors[pi].pos + d.normalized() * (min - l);
                    if self.walkable(np, p.stats.hurt_r, true) { self.actors[pi].pos = np; }
                }
            }
        }
    }

    fn release_projectiles(&mut self) {
        let mut spawned = Vec::new();
        let mut next = self.next_uid;
        for a in self.actors.iter_mut() {
            if a.state != State::Attack || a.phase() != Some(Phase::Active) { continue; }
            let run = a.run.as_mut().unwrap();
            if run.released { continue; }
            let atk = run.atk;
            let Some(spec) = atk.proj else { continue };
            run.released = true;
            let origin = a.pos + a.facing * atk.muzzle_fwd;
            let dirs: Vec<Vec2> = if atk.ring {
                (0..atk.count).map(|k| { let ang = k as f32 / atk.count as f32 * std::f32::consts::TAU; Vec2::new(ang.cos(), ang.sin()) }).collect()
            } else {
                // Aim where the target stood when the cast began; never homes.
                let base = if a.kind == Kind::Cultist { (run.aim - origin).normalized() } else { a.facing };
                let base = if base.length() < 0.5 { a.facing } else { base };
                (0..atk.count).map(|k| {
                    let off = if atk.count > 1 { (k as f32 / (atk.count - 1) as f32 - 0.5) * atk.spread_deg.to_radians() } else { 0.0 };
                    rot(base, off)
                }).collect()
            };
            for dir in dirs {
                next += 1;
                spawned.push(Projectile { id: next, spec, pos: origin, z: atk.muzzle_h, dir, left: spec.range, owner: a.uid,
                    from_player: a.player, dmg_mult: a.dmg_mult, passed: Vec::new(), alive: true });
            }
        }
        self.next_uid = next;
        // A bolt that would start inside a player fizzles instead.
        let players: Vec<(Vec2, f32)> = self.actors.iter().filter(|a| a.player && a.alive()).map(|a| (a.pos, a.stats.hurt_r)).collect();
        spawned.retain(|pr| pr.from_player || pr.spec.look == 2 || players.iter().all(|(p, r)| pr.pos.distance(*p) > pr.spec.radius + r));
        self.projectiles.extend(spawned);
    }

    fn fly_projectiles(&mut self) {
        for pi in 0..self.projectiles.len() {
            if !self.projectiles[pi].alive { continue; }
            let (a, b, spec, z, from_player) = {
                let pr = &self.projectiles[pi];
                (pr.pos, pr.pos + pr.dir * (pr.spec.speed * TICK), pr.spec, pr.z, pr.from_player)
            };
            let mut best: Option<(f32, usize)> = None;
            for (ti, t) in self.actors.iter().enumerate() {
                if !t.active() || t.player == from_player { continue; }
                if t.pos.distance(a) > 4.0 { continue; }
                if self.projectiles[pi].passed.contains(&t.uid) { continue; }
                if z + spec.radius < t.z || z - spec.radius > t.z + t.stats.height { continue; }
                if let Some(s) = seg_hits_circle(a, b, t.pos, spec.radius + t.stats.hurt_r) {
                    if best.map_or(true, |(bs, _)| s < bs) { best = Some((s, ti)); }
                }
            }
            let mut wall_t: Option<f32> = None;
            for (pl, pr) in self.geo.obstacles_near((a + b) * 0.5, 1.0) {
                if let Some(s) = seg_hits_circle(a, b, pl, pr + spec.radius * 0.5) { wall_t = Some(wall_t.map_or(s, |w: f32| w.min(s))); }
            }
            let blocked = match &self.geo { Geo::Map(m) => m.at(b).solid(), Geo::Cathedral => !self.walkable(b, 0.0, false) };
            if blocked { wall_t = Some(wall_t.map_or(1.0, |w| w.min(1.0))); }
            if let Some((s, ti)) = best {
                if wall_t.map_or(true, |w| s <= w) {
                    if self.actors[ti].invulnerable() {
                        let uid = self.actors[ti].uid;
                        self.projectiles[pi].passed.push(uid);
                    } else {
                        let (owner, dir, mult) = { let p = &self.projectiles[pi]; (p.owner, p.dir, p.dmg_mult) };
                        self.projectiles[pi].alive = false;
                        self.apply_hit(ti, owner, spec.damage * mult, spec.poise, spec.knockback, dir, false);
                        continue;
                    }
                }
            }
            if wall_t.is_some() { self.projectiles[pi].alive = false; continue; }
            let pr = &mut self.projectiles[pi];
            pr.pos = b;
            pr.left -= spec.speed * TICK;
            if pr.left <= 0.0 { pr.alive = false; }
        }
        self.projectiles.retain(|p| p.alive);
    }

    pub fn hitbox_contains(attacker: &Actor, atk: &Attack, target: &Actor) -> bool {
        let f = attacker.facing;
        let side = perp(f);
        let center = attacker.pos + f * atk.fwd;
        let rel = target.pos - center;
        let lx = dot(rel, side).clamp(-atk.w * 0.5, atk.w * 0.5);
        let ly = dot(rel, f).clamp(-atk.d * 0.5, atk.d * 0.5);
        let closest = center + side * lx + f * ly;
        if closest.distance(target.pos) > target.stats.hurt_r { return false; }
        !(target.z >= atk.top || target.z + target.stats.height <= atk.bot)
    }

    fn melee_hits(&mut self) {
        let n = self.actors.len();
        for i in 0..n {
            let a = &self.actors[i];
            if a.state != State::Attack || a.phase() != Some(Phase::Active) { continue; }
            let atk = a.run.as_ref().unwrap().atk;
            if !atk.is_melee() { continue; }
            for j in 0..n {
                if i == j { continue; }
                let (a, t) = (&self.actors[i], &self.actors[j]);
                if a.player == t.player || !t.active() { continue; }
                if a.pos.distance(t.pos) > atk.reach() + t.stats.hurt_r + 1.0 { continue; }
                if a.run.as_ref().unwrap().hit.contains(&t.uid) { continue; }
                if !Self::hitbox_contains(a, atk, t) { continue; }
                // Cover blocks melee: chest to the target's centre or either flank.
                let side = perp((t.pos - a.pos).normalized()) * (t.stats.hurt_r * 0.9);
                let (ap, tp) = (a.pos, t.pos);
                if !(self.line_clear(ap, tp) || self.line_clear(ap, tp + side) || self.line_clear(ap, tp - side)) { continue; }
                let t = &self.actors[j];
                if t.invulnerable() { continue; }
                let a = &self.actors[i];
                let (tuid, auid, dir, mult) = (t.uid, a.uid, (t.pos - a.pos).normalized(), a.dmg_mult);
                self.actors[i].run.as_mut().unwrap().hit.push(tuid);
                self.actors[i].freeze = if atk.heavy { 0.09 } else { 0.06 };
                self.apply_hit(j, auid, atk.damage * mult, atk.poise, atk.knockback, dir, atk.heavy);
            }
        }
    }

    fn apply_hit(&mut self, ti: usize, from: u32, damage: f32, poise: f32, knockback: f32, dir: Vec2, heavy: bool) {
        // The Concourse is a server-enforced safe zone: nothing there can be hurt.
        if self.kind == WorldKind::Concourse { return; }
        let t = &mut self.actors[ti];
        if !t.alive() { return; }
        t.hp -= damage;
        t.chip_hold = 0.5;
        t.flash = 0.12;
        t.freeze = if heavy { 0.09 } else { 0.06 };
        t.poise -= poise;
        t.poise_wait = t.stats.poise_delay;
        t.knock = dir * knockback * 0.8;
        if !t.player && t.target.is_none() { t.target = Some(from); }
        let (to_player, tuid, pos, height, kind, band) = (t.player, t.uid, t.pos, t.stats.height, t.kind, t.band);
        self.events.push(Event::Damage { pos, z: height + 0.3, amount: damage.round() as i32, to_player });
        if to_player {
            self.events.push(Event::Hurt { uid: tuid });
            self.events.push(Event::Shake { to: Some(tuid), amount: if heavy { 0.8 } else { 0.4 } });
        }
        let t = &mut self.actors[ti];
        if t.hp <= 0.0 {
            t.hp = 0.0; t.state = State::Dead; t.t = 0.0; t.run = None;
            if kind == Kind::Bellwarden { self.events.push(Event::Shake { to: None, amount: 1.4 }); }
            if !to_player { self.on_enemy_killed(kind, band, pos); }
            else { self.say(Some(tuid), "YOU HAVE FALLEN", 3.0); }
            return;
        }
        if t.poise <= 0.0 {
            // A stagger interrupts the attack (and ends a boss combo).
            t.poise = t.stats.poise;
            t.state = State::Stagger;
            t.t = 0.0;
            t.run = None;
            if kind == Kind::Bellwarden { self.boss.combo = None; self.boss.gap = 0.8; }
            self.events.push(Event::Stagger { pos });
        }
    }

    fn on_enemy_killed(&mut self, kind: Kind, band: u8, pos: Vec2) {
        // Experience is shared with every player nearby, as in ROTMG.
        let xp = (base_xp(kind) * (1.0 + band as f32 * 0.8)) as u32;
        let near: Vec<u32> = self.actors.iter().filter(|a| a.player && a.alive() && a.pos.distance(pos) < 16.0).map(|a| a.uid).collect();
        for uid in near { self.grant_xp(uid, xp); }
        // Realm foes sometimes leave a way into the cathedral.
        if self.kind == WorldKind::Realm {
            let chance = [0.0, 0.015, 0.03, 0.05, 0.08][band.min(4) as usize] * if kind == Kind::Pyreguard { 2.0 } else { 1.0 };
            let open = self.portals.iter().filter(|p| p.to == PortalTo::Cathedral && p.pos.distance(pos) < 30.0).count();
            if open == 0 && self.rand() < chance {
                let id = self.uid();
                self.portals.push(Portal { id, pos, to: PortalTo::Cathedral, ttl: 45.0, instance: None });
                self.say(None, "A Cathedral of the Vigil portal has opened!", 3.0);
            }
        }
    }

    pub fn grant_xp(&mut self, uid: u32, xp: u32) {
        let Some(p) = self.players.get_mut(&uid) else { return };
        if p.level >= MAX_LEVEL { return; }
        p.xp += xp;
        let mut leveled = false;
        while p.level < MAX_LEVEL && p.xp >= xp_to_next(p.level) { p.xp -= xp_to_next(p.level); p.level += 1; leveled = true; }
        if p.level >= MAX_LEVEL { p.xp = 0; }
        let level = p.level;
        if leveled {
            if let Some(i) = self.idx(uid) {
                let a = &mut self.actors[i];
                Self::apply_level(a, level);
                a.hp = a.stats.hp; a.chip = a.hp;
            }
            self.events.push(Event::LevelUp { uid, level });
            self.say(Some(uid), &format!("LEVEL {level}"), 2.0);
        }
    }

    fn cleanup(&mut self) {
        self.actors.retain(|a| a.player || !(a.state == State::Dead && a.kind != Kind::Bellwarden && a.t > 1.6));
    }

    fn update_portals(&mut self) {
        for p in self.portals.iter_mut() { if p.ttl > 0.0 { p.ttl -= TICK; if p.ttl <= 0.0 { p.ttl = 0.0; } } }
        self.portals.retain(|p| p.ttl != 0.0);
    }

    fn update_deaths(&mut self) {
        // Permadeath: a fallen character is gone; the player wakes as a new recruit in the Concourse.
        let dead: Vec<u32> = self.actors.iter().filter(|a| a.player && !a.alive()).map(|a| a.uid).collect();
        for uid in dead {
            let p = self.players.get_mut(&uid).unwrap();
            if p.dead_t < 0.0 { continue; }
            p.dead_t += TICK;
            if p.dead_t >= 3.0 {
                p.dead_t = -100.0;
                if self.kind == WorldKind::Concourse { let at = self.spawn_point(); self.revive(uid, at); }
                else { self.transfers.push((uid, Transfer::Died)); }
            }
        }
    }

    fn update_spawns(&mut self) {
        self.spawn_t -= TICK;
        if self.spawn_t > 0.0 { return; }
        self.spawn_t = 0.5;
        if !matches!(self.geo, Geo::Map(_)) { return; }
        let players: Vec<Vec2> = self.actors.iter().filter(|a| a.player && a.alive()).map(|a| a.pos).collect();
        let rolls: Vec<(f32, f32)> = players.iter().map(|_| (self.rand(), self.rand())).collect();
        // Despawn foes far from everyone.
        self.actors.retain(|a| a.player || a.state == State::Attack || players.iter().any(|p| p.distance(a.pos) < 42.0));
        let total = self.actors.iter().filter(|a| !a.player).count();
        if total > 180 { return; }
        let mut plans: Vec<(Vec2, u8)> = Vec::new();
        let Geo::Map(map) = &self.geo else { return };
        for (k, &pp) in players.iter().enumerate() {
            let band = map.band_at(pp);
            let near = self.actors.iter().filter(|a| !a.player && a.pos.distance(pp) < 24.0).count();
            let target = 4 + band as usize * 2;
            if near >= target { continue; }
            let ang = rolls[k].0 * std::f32::consts::TAU;
            let dist = 14.0 + rolls[k].1 * 7.0;
            let spot = pp + Vec2::new(ang.cos(), ang.sin()) * dist;
            if !map.walkable(spot, 0.7) { continue; }
            if players.iter().any(|p| p.distance(spot) < 12.0) { continue; }
            plans.push((spot, map.band_at(spot)));
        }
        for (spot, band) in plans {
            let roster = band_roster(band);
            let total_w: u32 = roster.iter().map(|r| r.1).sum();
            let mut pick = (self.rand() * total_w as f32) as u32;
            let mut kind = roster[0].0;
            for &(k, w) in roster { if pick < w { kind = k; break; } pick -= w; }
            let group = match kind { Kind::Husk => 2 + (self.rand() * 2.0) as usize, Kind::Pyreguard => 1, _ => 1 + (self.rand() * 2.0) as usize };
            for g in 0..group {
                let p = spot + Vec2::new((g % 2) as f32 * 1.4 - 0.7, (g / 2) as f32 * 1.4);
                if !self.walkable(p, 0.6, false) { continue; }
                let uid = self.uid();
                let mut a = Actor::new(uid, kind, p, Vec2::new(0.0, 1.0));
                a.band = band;
                a.stats.hp *= band_hp_mult(band); a.hp = a.stats.hp; a.chip = a.hp;
                a.dmg_mult = band_dmg_mult(band);
                a.rise = 0.8;
                a.cooldown = 1.0 + self.rand();
                self.actors.push(a);
            }
        }
    }

    fn update_cathedral(&mut self) {
        self.update_deaths();
        // With nobody left standing, the encounter resets for whoever enters next.
        if self.actors.iter().filter(|a| a.player).all(|a| !a.alive()) && !self.players.is_empty() { return; }
        if self.players.is_empty() && self.empty_for > 1.0 && !self.boss_defeated {
            match self.stage { Stage::BossArrival | Stage::Boss => self.spawn_boss(), Stage::Wave(n) if self.enemies_alive() < WAVES[n as usize].len() => self.start_wave(n), _ => {} }
            self.empty_for = -1000.0;
            return;
        }
        match self.stage {
            Stage::Wave(n) => {
                if self.enemies_alive() == 0 {
                    self.wave_clear_t += TICK;
                    if (self.wave_clear_t - TICK).abs() < 1e-6 {
                        for a in self.actors.iter_mut().filter(|a| a.player) { a.flasks = (a.flasks + 1).min(3); }
                    }
                    if self.wave_clear_t >= 1.6 {
                        self.wave_clear_t = 0.0;
                        if (n as usize) + 1 < WAVES.len() { self.start_wave(n + 1); }
                        else { self.stage = Stage::GateOpen; self.gate_open = true; self.say(None, "THE BELFRY GATE OPENS", 3.0); }
                    }
                }
            }
            Stage::GateOpen => {
                if !self.boss_defeated && self.actors.iter().any(|a| a.player && a.alive() && a.pos.y < CORRIDOR_END_Y - 0.8) {
                    self.mist = true;
                    // Everyone is drawn through the mist with the one who crossed.
                    for a in self.actors.iter_mut().filter(|a| a.player) {
                        if a.pos.y >= CORRIDOR_END_Y - 0.8 { a.pos = Vec2::new(a.pos.x.clamp(-1.0, 1.0), CORRIDOR_END_Y - 1.4); a.vel = Vec2::ZERO; }
                    }
                    self.spawn_boss();
                }
            }
            Stage::BossArrival => { if self.stage_t >= 2.2 { self.stage = Stage::Boss; self.stage_t = 0.0; } }
            Stage::Boss => {
                if self.boss_actor().map_or(false, |b| !b.alive()) {
                    self.boss_defeated = true;
                    self.mist = false;
                    self.stage = Stage::Victory;
                    self.projectiles.clear();
                    let id = self.uid();
                    self.portals.push(Portal { id, pos: BELFRY_CENTER, to: PortalTo::Nexus, ttl: -1.0, instance: None });
                    self.say(None, "THE BELL FALLS SILENT", 5.0);
                }
            }
            Stage::Victory => {}
        }
    }
}

/// Fairness rules from the original BellwardenAI.validate_combos().
pub fn validate_combos() -> Result<(), String> {
    for combo in PHASE1_COMBOS.iter().chain(PHASE2_COMBOS.iter()) {
        let last = combo.last().ok_or("empty combo")?;
        if last.recovery < 0.9 { return Err(format!("{} ends on {:.2}s recovery", last.name, last.recovery)); }
        for a in combo.iter() {
            if a.windup < 0.6 { return Err(format!("{} winds up only {:.2}s", a.name, a.windup)); }
            if a.tell.is_empty() { return Err(format!("{} has no tell", a.name)); }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cathedral_with_player() -> (Instance, u32) {
        let mut s = Instance::new(1, WorldKind::Cathedral, 1);
        s.add_player(7, "Tester", 1, 0);
        (s, 7)
    }
    fn press(s: &mut Instance, uid: u32, f: impl Fn(&mut Input)) {
        let mut inp = Input { aim: s.actor(uid).unwrap().pos + Vec2::new(0.0, -5.0), ..Default::default() };
        f(&mut inp);
        s.set_input(uid, &inp);
    }
    fn idle(s: &mut Instance, uid: u32, ticks: usize) { for _ in 0..ticks { press(s, uid, |_| {}); s.step(); } }

    #[test]
    fn combos_are_fair() { validate_combos().unwrap(); }

    #[test]
    fn dodge_iframes_window() {
        let mut a = Actor::new(1, Kind::Knight, Vec2::ZERO, Vec2::new(0.0, -1.0));
        a.state = State::Dodge;
        a.t = 0.05; assert!(!a.invulnerable());
        a.t = 0.1; assert!(a.invulnerable());
        a.t = 0.25; assert!(!a.invulnerable());
    }

    #[test]
    fn jumping_clears_low_sweep_but_not_slam() {
        let boss = Actor::new(1, Kind::Bellwarden, Vec2::ZERO, Vec2::new(0.0, 1.0));
        let mut k = Actor::new(2, Kind::Knight, Vec2::new(0.0, 1.8), Vec2::new(0.0, -1.0));
        assert!(Instance::hitbox_contains(&boss, &TOLLING_SWEEP, &k));
        k.z = 1.0;
        assert!(!Instance::hitbox_contains(&boss, &TOLLING_SWEEP, &k));
        assert!(Instance::hitbox_contains(&boss, &BELL_SLAM, &k));
        assert!(8.0f32 * 8.0 / (2.0 * 24.0) > LOW_CLEARANCE);
    }

    #[test]
    fn light_attack_spends_stamina_and_hits() {
        let (mut s, uid) = cathedral_with_player();
        s.actors.retain(|a| a.player);
        let ppos = s.actor(uid).unwrap().pos;
        let mut h = Actor::new(99, Kind::Husk, ppos + Vec2::new(0.0, -1.2), Vec2::new(0.0, 1.0));
        h.cooldown = 99.0;
        s.actors.push(h);
        press(&mut s, uid, |i| i.light = true);
        s.step();
        assert_eq!(s.actor(uid).unwrap().state, State::Attack);
        assert!(s.actor(uid).unwrap().stamina <= 80.01);
        idle(&mut s, uid, 40);
        assert!(s.actor(99).unwrap().hp < 48.0);
    }

    #[test]
    fn flask_heals_45() {
        let (mut s, uid) = cathedral_with_player();
        s.actors.retain(|a| a.player);
        s.stage = Stage::GateOpen;
        let i = s.idx(uid).unwrap();
        s.actors[i].hp = 30.0;
        press(&mut s, uid, |i| i.flask = true);
        s.step();
        assert_eq!(s.actor(uid).unwrap().flasks, 2);
        idle(&mut s, uid, 45);
        assert!((s.actor(uid).unwrap().hp - 75.0).abs() < 0.01);
    }

    #[test]
    fn cathedral_runs_through_to_the_boss_with_two_players() {
        let (mut s, uid) = cathedral_with_player();
        s.add_player(8, "Second", 1, 0);
        for _ in 0..3 {
            for a in s.actors.iter_mut().filter(|a| !a.player) { a.hp = 0.0; a.state = State::Dead; }
            idle(&mut s, uid, 200);
        }
        assert_eq!(s.stage, Stage::GateOpen);
        let i = s.idx(uid).unwrap();
        s.actors[i].pos = Vec2::new(0.0, -13.0);
        idle(&mut s, uid, 1);
        assert!(s.mist);
        // The second knight is drawn into the belfry too.
        assert!(s.actor(8).unwrap().pos.y < CORRIDOR_END_Y);
        for k in 0..6000 {
            for a in s.actors.iter_mut().filter(|a| a.player) { a.hp = 10_000.0; }
            if k == 600 { let b = s.actors.iter_mut().find(|a| a.kind == Kind::Bellwarden).unwrap(); b.hp = b.stats.hp * 0.45; }
            idle(&mut s, uid, 1);
        }
        assert!(s.boss.phase2);
    }

    #[test]
    fn death_is_permanent_outside_the_concourse() {
        let (mut s, uid) = cathedral_with_player();
        s.add_player(8, "Second", 1, 0);
        let i = s.idx(uid).unwrap();
        s.actors[i].hp = 0.0; s.actors[i].state = State::Dead;
        let before = s.enemies_alive();
        idle(&mut s, uid, 200);
        assert_eq!(s.transfers.iter().filter(|t| **t == (uid, Transfer::Died)).count(), 1);
        assert_eq!(s.enemies_alive(), before, "the survivor keeps fighting the same wave");
    }

    #[test]
    fn realm_spawns_foes_near_players_and_drops_xp() {
        let mut s = Instance::new(2, WorldKind::Realm, 4242);
        s.add_player(5, "Roamer", 1, 0);
        for _ in 0..300 { s.step(); }
        assert!(s.enemies_alive() > 0, "no realm spawns");
        let foe = s.actors.iter().position(|a| !a.player).unwrap();
        let pos = s.actor(5).unwrap().pos;
        s.actors[foe].pos = pos + Vec2::new(0.0, -1.0);
        s.actors[foe].rise = 0.0;
        let foe_uid = s.actors[foe].uid;
        let fi = s.idx(foe_uid).unwrap();
        s.apply_hit(fi, 5, 9999.0, 0.0, 0.0, Vec2::new(0.0, -1.0), false);
        let p = &s.players[&5];
        assert!(p.xp > 0 || p.level > 1);
    }

    #[test]
    fn concourse_is_safe_and_portal_transfers() {
        let mut s = Instance::new(3, WorldKind::Concourse, 0);
        s.add_player(5, "Hub", 1, 0);
        let i = s.idx(5).unwrap();
        s.actors[i].pos = NEXUS_REALM_PORTAL + Vec2::new(0.0, 0.6);
        press(&mut s, 5, |i| i.light = true);
        s.step();
        assert_eq!(s.actor(5).unwrap().state, State::Idle, "no attacks in the Nexus");
        press(&mut s, 5, |i| i.interact = true);
        s.step();
        assert_eq!(s.transfers.first(), Some(&(5, Transfer::Portal(1))));
    }

    #[test]
    fn xp_curve_reaches_max_level() {
        let mut s = Instance::new(4, WorldKind::Concourse, 0);
        s.add_player(5, "Grinder", 1, 0);
        for _ in 0..400 { s.grant_xp(5, 500); }
        assert_eq!(s.players[&5].level, MAX_LEVEL);
        assert!((s.actor(5).unwrap().stats.hp - level_hp(MAX_LEVEL)).abs() < 0.01);
    }
}
