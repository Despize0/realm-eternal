//! Souls-style combat simulation for Realm Eternal, ported from the Ashen Vigil
//! prototype (Godot/GDScript) into plain Rust so it runs in WASM and in native tests.
//!
//! Units are metres and seconds. The client multiplies positions by [`PX_PER_M`]
//! before projecting them. The simulation always advances in fixed [`TICK`] steps.

use crate::Vec2;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State { Idle, Attack, Dodge, Stagger, Heal, Dead }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase { Windup, Active, Recovery }

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
    pub rise: f32,
    pub drop: f32,
    pub cooldown: f32,
    pub strafe: f32,
    pub stats: Stats,
}

impl Actor {
    pub fn new(uid: u32, kind: Kind, pos: Vec2, facing: Vec2) -> Self {
        let s = kind.stats();
        Actor { uid, kind, player: kind == Kind::Knight, pos, vel: Vec2::ZERO, knock: Vec2::ZERO, z: 0.0, vz: 0.0,
            facing: facing.normalized(), hp: s.hp, chip: s.hp, chip_hold: 0.0, stamina: 100.0, stamina_wait: 0.0,
            poise: s.poise, poise_wait: 0.0, state: State::Idle, t: 0.0, run: None, dodge_dir: Vec2::ZERO,
            backstep: false, flasks: 3, healed: false, flash: 0.0, rise: 0.0, drop: 0.0, cooldown: 0.6,
            strafe: if uid % 2 == 0 { 1.0 } else { -1.0 }, stats: s }
    }
    pub fn alive(&self) -> bool { self.state != State::Dead }
    pub fn grounded(&self) -> bool { self.z <= 0.0001 && self.vz <= 0.0 }
    pub fn active(&self) -> bool { self.alive() && self.rise <= 0.0 && self.drop <= 0.0 }
    pub fn phase(&self) -> Option<Phase> {
        let r = self.run.as_ref()?;
        let a = r.atk;
        Some(if self.t < a.windup { Phase::Windup } else if self.t < a.windup + a.active { Phase::Active } else { Phase::Recovery })
    }
    pub fn invulnerable(&self) -> bool {
        // Dodge i-frames quantised to 60 Hz ticks: 0.067 .. 0.233 s.
        self.state == State::Dodge && self.t >= 0.06 && self.t < 0.24
    }
    fn regen_state(&self) -> bool { self.state == State::Idle }
}

#[derive(Clone, Debug)]
pub struct Projectile {
    pub spec: &'static ProjSpec,
    pub pos: Vec2,
    pub z: f32,
    pub dir: Vec2,
    pub left: f32,
    pub owner: u32,
    pub from_player: bool,
    pub passed: Vec<u32>,
    pub alive: bool,
}

