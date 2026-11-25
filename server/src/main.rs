//! Realm Eternal authoritative server.
//!
//! Every world (the Eternal Concourse, the open Realm, and cathedral dungeon instances) is simulated
//! here at 60 Hz. Clients only send inputs; the server decides movement, hits, damage, deaths and
//! transfers, and streams interest-managed snapshots at 30 Hz.

use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use realm_shared::{
    combat::{self, Event, Input, Instance, PortalTo, Transfer, TICK},
    world::{band_name, WorldKind},
    ActorNet, ClientMessage, EventNet, FallenNet, MeNet, PortalNet, ProjNet, ServerMessage, Snapshot, Vec2,
    BTN_DODGE, BTN_FLASK, BTN_HEAVY, BTN_HUB, BTN_INTERACT, BTN_JUMP, BTN_JUMP_HELD, BTN_LIGHT, PROTOCOL_VERSION, TICK_HZ,
};
use std::{collections::HashMap, env, sync::{Arc, Mutex}, time::Duration};
use tokio::sync::mpsc;
use tower_http::services::ServeDir;

const VIEW_RADIUS: f32 = 22.0;
const CATHEDRAL_CAPACITY: usize = 6;
const REALM_SEED: u32 = 0xA5E1_2026;

struct Session {
    tx: mpsc::UnboundedSender<String>,
    name: String,
    level: u32,
    xp: u32,
    instance: u64,
    events: Vec<EventNet>,
}

struct Server {
    instances: HashMap<u64, Instance>,
    sessions: HashMap<u32, Session>,
    concourse: u64,
    realm: u64,
    next_uid: u32,
    next_instance: u64,
    tick: u64,
    memorial: Vec<FallenNet>,
}

type Shared = Arc<Mutex<Server>>;

impl Server {
    fn new() -> Self {
        let mut instances = HashMap::new();
        instances.insert(1, Instance::new(1, WorldKind::Concourse, 1));
        instances.insert(2, Instance::new(2, WorldKind::Realm, REALM_SEED));
        Server { instances, sessions: HashMap::new(), concourse: 1, realm: 2, next_uid: 1, next_instance: 10, tick: 0, memorial: Vec::new() }
    }

    fn send(&self, uid: u32, msg: &ServerMessage) {
        if let (Some(s), Ok(text)) = (self.sessions.get(&uid), serde_json::to_string(msg)) { s.tx.send(text).ok(); }
    }

    /// Move a player into an instance and tell their client which world to build.
    fn enter(&mut self, uid: u32, to: u64) {
        let Some(sess) = self.sessions.get(&uid) else { return };
        let (from, name, level, xp) = (sess.instance, sess.name.clone(), sess.level, sess.xp);
        if let Some(inst) = self.instances.get_mut(&from) {
            if let Some(info) = inst.remove_player(uid) {
                let s = self.sessions.get_mut(&uid).unwrap();
                s.level = info.level; s.xp = info.xp;
            }
        }
        let (level, xp) = self.sessions.get(&uid).map_or((level, xp), |s| (s.level, s.xp));
        let Some(inst) = self.instances.get_mut(&to) else { return };
        inst.add_player(uid, &name, level, xp);
        let (world, seed) = (inst.kind, inst.seed);
        let s = self.sessions.get_mut(&uid).unwrap();
        s.instance = to;
        s.events.clear();
        self.send(uid, &ServerMessage::Enter { world, seed, instance: to });
        if world == WorldKind::Concourse { self.send(uid, &ServerMessage::Memorial { fallen: self.memorial.clone() }); }
    }

    fn handle_transfer(&mut self, from: u64, uid: u32, t: Transfer) {
        match t {
            Transfer::Nexus => { let c = self.concourse; self.enter(uid, c); }
            Transfer::Realm => { let r = self.realm; self.enter(uid, r); }
            Transfer::Died => {
                // Permadeath: lay the character in the memorial hall and start a fresh recruit.
                let place = self.instances.get(&from).map(|i| match (&i.geo, i.actor(uid)) {
                    (combat::Geo::Map(m), Some(a)) => band_name(m.band_at(a.pos)).to_string(),
                    (combat::Geo::Cathedral, _) => "the Cathedral of the Vigil".to_string(),
                    _ => "the Realm".to_string(),
                }).unwrap_or_default();
                let level = self.instances.get(&from).and_then(|i| i.players.get(&uid)).map_or(1, |p| p.level);
                let name = self.sessions.get(&uid).map(|s| s.name.clone()).unwrap_or_default();
                self.memorial.insert(0, FallenNet { name: name.clone(), level, place: place.clone() });
                self.memorial.truncate(50);
                if let Some(inst) = self.instances.get_mut(&from) { inst.remove_player(uid); }
                if let Some(s) = self.sessions.get_mut(&uid) { s.level = 1; s.xp = 0; s.instance = 0; }
                self.send(uid, &ServerMessage::Died { name, level, place });
                let c = self.concourse;
                self.enter(uid, c);
            }
            Transfer::Portal(pid) => {
                let Some(inst) = self.instances.get(&from) else { return };
                let Some(portal) = inst.portals.iter().find(|p| p.id == pid).cloned() else { return };
                match portal.to {
                    PortalTo::Realm => { let r = self.realm; self.enter(uid, r); }
                    PortalTo::Nexus => { let c = self.concourse; self.enter(uid, c); }
                    PortalTo::Cathedral => {
                        let target = match portal.instance {
                            Some(id) if self.instances.contains_key(&id) => id,
                            _ => {
                                let id = self.next_instance;
                                self.next_instance += 1;
                                self.instances.insert(id, Instance::new(id, WorldKind::Cathedral, id as u32));
                                if let Some(p) = self.instances.get_mut(&from).unwrap().portals.iter_mut().find(|p| p.id == pid) { p.instance = Some(id); }
                                id
                            }
                        };
                        if self.instances[&target].player_count() >= CATHEDRAL_CAPACITY {
                            if let Some(s) = self.sessions.get_mut(&uid) { s.events.push(EventNet::Msg { text: "That cathedral is full (6 knights)".into(), secs: 2.0 }); }
                            return;
                        }
                        self.enter(uid, target);
                    }
                }
            }
        }
    }

    fn step(&mut self) {
        self.tick += 1;
        let ids: Vec<u64> = self.instances.keys().copied().collect();
        for id in ids {
            let inst = self.instances.get_mut(&id).unwrap();
            if inst.players.is_empty() && inst.kind != WorldKind::Cathedral { continue; }
            inst.step();
            let events = std::mem::take(&mut inst.events);
            let transfers = std::mem::take(&mut inst.transfers);
            let members: Vec<u32> = inst.players.keys().copied().collect();
            for ev in events {
                for &uid in &members {
                    let Some(s) = self.sessions.get_mut(&uid) else { continue };
                    let net = match &ev {
                        Event::Damage { pos, z, amount, to_player } => Some(EventNet::Dmg { x: pos.x, y: pos.y, z: *z, v: *amount, p: *to_player }),
                        Event::Hurt { uid: u } if *u == uid => Some(EventNet::Hurt),
                        Event::Message { to, text, secs } if to.map_or(true, |t| t == uid) => Some(EventNet::Msg { text: text.clone(), secs: *secs }),
                        Event::Shake { to, amount } if to.map_or(true, |t| t == uid) => Some(EventNet::Shake { a: *amount }),
                        Event::Stagger { pos } => Some(EventNet::Stagger { x: pos.x, y: pos.y }),
                        Event::LevelUp { uid: u, level } if *u == uid => Some(EventNet::Level { lv: *level }),
                        _ => None,
                    };
                    if let Some(n) = net { if s.events.len() < 64 { s.events.push(n); } }
                }
            }
            for (uid, t) in transfers { self.handle_transfer(id, uid, t); }
        }
        // Clean up empty dungeon instances whose portal is gone.
        let portal_targets: Vec<u64> = self.instances.values().flat_map(|i| i.portals.iter().filter_map(|p| p.instance)).collect();
        self.instances.retain(|id, i| i.kind != WorldKind::Cathedral || !(i.players.is_empty() && i.empty_for > 30.0 && !portal_targets.contains(id)) || false);
        let snap_every = (TICK_HZ / realm_shared::SNAPSHOT_HZ).round() as u64;
        if self.tick % snap_every == 0 { self.broadcast(); }
    }