#[derive(Clone, Debug)]
pub enum Event {
    Damage { pos: Vec2, z: f32, amount: i32, to_player: bool },
    Hurt,
    Message(String),
    Shake(f32),
    Swing { uid: u32 },
    Stagger { pos: Vec2 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Wave(u8),
    GateOpen,
    BossArrival,
    Boss,
    Victory,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub mv: Vec2,
    pub aim: Vec2,
    pub light: bool,
    pub heavy: bool,
    pub dodge: bool,
    pub jump: bool,
    pub jump_held: bool,
    pub flask: bool,
    pub rest: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action { Light, Heavy, Dodge, Jump, Flask, Rest }

#[derive(Clone, Debug)]
pub struct BossBrain {
    bag: Vec<usize>,
    combo: Option<(usize, usize)>,
    pub phase2: bool,
    toll_pending: bool,
    gap: f32,
}

#[derive(Clone, Debug)]
pub struct Sim {
    pub actors: Vec<Actor>,
    pub projectiles: Vec<Projectile>,
    pub events: Vec<Event>,
    pub stage: Stage,
    pub stage_t: f32,
    pub gate_open: bool,
    pub mist: bool,
    pub boss_defeated: bool,
    pub time: f32,
    pub hitstop: f32,
    pub dead_t: f32,
    pub boss: BossBrain,
    pub banner: Option<(String, f32)>,
    buffer: Option<(Action, f32)>,
    next_uid: u32,
    turn_gap: f32,
    rng: u32,
    wave_clear_t: f32,
    pending_rest: bool,
}

pub static WAVES: [&[Kind]; 3] = [
    &[Kind::Husk, Kind::Husk, Kind::Cultist],
    &[Kind::Husk, Kind::Duelist, Kind::Cultist, Kind::Husk],
    &[Kind::Duelist, Kind::Pyreguard, Kind::Cultist, Kind::Husk, Kind::Husk],
];

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

pub fn line_clear(a: Vec2, b: Vec2) -> bool { all_pillars().all(|p| seg_hits_circle(a, b, p, PILLAR_R).is_none()) }

impl Sim {
    pub fn walkable(&self, p: Vec2, r: f32, player: bool) -> bool {
        let in_nave = p.x >= NAVE_MIN.x + r && p.x <= NAVE_MAX.x - r && p.y >= NAVE_MIN.y + r && p.y <= NAVE_MAX.y - r;
        let in_corr = self.gate_open && p.x.abs() <= GATE_HALF_W - r && p.y <= NAVE_MIN.y + 0.5 && p.y >= CORRIDOR_END_Y - 0.5;
        let in_belfry = p.distance(BELFRY_CENTER) <= BELFRY_R - r;
        if player && self.mist {
            // The mist wall seals the belfry while the Bellwarden lives.
            return in_belfry && p.y < CORRIDOR_END_Y + 0.6 - r;
        }
        in_nave || in_corr || in_belfry
    }

    fn rand(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13; x ^= x >> 17; x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    #[allow(dead_code)]
    fn shuffle_bag(&mut self, n: usize) -> Vec<usize> {
        let mut v: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() { let j = (self.rand() * (i + 1) as f32) as usize % (i + 1); v.swap(i, j); }
        v
    }

    pub fn new() -> Self {
        let mut s = Sim {
            actors: Vec::new(), projectiles: Vec::new(), events: Vec::new(), stage: Stage::Wave(0), stage_t: 0.0,
            gate_open: false, mist: false, boss_defeated: false, time: 0.0, hitstop: 0.0, dead_t: 0.0,
            boss: BossBrain { bag: Vec::new(), combo: None, phase2: false, toll_pending: false, gap: 1.0 },
            banner: None, buffer: None, next_uid: 1, turn_gap: 0.0, rng: 0x9E37_79B9, wave_clear_t: 0.0, pending_rest: false,
        };
        let uid = s.uid();
        s.actors.push(Actor::new(uid, Kind::Knight, NAVE_ALTAR + Vec2::new(0.0, -1.2), Vec2::new(0.0, -1.0)));
        s.start_wave(0);
        s
    }

    fn uid(&mut self) -> u32 { self.next_uid += 1; self.next_uid }

    pub fn player(&self) -> &Actor { &self.actors[0] }
    pub fn boss_actor(&self) -> Option<&Actor> { self.actors.iter().find(|a| a.kind == Kind::Bellwarden) }
    pub fn enemies_alive(&self) -> usize { self.actors.iter().skip(1).filter(|a| a.alive()).count() }

    fn say(&mut self, msg: &str, secs: f32) {
        self.banner = Some((msg.to_string(), secs));
        self.events.push(Event::Message(msg.to_string()));
    }

    fn start_wave(&mut self, n: u8) {
        self.actors.truncate(1);
        self.projectiles.clear();
        self.stage = Stage::Wave(n);
        self.stage_t = 0.0;
        let spots = [Vec2::new(-4.0, -5.0), Vec2::new(4.0, -5.0), Vec2::new(0.0, -6.0), Vec2::new(-8.5, -2.0), Vec2::new(8.5, -2.0)];
        for (i, &k) in WAVES[n as usize].iter().enumerate() {
            let mut p = spots[i % spots.len()];
            if k == Kind::Cultist { p = Vec2::new(if i % 2 == 0 { -9.0 } else { 9.0 }, -6.5); }
            let uid = self.uid();
            let mut a = Actor::new(uid, k, p, Vec2::new(0.0, 1.0));
            a.cooldown = 0.8 + i as f32 * 0.35;
            if n > 0 { a.rise = 1.2; }
            self.actors.push(a);
        }
        self.say(&format!("WAVE {} OF 3", n + 1), 1.8);
    }

    fn spawn_boss(&mut self) {
        self.actors.truncate(1);
        self.projectiles.clear();
        let uid = self.uid();
        let mut b = Actor::new(uid, Kind::Bellwarden, BELFRY_CENTER + Vec2::new(0.0, -2.5), Vec2::new(0.0, 1.0));
        b.drop = 2.2;
        self.actors.push(b);
        self.boss = BossBrain { bag: Vec::new(), combo: None, phase2: false, toll_pending: false, gap: 1.0 };
        self.stage = Stage::BossArrival;
        self.stage_t = 0.0;
    }

    fn respawn_player(&mut self, at: Vec2) {
        let p = &mut self.actors[0];
        let s = p.stats;
        *p = Actor::new(p.uid, Kind::Knight, at, Vec2::new(0.0, -1.0));
        p.hp = s.hp; p.chip = s.hp;
    }

    /// Restart everything: the whole cathedral resets.
    pub fn restart(&mut self) {
        *self = Sim::new();
    }

    // -------------------------------------------------------- step ----

    pub fn step(&mut self, input: &Input) {
        self.time += TICK;
        if let Some((_, t)) = &mut self.banner { *t -= TICK; if *t <= 0.0 { self.banner = None; } }
        self.buffer_input(input);
        if self.hitstop > 0.0 { self.hitstop -= TICK; return; }
        self.stage_t += TICK;
        self.turn_gap -= TICK;

        self.update_player(input);
        self.update_enemies();
        for i in 0..self.actors.len() { self.tick_actor(i); }
        self.resolve_bodies();
        self.release_projectiles();
        self.fly_projectiles();
        self.melee_hits();
        self.cleanup();
        self.update_stage();
    }

    fn buffer_input(&mut self, input: &Input) {
        let action = if input.dodge { Some(Action::Dodge) } else if input.light { Some(Action::Light) }
            else if input.heavy { Some(Action::Heavy) } else if input.jump { Some(Action::Jump) }
            else if input.flask { Some(Action::Flask) } else if input.rest { Some(Action::Rest) } else { None };
        if let Some(a) = action { self.buffer = Some((a, self.time)); }
        if let Some((_, t)) = self.buffer { if self.time - t > INPUT_BUFFER { self.buffer = None; } }
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

    fn update_player(&mut self, input: &Input) {
        let can_rest_here = self.can_rest_here();
        let p = &mut self.actors[0];
        if !p.alive() { return; }
        let aim_dir = (input.aim - p.pos).normalized();
        let mv = input.mv.normalized();
        let free = p.state == State::Idle;

        // Facing follows the cursor while free (mouse aim).
        if free && aim_dir.length() > 0.5 { p.facing = aim_dir; }

        if let Some((action, _)) = self.buffer {
            let grounded = p.grounded();
            let mut consumed = false;
            match action {
                Action::Light | Action::Heavy if free && grounded && p.stamina > 0.0 => {
                    let atk: &'static Attack = if action == Action::Light { &VIGIL_CUT } else { &ASHFALL_CLEAVE };
                    Self::spend(p, atk.stamina);
                    if aim_dir.length() > 0.5 { p.facing = aim_dir; }
                    Self::begin_attack(p, atk, input.aim);
                    self.events.push(Event::Swing { uid: p.uid });
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
                Action::Jump if free && grounded => {
                    p.vz = 8.0;
                    consumed = true;
                }
                Action::Flask if free && grounded => {
                    if p.flasks > 0 { p.state = State::Heal; p.t = 0.0; p.healed = false; p.flasks -= 1; }
                    consumed = true;
                }
                Action::Rest if free && grounded => {
                    consumed = true;
                    match can_rest_here {
                        Some(true) => self.pending_rest = true,
                        Some(false) => { self.banner = Some(("You cannot rest while the Bellwarden lives".into(), 2.0)); }
                        None => {}
                    }
                }
                _ => {}
            }
            if consumed { self.buffer = None; }
        }
        let p = &mut self.actors[0];

        // Variable jump height: releasing early cuts the rise.
        if !input.jump_held && p.vz > 3.0 { p.vz = 3.0; }

        let speed = p.stats.speed * match p.state { State::Heal => 0.35, _ => 1.0 };
        let want = match p.state { State::Idle | State::Heal => mv * speed, _ => Vec2::ZERO };
        if p.state == State::Idle || p.state == State::Heal {
            let accel = if p.grounded() { if want.length() > 0.01 { 32.0 } else { 40.0 } } else { 5.0 };
            let diff = want - p.vel;
            let step = accel * TICK;
            p.vel = if diff.length() <= step { want } else { p.vel + diff.normalized() * step };
        }
    }

    fn can_rest_here(&self) -> Option<bool> {
        let p = &self.actors[0];
        let at_nave = p.pos.distance(NAVE_ALTAR) < ALTAR_REACH;
        let at_belfry = p.pos.distance(BELFRY_ALTAR) < ALTAR_REACH;
        if at_belfry {
            Some(self.boss_defeated)
        } else if at_nave {
            Some(self.enemies_alive() == 0 || self.boss_defeated)
        } else { None }
    }

    fn update_enemies(&mut self) {
        let ppos = self.actors[0].pos;
        let palive = self.actors[0].alive();
        let pz = self.actors[0].z;
        let _ = pz;
        // Attack turns: at most two melee fighters and one caster attacking at once.
        let mut melee_busy = 0; let mut caster_busy = 0;
        for a in self.actors.iter().skip(1) {
            if a.state == State::Attack && a.kind != Kind::Bellwarden {
                if a.kind == Kind::Cultist { caster_busy += 1 } else { melee_busy += 1 }
            }
        }
        let boss_proj_alive = self.projectiles.iter().any(|p| p.alive && !p.from_player && p.spec.look != 0);
        for i in 1..self.actors.len() {
            if !self.actors[i].active() { continue; }
            // Separation from other enemies.
            let mut push = Vec2::ZERO;
            for j in 1..self.actors.len() {
                if i == j || !self.actors[j].alive() { continue; }
                let d = self.actors[i].pos - self.actors[j].pos;
                let min = self.actors[i].stats.hurt_r + self.actors[j].stats.hurt_r + 0.6;
                let l = d.length();
                if l < min && l > 0.001 { push += d.normalized() * (min - l); }
            }
            let r1 = self.rand();
            let r2 = self.rand();
            let a = &mut self.actors[i];
            let to = ppos - a.pos;
            let dist = to.length();
            let dir = to.normalized();
            a.cooldown -= TICK;

            match a.state {
                State::Attack => {
                    let turn = if a.kind == Kind::Bellwarden { 2.0 } else { 3.5 };
                    if a.phase() == Some(Phase::Windup) && a.kind != Kind::Cultist {
                        a.facing = turn_toward(a.facing, to, turn * TICK);
                    }
                    continue;
                }
                State::Idle => {}
                _ => continue,
            }
            if !palive { a.vel = a.vel * 0.8; continue; }
            a.facing = turn_toward(a.facing, to, 6.0 * TICK);
            let speed = a.stats.speed;

            if a.kind == Kind::Bellwarden {
                let brain = &mut self.boss;
                brain.gap -= TICK;
                if !brain.phase2 && a.hp <= a.stats.hp * 0.5 {
                    brain.phase2 = true;
                    brain.toll_pending = true;
                    brain.bag.clear();
                    brain.combo = None;
                }
                let combos: &[&[&Attack]] = if brain.phase2 { &PHASE2_COMBOS } else { &PHASE1_COMBOS };
                let next: Option<&'static Attack> = if brain.toll_pending { Some(&GRAVE_TOLL) }
                    else if let Some((c, s)) = brain.combo { Some(combos[c][s]) } else { None };
                if next.is_none() && brain.gap <= 0.0 {
                    if brain.bag.is_empty() {
                        let n = combos.len();
                        let mut v: Vec<usize> = (0..n).collect();
                        let mut seed = (self.time * 1000.0) as u32 | 1;
                        for k in (1..n).rev() { seed ^= seed << 13; seed ^= seed >> 17; seed ^= seed << 5; v.swap(k, seed as usize % (k + 1)); }
                        brain.bag = v;
                    }
                    let c = brain.bag.pop().unwrap();
                    brain.combo = Some((c, 0));
                }
                let next: Option<&'static Attack> = if brain.toll_pending { Some(&GRAVE_TOLL) }
                    else if let Some((c, s)) = brain.combo { Some(combos[c][s]) } else { None };
                let mut wants_walk = true;
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
                        self.events.push(Event::Swing { uid: a.uid });
                        wants_walk = false;
                    }
                }
                if wants_walk {
                    let want = if dist > 2.4 { dir * speed } else if dist < 1.6 { dir * -speed * 0.6 } else { Vec2::ZERO };
                    a.vel = a.vel.lerp(want, 1.0 - (-6.0 * TICK).exp());
                }
                continue;
            }

            if a.kind == Kind::Cultist {
                let want = if dist < 2.6 { dir * -speed }
                    else if dist > 8.5 { dir * speed }
                    else { perp(dir) * a.strafe * speed * 0.35 };
                if a.cooldown <= 0.0 && dist >= 2.6 && dist <= 13.0 && caster_busy == 0 && self.turn_gap <= 0.0
                    && line_clear(a.pos, ppos) {
                    let atk: &'static Attack = if r1 < 0.62 { &CINDER_BOLT } else { &ASH_VOLLEY };
                    Self::begin_attack(a, atk, ppos);
                    a.facing = dir;
                    caster_busy += 1;
                    self.turn_gap = 0.45;
                    continue;
                }
                a.vel = a.vel.lerp(want + push * 2.0, 1.0 - (-8.0 * TICK).exp());
                continue;
            }

            // Melee fighters: husk, duelist, pyreguard.
            let (light, heavy): (&'static Attack, &'static Attack) = match a.kind {
                Kind::Husk => (&RAKE, &HACK),
                Kind::Duelist => (&CUTTING_DRAW, &LUNGING_THRUST),
                _ => (&HEW, &PYRE_CLEAVE),
            };
            let pick: &'static Attack = if a.kind == Kind::Duelist {
                if dist > 2.0 { heavy } else { light }
            } else if r2 < 0.35 { heavy } else { light };
            let reach = pick.reach() + if pick.lunge > 4.0 { 1.0 } else { 0.2 };
            let can_turn = melee_busy < 2 && self.turn_gap <= 0.0;
            if a.cooldown <= 0.0 && dist <= reach && can_turn && dot(a.facing, dir) > 0.8 && line_clear(a.pos, ppos) {
                Self::begin_attack(a, pick, ppos);
                melee_busy += 1;
                self.turn_gap = 0.45;
                self.events.push(Event::Swing { uid: a.uid });
                continue;
            }
            // Approach to just outside reach, circle when waiting for a turn.
            let hold = light.reach() + 0.3;
            let mut want = if dist > hold + 0.2 { dir * speed }
                else if dist < hold - 0.6 { dir * -speed * 0.5 }
                else { perp(dir) * a.strafe * speed * 0.4 };
            if !can_turn && dist < 3.5 { want = perp(dir) * a.strafe * speed * 0.5 + dir * -0.6; }
            if r1 < 0.004 { a.strafe = -a.strafe; }
            a.vel = a.vel.lerp(want + push * 2.0, 1.0 - (-8.0 * TICK).exp());
        }
        // Boss uses its own cadence; regular cooldowns are reset when attacks end (tick_actor).
    }

    fn tick_actor(&mut self, i: usize) {
        let mut ev: Vec<Event> = Vec::new();
        let rnd = self.rand();
        let a = &mut self.actors[i];
        a.flash = (a.flash - TICK).max(0.0);
        if a.chip > a.hp { a.chip_hold -= TICK; if a.chip_hold <= 0.0 { a.chip = (a.chip - 60.0 * TICK).max(a.hp); } } else { a.chip = a.hp; }
        if a.rise > 0.0 { a.rise -= TICK; return; }
        if a.drop > 0.0 { a.drop -= TICK; if a.drop <= 0.0 { ev.push(Event::Shake(1.0)); } self.events.extend(ev); return; }
        a.t += TICK;

        // Vertical motion (jumping).
        if a.z > 0.0 || a.vz > 0.0 {
            a.z += a.vz * TICK;
            a.vz -= 24.0 * TICK;
            if a.z <= 0.0 { a.z = 0.0; a.vz = 0.0; }
        }

        match a.state {
            State::Attack => {
                let atk = a.run.as_ref().unwrap().atk;
                let ph = a.phase();
                if ph == Some(Phase::Active) && atk.lunge > 0.0 { a.vel = a.facing * atk.lunge; }
                else { a.vel = a.vel * 0.7; }
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

        // Stamina regenerates only while free or airborne, 0.7 s after the last spend.
        if a.stamina_wait > 0.0 { a.stamina_wait -= TICK; }
        else if a.regen_state() { a.stamina = (a.stamina + 25.0 * TICK).min(100.0); }
        if a.poise_wait > 0.0 { a.poise_wait -= TICK; }
        else { a.poise = (a.poise + a.stats.poise_regen * TICK).min(a.stats.poise); }

        a.knock = a.knock * (-9.0 * TICK).exp();
        self.events.extend(ev);
        // Movement with wall sliding.
        let a = &self.actors[i];
        let delta = (a.vel + a.knock) * TICK;
        let (pos, r, player) = (a.pos, a.stats.hurt_r.min(0.6), a.player);
        let mut np = pos + delta;
        if !self.walkable(np, r, player) {
            let nx = Vec2::new(pos.x + delta.x, pos.y);
            let ny = Vec2::new(pos.x, pos.y + delta.y);
            np = if self.walkable(nx, r, player) { nx } else if self.walkable(ny, r, player) { ny } else { pos };
        }
        for pl in all_pillars() {
            let d = np - pl;
            let min = PILLAR_R + r;
            if d.length() < min { np = pl + d.normalized() * min; }
        }
        if !self.walkable(np, r, player) { np = pos; }
        self.actors[i].pos = np;
    }

    fn resolve_bodies(&mut self) {
        let p = self.actors[0].clone();
        if !p.alive() { return; }
        for i in 1..self.actors.len() {
            let e = &self.actors[i];
            if !e.active() { continue; }
            let d = p.pos - e.pos;
            let min = p.stats.hurt_r + e.stats.hurt_r * 0.8;
            let l = d.length();
            if l < min && l > 0.0001 && p.z < 0.9 {
                let n = p.pos + d.normalized() * (min - l);
                if self.walkable(n, p.stats.hurt_r, true) { self.actors[0].pos = n; }
            }
        }
    }

    fn release_projectiles(&mut self) {
        let mut spawned = Vec::new();
        for a in self.actors.iter_mut() {
            if a.state != State::Attack || a.phase() != Some(Phase::Active) { continue; }
            let run = a.run.as_mut().unwrap();
            if run.released { continue; }
            let atk = run.atk;
            let Some(spec) = atk.proj else { continue };
            run.released = true;
            let origin = a.pos + a.facing * atk.muzzle_fwd;
            if atk.ring {
                for k in 0..atk.count {
                    let ang = k as f32 / atk.count as f32 * std::f32::consts::TAU;
                    let dir = Vec2::new(ang.cos(), ang.sin());
                    spawned.push(Projectile { spec, pos: origin, z: atk.muzzle_h, dir, left: spec.range, owner: a.uid, from_player: a.player, passed: Vec::new(), alive: true });
                }
            } else {
                // Aim where the target stood when the cast began; never homes.
                let base = if a.kind == Kind::Cultist { (run.aim - origin).normalized() } else { a.facing };
                let base = if base.length() < 0.5 { a.facing } else { base };
                for k in 0..atk.count {
                    let off = if atk.count > 1 { (k as f32 / (atk.count - 1) as f32 - 0.5) * atk.spread_deg.to_radians() } else { 0.0 };
                    spawned.push(Projectile { spec, pos: origin, z: atk.muzzle_h, dir: rot(base, off), left: spec.range, owner: a.uid, from_player: a.player, passed: Vec::new(), alive: true });
                }
            }
        }
        // A bolt that would start inside the player fizzles instead.
        let p = &self.actors[0];
        spawned.retain(|pr| pr.from_player || pr.pos.distance(p.pos) > pr.spec.radius + p.stats.hurt_r || pr.spec.look == 2);
        self.projectiles.extend(spawned);
    }

    fn fly_projectiles(&mut self) {
        for pi in 0..self.projectiles.len() {
            if !self.projectiles[pi].alive { continue; }
            let (a, b, spec, z, from_player) = {
                let pr = &self.projectiles[pi];
                let step = pr.spec.speed * TICK;
                (pr.pos, pr.pos + pr.dir * step, pr.spec, pr.z, pr.from_player)
            };
            // Earliest contact along the swept path wins.
            let mut best: Option<(f32, usize)> = None;
            for (ti, t) in self.actors.iter().enumerate() {
                if !t.active() || t.player == from_player { continue; }
                if self.projectiles[pi].passed.contains(&t.uid) { continue; }
                // Vertical overlap of the orb band and the body.
                if z + spec.radius < t.z || z - spec.radius > t.z + t.stats.height { continue; }
                if let Some(s) = seg_hits_circle(a, b, t.pos, spec.radius + t.stats.hurt_r) {
                    if best.map_or(true, |(bs, _)| s < bs) { best = Some((s, ti)); }
                }
            }
            let mut wall_t: Option<f32> = None;
            for pl in all_pillars() {
                if let Some(s) = seg_hits_circle(a, b, pl, PILLAR_R + spec.radius * 0.5) { wall_t = Some(wall_t.map_or(s, |w: f32| w.min(s))); }
            }
            if !self.walkable(b, 0.0, false) { wall_t = Some(wall_t.map_or(1.0, |w| w.min(1.0))); }
            if let Some((s, ti)) = best {
                if wall_t.map_or(true, |w| s <= w) {
                    if self.actors[ti].invulnerable() {
                        let uid = self.actors[ti].uid;
                        self.projectiles[pi].passed.push(uid);
                    } else {
                        let owner = self.projectiles[pi].owner;
                        let dir = self.projectiles[pi].dir;
                        self.projectiles[pi].alive = false;
                        self.apply_hit(ti, owner, spec.damage, spec.poise, spec.knockback, dir, false);
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
        // Heights: the body spans z .. z + height.
        !(target.z >= atk.top || target.z + target.stats.height <= atk.bot)
    }

    fn melee_hits(&mut self) {
        let n = self.actors.len();
        for i in 0..n {
            let a = &self.actors[i];
            if a.state != State::Attack || a.phase() != Some(Phase::Active) { continue; }
            let run = a.run.as_ref().unwrap();
            let atk = run.atk;
            if !atk.is_melee() { continue; }
            for j in 0..n {
                if i == j { continue; }
                let (a, t) = (&self.actors[i], &self.actors[j]);
                if a.player == t.player || !t.active() { continue; }
                if a.run.as_ref().unwrap().hit.contains(&t.uid) { continue; }
                if !Self::hitbox_contains(a, atk, t) { continue; }
                // Cover blocks melee: chest to the target's centre or either flank.
                let side = perp((t.pos - a.pos).normalized()) * (t.stats.hurt_r * 0.9);
                if !(line_clear(a.pos, t.pos) || line_clear(a.pos, t.pos + side) || line_clear(a.pos, t.pos - side)) { continue; }
                if t.invulnerable() { continue; }
                let (tuid, auid, dir) = (t.uid, a.uid, (t.pos - a.pos).normalized());
                self.actors[i].run.as_mut().unwrap().hit.push(tuid);
                self.apply_hit(j, auid, atk.damage, atk.poise, atk.knockback, dir, atk.heavy);
            }
        }
    }

    fn apply_hit(&mut self, ti: usize, _from: u32, damage: f32, poise: f32, knockback: f32, dir: Vec2, heavy: bool) {
        let t = &mut self.actors[ti];
        if !t.alive() { return; }
        t.hp -= damage;
        t.chip_hold = 0.5;
        t.flash = 0.12;
        t.poise -= poise;
        t.poise_wait = t.stats.poise_delay;
        t.knock = dir * knockback * 0.8;
        let to_player = t.player;
        self.events.push(Event::Damage { pos: t.pos, z: t.stats.height + 0.3, amount: damage.round() as i32, to_player });
        if to_player { self.events.push(Event::Hurt); self.events.push(Event::Shake(if heavy { 0.8 } else { 0.4 })); }
        self.hitstop = self.hitstop.max(if heavy { 0.09 } else { 0.06 });
        if t.hp <= 0.0 {
            t.hp = 0.0;
            t.state = State::Dead;
            t.t = 0.0;
            t.run = None;
            if t.kind == Kind::Bellwarden { self.events.push(Event::Shake(1.4)); }
            return;
        }
        if t.poise <= 0.0 {
            // A stagger interrupts the attack (and ends a boss combo).
            t.poise = t.stats.poise;
            if t.state == State::Heal && !t.healed { /* the flask is spent */ }
            t.state = State::Stagger;
            t.t = 0.0;
            t.run = None;
            let pos = t.pos;
            if t.kind == Kind::Bellwarden { self.boss.combo = None; self.boss.gap = 0.8; }
            self.events.push(Event::Stagger { pos });
        }
    }

    fn cleanup(&mut self) {
        // Fallen enemies linger briefly, then crumble.
        let mut i = 1;
        while i < self.actors.len() {
            let a = &self.actors[i];
            if a.state == State::Dead && a.kind != Kind::Bellwarden && a.t > 1.6 { self.actors.remove(i); } else { i += 1; }
        }
    }

    fn update_stage(&mut self) {
        let p_alive = self.actors[0].alive();
        if self.pending_rest {
            self.pending_rest = false;
            self.rest();
            return;
        }
        if !p_alive {
            if self.dead_t == 0.0 { self.say("YOU HAVE FALLEN", 3.0); }
            self.dead_t += TICK;
            if self.dead_t >= 3.0 {
                self.dead_t = 0.0;
                match self.stage {
                    Stage::Wave(n) => { self.respawn_player(NAVE_ALTAR + Vec2::new(0.0, -1.2)); self.start_wave(n); }
                    Stage::BossArrival | Stage::Boss => {
                        // Only the Bellwarden resets; the waves stay cleared.
                        self.respawn_player(BELFRY_ALTAR + Vec2::new(0.0, -1.2));
                        self.spawn_boss();
                    }
                    Stage::GateOpen | Stage::Victory => {
                        let at = if area_of(self.actors[0].pos) == Area::Belfry { BELFRY_ALTAR } else { NAVE_ALTAR };
                        self.respawn_player(at + Vec2::new(0.0, -1.2));
                    }
                }
            }
            return;
        }
        match self.stage {
            Stage::Wave(n) => {
                if self.enemies_alive() == 0 {
                    self.wave_clear_t += TICK;
                    if self.wave_clear_t == TICK {
                        let p = &mut self.actors[0];
                        p.flasks = (p.flasks + 1).min(3);
                    }
                    if self.wave_clear_t >= 1.6 {
                        self.wave_clear_t = 0.0;
                        if (n as usize) + 1 < WAVES.len() { self.start_wave(n + 1); }
                        else {
                            self.stage = Stage::GateOpen;
                            self.gate_open = true;
                            self.say("THE BELFRY GATE OPENS", 3.0);
                        }
                    }
                }
            }
            Stage::GateOpen => {
                if !self.boss_defeated && self.actors[0].pos.y < CORRIDOR_END_Y - 0.8 {
                    self.mist = true;
                    self.spawn_boss();
                }
            }
            Stage::BossArrival => {
                if self.stage_t >= 2.2 { self.stage = Stage::Boss; self.stage_t = 0.0; }
            }
            Stage::Boss => {
                if self.boss_actor().map_or(false, |b| !b.alive()) {
                    self.boss_defeated = true;
                    self.mist = false;
                    self.stage = Stage::Victory;
                    self.projectiles.clear();
                    self.say("THE BELL FALLS SILENT", 5.0);
                }
            }
            Stage::Victory => {}
        }
    }

    fn rest(&mut self) {
        let in_belfry = area_of(self.actors[0].pos) == Area::Belfry;
        if self.boss_defeated && in_belfry {
            // Resting after the Bellwarden falls resets the whole cathedral.
            self.restart();
            self.say("THE VIGIL BEGINS ANEW", 2.5);
            return;
        }
        let p = &mut self.actors[0];
        p.hp = p.stats.hp; p.chip = p.hp; p.flasks = 3; p.stamina = 100.0;
        self.say("YOU REST AT THE VIGIL ALTAR", 2.0);
    }
}

impl Default for Sim { fn default() -> Self { Sim::new() } }

/// Fairness rules from the original BellwardenAI.validate_combos().
pub fn validate_combos() -> Result<(), String> {
    let mut volleys = 0;
    for combo in PHASE1_COMBOS.iter().chain(PHASE2_COMBOS.iter()) {
        let last = combo.last().ok_or("empty combo")?;
        if last.recovery < 0.9 { return Err(format!("{} ends on {:.2}s recovery", last.name, last.recovery)); }
        for a in combo.iter() {
            if a.windup < 0.6 { return Err(format!("{} winds up only {:.2}s", a.name, a.windup)); }
            if a.tell.is_empty() { return Err(format!("{} has no tell", a.name)); }
        }
        volleys = volleys.max(combo.iter().filter(|a| a.proj.is_some()).count());
    }
    let _ = volleys;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> Input { Input { aim: Vec2::new(0.0, -10.0), ..Default::default() } }

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
        assert!(Sim::hitbox_contains(&boss, &TOLLING_SWEEP, &k));
        k.z = 1.0;
        assert!(!Sim::hitbox_contains(&boss, &TOLLING_SWEEP, &k));
        assert!(Sim::hitbox_contains(&boss, &BELL_SLAM, &k));
        // Max jump height (v^2 / 2g) must clear the LOW band.
        assert!(8.0f32 * 8.0 / (2.0 * 24.0) > LOW_CLEARANCE);
    }

    #[test]
    fn light_attack_spends_stamina_and_hits() {
        let mut s = Sim::new();
        // Put one husk right in front of the player.
        s.actors.truncate(1);
        let ppos = s.actors[0].pos;
        let mut h = Actor::new(99, Kind::Husk, ppos + Vec2::new(0.0, -1.2), Vec2::new(0.0, 1.0));
        h.cooldown = 99.0;
        s.actors.push(h);
        let mut inp = idle(); inp.aim = ppos + Vec2::new(0.0, -5.0); inp.light = true;
        s.step(&inp);
        assert_eq!(s.actors[0].state, State::Attack);
        assert!(s.actors[0].stamina <= 80.0 + 0.01);
        let inp = Input { aim: inp.aim, ..Default::default() };
        for _ in 0..40 { s.step(&inp); }
        assert!(s.actors[1].hp < 48.0, "husk hp {}", s.actors[1].hp);
    }

    #[test]
    fn stamina_waits_before_regen() {
        let mut s = Sim::new();
        s.actors.truncate(1);
        s.actors[0].stamina = 50.0;
        s.actors[0].stamina_wait = 0.7;
        for _ in 0..30 { s.step(&idle()); }
        assert!((s.actors[0].stamina - 50.0).abs() < 0.01);
        for _ in 0..30 { s.step(&idle()); }
        assert!(s.actors[0].stamina > 50.0);
    }

    #[test]
    fn flask_heals_45_after_point_six_seconds() {
        let mut s = Sim::new();
        s.actors.truncate(1);
        s.stage = Stage::GateOpen;
        s.actors[0].hp = 30.0;
        let mut inp = idle(); inp.flask = true;
        s.step(&inp);
        assert_eq!(s.actors[0].flasks, 2);
        for _ in 0..30 { s.step(&idle()); }
        assert!((s.actors[0].hp - 30.0).abs() < 0.01);
        for _ in 0..10 { s.step(&idle()); }
        assert!((s.actors[0].hp - 75.0).abs() < 0.01);
    }

    #[test]
    fn full_run_reaches_the_belfry_without_panicking() {
        // Clear every wave by fiat, walk through the gate, and let the boss fight play out.
        let mut s = Sim::new();
        for _ in 0..3 {
            for a in s.actors.iter_mut().skip(1) { a.hp = 0.0; a.state = State::Dead; }
            for _ in 0..200 { s.step(&idle()); }
        }
        assert_eq!(s.stage, Stage::GateOpen);
        s.actors[0].pos = Vec2::new(0.0, -13.0);
        s.step(&idle());
        assert!(s.mist);
        assert!(matches!(s.stage, Stage::BossArrival));
        // Survive with infinite HP for a while: boss must cycle combos and enter phase 2.
        for k in 0..6000 {
            s.actors[0].hp = 10_000.0;
            if k == 600 { let b = s.actors.iter_mut().find(|a| a.kind == Kind::Bellwarden).unwrap(); b.hp = 200.0; }
            s.step(&idle());
        }
        assert!(s.boss.phase2);
    }

    #[test]
    fn death_in_wave_restarts_that_wave() {
        let mut s = Sim::new();
        for a in s.actors.iter_mut().skip(1) { a.hp = 0.0; a.state = State::Dead; }
        for _ in 0..200 { s.step(&idle()); }
        assert_eq!(s.stage, Stage::Wave(1));
        s.actors[0].hp = 0.0; s.actors[0].state = State::Dead;
        for _ in 0..200 { s.step(&idle()); }
        assert_eq!(s.stage, Stage::Wave(1));
        assert!(s.actors[0].alive());
        assert_eq!(s.enemies_alive(), WAVES[1].len());
    }
}