    fn broadcast(&mut self) {
        let online = self.sessions.len() as u32;
        let uids: Vec<u32> = self.sessions.keys().copied().collect();
        for uid in uids {
            let (inst_id, events) = {
                let s = self.sessions.get_mut(&uid).unwrap();
                (s.instance, std::mem::take(&mut s.events))
            };
            let Some(inst) = self.instances.get(&inst_id) else { continue };
            let Some(me) = inst.actor(uid) else { continue };
            let center = me.pos;
            let wide = inst.kind == WorldKind::Cathedral;
            let near = |p: Vec2| wide || p.distance(center) < VIEW_RADIUS;
            let actors = inst.actors.iter().filter(|a| near(a.pos)).map(|a| {
                let run = a.run.as_ref();
                ActorNet {
                    u: a.uid, k: a.kind.to_u8(), x: a.pos.x, y: a.pos.y, z: a.z, fx: a.facing.x, fy: a.facing.y,
                    vx: a.vel.x + a.knock.x, vy: a.vel.y + a.knock.y,
                    hp: a.hp, mhp: a.stats.hp, ch: a.chip, po: (a.poise / a.stats.poise).clamp(0.0, 1.0),
                    s: a.state.to_u8(), t: a.t, a: run.map_or(-1, |r| combat::attack_index(r.atk) as i16),
                    ax: run.map_or(0.0, |r| r.aim.x), ay: run.map_or(0.0, |r| r.aim.y),
                    r: a.rise, d: a.drop, fl: a.flash > 0.0,
                    n: if a.player { inst.players.get(&a.uid).map(|p| p.name.clone()) } else { None },
                    lv: inst.players.get(&a.uid).map_or(0, |p| p.level as u8), band: a.band,
                }
            }).collect();
            let projs = inst.projectiles.iter().filter(|p| near(p.pos)).map(|p| ProjNet {
                i: p.id, l: p.spec.look, x: p.pos.x, y: p.pos.y, z: p.z, dx: p.dir.x * p.spec.speed, dy: p.dir.y * p.spec.speed,
            }).collect();
            let portals = inst.portals.iter().filter(|p| wide || p.pos.distance(center) < VIEW_RADIUS * 1.5).map(|p| PortalNet {
                i: p.id, x: p.pos.x, y: p.pos.y, ttl: p.ttl,
                to: match p.to { PortalTo::Realm => 0, PortalTo::Nexus => 1, PortalTo::Cathedral => 2 },
            }).collect();
            let info = &inst.players[&uid];
            let snap = Snapshot {
                tick: self.tick,
                me: MeNet { uid, name: info.name.clone(), stamina: me.stamina, flasks: me.flasks, level: info.level, xp: info.xp, next: combat::xp_to_next(info.level) },
                actors, projs, portals, events,
                stage: inst.stage.label(), gate: inst.gate_open, mist: inst.mist, phase2: inst.boss.phase2, online,
            };
            self.send(uid, &ServerMessage::Snap(snap));
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).init();
    let state: Shared = Arc::new(Mutex::new(Server::new()));
    tokio::spawn(world_loop(state.clone()));

    let static_dir = env::var("STATIC_DIR").unwrap_or_else(|_| "web".into());
    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/ws", get(ws_handler))
        .fallback_service(ServeDir::new(static_dir).append_index_html_on_directories(true))
        .with_state(state);

    let port: u16 = env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await.unwrap();
    tracing::info!(port, "Realm Eternal server listening");
    axum::serve(listener, app).await.unwrap();
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Shared>) -> Response {
    ws.on_upgrade(move |socket| session(socket, state)).into_response()
}

fn clean_name(raw: &str, uid: u32) -> String {
    let n: String = raw.chars().filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_').take(16).collect();
    let n = n.trim().to_string();
    if n.is_empty() { format!("Wanderer{uid}") } else { n }
}

async fn session(socket: WebSocket, state: Shared) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let writer = tokio::spawn(async move {
        while let Some(text) = rx.recv().await {
            if sink.send(Message::Text(text)).await.is_err() { break; }
        }
    });

    let mut uid: Option<u32> = None;
    while let Some(Ok(message)) = stream.next().await {
        let Message::Text(text) = message else { continue };
        let Ok(message) = serde_json::from_str::<ClientMessage>(&text) else { continue };
        match message {
            ClientMessage::Hello { version, name } => {
                if uid.is_some() { continue; }
                if version != PROTOCOL_VERSION {
                    tx.send(serde_json::to_string(&ServerMessage::Error { message: "Please refresh: the game was updated".into() }).unwrap()).ok();
                    break;
                }
                let mut s = state.lock().unwrap();
                let id = s.next_uid;
                s.next_uid += 1;
                let name = clean_name(&name, id);
                s.sessions.insert(id, Session { tx: tx.clone(), name, level: 1, xp: 0, instance: 0, events: Vec::new() });
                s.send(id, &ServerMessage::Welcome { id, tick_hz: TICK_HZ });
                let c = s.concourse;
                s.enter(id, c);
                uid = Some(id);
            }
            ClientMessage::Input { mx, my, ax, ay, b, .. } => {
                let Some(id) = uid else { continue };
                if !(mx.is_finite() && my.is_finite() && ax.is_finite() && ay.is_finite()) { continue; }
                let input = Input {
                    // The server clamps movement to a unit vector; speed comes from the sim, never the client.
                    mv: Vec2::new(mx.clamp(-1.0, 1.0), my.clamp(-1.0, 1.0)),
                    aim: Vec2::new(ax, ay),
                    light: b & BTN_LIGHT != 0, heavy: b & BTN_HEAVY != 0, dodge: b & BTN_DODGE != 0,
                    jump: b & BTN_JUMP != 0, jump_held: b & BTN_JUMP_HELD != 0, flask: b & BTN_FLASK != 0,
                    interact: b & BTN_INTERACT != 0, nexus: b & BTN_HUB != 0,
                };
                let mut s = state.lock().unwrap();
                let inst_id = s.sessions.get(&id).map(|x| x.instance);
                if let Some(inst) = inst_id.and_then(|i| s.instances.get_mut(&i)) { inst.set_input(id, &input); }
            }
            ClientMessage::Ping { client_ms } => {
                tx.send(serde_json::to_string(&ServerMessage::Pong { client_ms }).unwrap()).ok();
            }
        }
    }
    if let Some(id) = uid {
        let mut s = state.lock().unwrap();
        if let Some(sess) = s.sessions.remove(&id) {
            if let Some(inst) = s.instances.get_mut(&sess.instance) { inst.remove_player(id); }
        }
    }
    writer.abort();
}

async fn world_loop(state: Shared) {
    let mut interval = tokio::time::interval(Duration::from_secs_f32(TICK));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        state.lock().unwrap().step();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn join(s: &mut Server, name: &str) -> (u32, mpsc::UnboundedReceiver<String>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let id = s.next_uid; s.next_uid += 1;
        s.sessions.insert(id, Session { tx, name: name.into(), level: 1, xp: 0, instance: 0, events: Vec::new() });
        let c = s.concourse;
        s.enter(id, c);
        (id, rx)
    }

    #[test]
    fn two_players_see_each_other_and_travel_to_the_realm() {
        let mut s = Server::new();
        let (a, mut ra) = join(&mut s, "Aria");
        let (b, _rb) = join(&mut s, "Bram");
        for _ in 0..4 { s.step(); }
        let mut saw = false;
        while let Ok(m) = ra.try_recv() {
            if let Ok(ServerMessage::Snap(snap)) = serde_json::from_str::<ServerMessage>(&m) {
                if snap.actors.iter().any(|x| x.u == b && x.n.as_deref() == Some("Bram")) { saw = true; }
            }
        }
        assert!(saw, "Aria should see Bram in the Concourse");
        s.handle_transfer(s.concourse, a, Transfer::Portal(1));
        assert_eq!(s.sessions[&a].instance, s.realm);
        assert!(s.instances[&s.realm].actor(a).is_some());
        assert!(s.instances[&s.concourse].actor(a).is_none());
    }

    #[test]
    fn permadeath_resets_level_and_records_the_fallen() {
        let mut s = Server::new();
        let (a, _ra) = join(&mut s, "Doomed");
        let realm = s.realm;
        s.enter(a, realm);
        s.instances.get_mut(&realm).unwrap().grant_xp(a, 5000);
        assert!(s.instances[&realm].players[&a].level > 1);
        s.handle_transfer(realm, a, Transfer::Died);
        assert_eq!(s.sessions[&a].instance, s.concourse);
        assert_eq!(s.sessions[&a].level, 1);
        assert_eq!(s.memorial[0].name, "Doomed");
        assert!(s.memorial[0].level > 1);
    }

    #[test]
    fn cathedral_portal_makes_one_shared_instance() {
        let mut s = Server::new();
        let (a, _ra) = join(&mut s, "One");
        let (b, _rb) = join(&mut s, "Two");
        let realm = s.realm;
        s.enter(a, realm); s.enter(b, realm);
        let pos = s.instances[&realm].actor(a).unwrap().pos;
        s.instances.get_mut(&realm).unwrap().portals.push(combat::Portal { id: 777, pos, to: PortalTo::Cathedral, ttl: 40.0, instance: None });
        s.handle_transfer(realm, a, Transfer::Portal(777));
        s.handle_transfer(realm, b, Transfer::Portal(777));
        let ia = s.sessions[&a].instance;
        assert_eq!(ia, s.sessions[&b].instance);
        assert_eq!(s.instances[&ia].kind, WorldKind::Cathedral);
        assert_eq!(s.instances[&ia].player_count(), 2);
    }
}
