//! Realm Eternal browser client.
//!
//! The client sends inputs and renders what the authoritative server says happened. It predicts only
//! its own walking (so movement feels immediate) and smooths everything else between snapshots.

mod draw;
mod gfx;

use realm_shared::{
    combat::{self, cathedral_walkable, State},
    world::{TileMap, WorldKind},
    ActorNet, Camera, ClientMessage, EventNet, FallenNet, MeNet, PortalNet, ProjNet, ServerMessage, Vec2,
    BTN_DODGE, BTN_FLASK, BTN_HEAVY, BTN_HUB, BTN_INTERACT, BTN_JUMP, BTN_JUMP_HELD, BTN_LIGHT, PROTOCOL_VERSION,
};
use std::{cell::RefCell, collections::{HashMap, HashSet, VecDeque}, rc::Rc};
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlImageElement, KeyboardEvent, MessageEvent, MouseEvent, WebSocket, WheelEvent};

pub const M: f32 = combat::PX_PER_M;

pub struct Ent {
    pub net: ActorNet,
    pub pos: Vec2,
    pub z: f32,
    pub walk: f32,
    pub moving: bool,
}

pub struct Float { pub pos: Vec2, pub z: f32, pub text: String, pub color: &'static str, pub t: f32 }
pub struct Ember { pub pos: Vec2, pub z: f32, pub vz: f32, pub life: f32, pub ash: bool }

pub struct Game {
    pub canvas: HtmlCanvasElement,
    pub ctx: CanvasRenderingContext2d,
    keys: HashSet<String>,
    presses: u16,
    pub mouse: Vec2,
    ws: Option<WebSocket>,
    inbox: Rc<RefCell<Vec<String>>>,
    pub status: String,
    pub me: u32,
    pub me_net: MeNet,
    pub world: Option<WorldKind>,
    pub map: Option<TileMap>,
    pub chunks: HashMap<(i32, i32), HtmlCanvasElement>,
    chunk_order: VecDeque<(i32, i32)>,
    pub minimap: Option<HtmlCanvasElement>,
    pub tex: gfx::Textures,
    pub props: HashMap<(u8, u8), gfx::PropSprite>,
    pub warrior: HtmlImageElement,
    pub ents: HashMap<u32, Ent>,
    pub projs: Vec<ProjNet>,
    pub portals: Vec<PortalNet>,
    pub snap_age: f32,
    pub pred: Vec2,
    pred_vel: Vec2,
    have_pred: bool,
    pub camera: Camera,
    pub time: f32,
    pub floats: Vec<Float>,
    pub embers: Vec<Ember>,
    pub hurt: f32,
    pub shake: f32,
    pub banner: Option<(String, f32)>,
    pub help: bool,
    pub died: Option<(String, u32, String)>,
    pub memorial: Vec<FallenNet>,
    pub gate: bool,
    pub mist: bool,
    pub stage: String,
    pub phase2: bool,
    pub online: u32,
    send_acc: f32,
    seq: u32,
    seed: u32,
    name: String,
}

thread_local! { static GAME: RefCell<Option<Rc<RefCell<Game>>>> = const { RefCell::new(None) }; }

#[wasm_bindgen(start)]
pub fn boot() { console_error_panic_hook::set_once(); }

/// Called by the page once the player has chosen a name.
#[wasm_bindgen]
pub fn play(name: String) -> Result<(), JsValue> {
    let window = web_sys::window().unwrap();
    let document = window.document().unwrap();
    let canvas: HtmlCanvasElement = document.get_element_by_id("game").unwrap().dyn_into()?;
    let ctx: CanvasRenderingContext2d = canvas.get_context("2d")?.unwrap().dyn_into()?;
    resize(&canvas);
    let warrior = HtmlImageElement::new()?;
    warrior.set_src("/assets/warrior.png");
    let game = Rc::new(RefCell::new(Game {
        canvas: canvas.clone(), ctx, keys: HashSet::new(), presses: 0, mouse: Vec2::new(640.0, 300.0), ws: None,
        inbox: Rc::new(RefCell::new(Vec::new())), status: "Connecting…".into(), me: 0, me_net: MeNet::default(),
        world: None, map: None, chunks: HashMap::new(), chunk_order: VecDeque::new(), minimap: None,
        tex: gfx::build_textures(), props: gfx::build_props(), warrior, ents: HashMap::new(), projs: Vec::new(),
        portals: Vec::new(), snap_age: 0.0, pred: Vec2::ZERO, pred_vel: Vec2::ZERO, have_pred: false,
        camera: Camera { position: Vec2::ZERO, zoom: 1.3, viewport: viewport(&canvas) }, time: 0.0, floats: Vec::new(),
        embers: Vec::new(), hurt: 0.0, shake: 0.0, banner: None, help: true, died: None, memorial: Vec::new(),
        gate: false, mist: false, stage: String::new(), phase2: false, online: 0, send_acc: 0.0, seq: 0, seed: 777,
        name: name.chars().take(16).collect(),
    }));
    connect(&game);
    install_events(game.clone())?;
    GAME.with(|slot| *slot.borrow_mut() = Some(game.clone()));
    let last = Rc::new(RefCell::new(window.performance().unwrap().now()));
    animation_loop(game, last);
    Ok(())
}

fn connect(game: &Rc<RefCell<Game>>) {
    let loc = web_sys::window().unwrap().location();
    let proto = if loc.protocol().unwrap_or_default() == "https:" { "wss" } else { "ws" };
    let url = format!("{proto}://{}/ws", loc.host().unwrap_or_default());
    let Ok(ws) = WebSocket::new(&url) else { game.borrow_mut().status = "Could not connect".into(); return };
    let inbox = game.borrow().inbox.clone();
    let onmsg = Closure::<dyn FnMut(_)>::new(move |e: MessageEvent| {
        if let Some(t) = e.data().as_string() { inbox.borrow_mut().push(t); }
    });
    ws.set_onmessage(Some(onmsg.as_ref().unchecked_ref()));
    onmsg.forget();
    let g = game.clone();
    let onopen = Closure::<dyn FnMut()>::new(move || {
        let s = g.borrow();
        let hello = ClientMessage::Hello { version: PROTOCOL_VERSION, name: s.name.clone() };
        if let (Some(ws), Ok(text)) = (&s.ws, serde_json::to_string(&hello)) { ws.send_with_str(&text).ok(); }
    });
    ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();
    let g = game.clone();
    let onclose = Closure::<dyn FnMut()>::new(move || { g.borrow_mut().status = "Disconnected — refresh to rejoin".into(); });
    ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
    onclose.forget();
    game.borrow_mut().ws = Some(ws);
}

fn viewport(canvas: &HtmlCanvasElement) -> Vec2 { Vec2::new(canvas.width() as f32, canvas.height() as f32) }

fn resize(canvas: &HtmlCanvasElement) {
    let window = web_sys::window().unwrap();
    let dpr = window.device_pixel_ratio().min(2.0);
    let w = window.inner_width().unwrap().as_f64().unwrap();
    let h = window.inner_height().unwrap().as_f64().unwrap();
    if canvas.width() != (w * dpr) as u32 || canvas.height() != (h * dpr) as u32 {
        canvas.set_width((w * dpr) as u32);
        canvas.set_height((h * dpr) as u32);
        canvas.style().set_property("width", &format!("{w}px")).ok();
        canvas.style().set_property("height", &format!("{h}px")).ok();
    }
}

fn install_events(game: Rc<RefCell<Game>>) -> Result<(), JsValue> {
    let window = web_sys::window().unwrap();
    let g = game.clone();
    let keydown = Closure::<dyn FnMut(_)>::new(move |e: KeyboardEvent| {
        let k = e.key().to_lowercase();
        if !matches!(k.as_str(), "f12" | "f11" | "f5") { e.prevent_default(); }
        let mut s = g.borrow_mut();
        if s.died.is_some() && !e.repeat() { s.died = None; return; }
        if !e.repeat() {
            match k.as_str() {
                "j" => s.presses |= BTN_LIGHT,
                "k" => s.presses |= BTN_HEAVY,
                "shift" => s.presses |= BTN_DODGE,
                " " => s.presses |= BTN_JUMP,
                "q" => s.presses |= BTN_FLASK,
                "e" => s.presses |= BTN_INTERACT,
                "r" => s.presses |= BTN_HUB,
                "h" | "f1" => s.help = !s.help,
                _ => {}
            }
        }
        s.keys.insert(k);
    });
    window.add_event_listener_with_callback("keydown", keydown.as_ref().unchecked_ref())?;
    keydown.forget();
    let g = game.clone();
    let keyup = Closure::<dyn FnMut(_)>::new(move |e: KeyboardEvent| { g.borrow_mut().keys.remove(&e.key().to_lowercase()); });
    window.add_event_listener_with_callback("keyup", keyup.as_ref().unchecked_ref())?;
    keyup.forget();
    let g = game.clone();
    let blur = Closure::<dyn FnMut()>::new(move || { g.borrow_mut().keys.clear(); });
    window.add_event_listener_with_callback("blur", blur.as_ref().unchecked_ref())?;
    blur.forget();
    let g = game.clone();
    let mousemove = Closure::<dyn FnMut(_)>::new(move |e: MouseEvent| {
        let rect = g.borrow().canvas.get_bounding_client_rect();
        let mut s = g.borrow_mut();
        let (cw, ch) = (s.canvas.width() as f32, s.canvas.height() as f32);
        s.mouse = Vec2::new(
            (e.client_x() as f64 - rect.left()) as f32 * cw / rect.width() as f32,
            (e.client_y() as f64 - rect.top()) as f32 * ch / rect.height() as f32,
        );
    });
    window.add_event_listener_with_callback("mousemove", mousemove.as_ref().unchecked_ref())?;
    mousemove.forget();
    let g = game.clone();
    let down = Closure::<dyn FnMut(_)>::new(move |e: MouseEvent| {
        let mut s = g.borrow_mut();
        if s.died.is_some() { s.died = None; return; }
        match e.button() { 0 => s.presses |= BTN_LIGHT, 2 => s.presses |= BTN_HEAVY, _ => {} }
    });
    window.add_event_listener_with_callback("mousedown", down.as_ref().unchecked_ref())?;
    down.forget();
    let menu = Closure::<dyn FnMut(_)>::new(move |e: MouseEvent| e.prevent_default());
    window.add_event_listener_with_callback("contextmenu", menu.as_ref().unchecked_ref())?;
    menu.forget();
    let g = game;
    let wheel = Closure::<dyn FnMut(_)>::new(move |e: WheelEvent| {
        e.prevent_default();
        let mut s = g.borrow_mut();
        s.camera.zoom = (s.camera.zoom - e.delta_y() as f32 * 0.0006).clamp(0.8, 2.2);
    });
    window.add_event_listener_with_callback("wheel", wheel.as_ref().unchecked_ref())?;
    wheel.forget();
    Ok(())
}

fn animation_loop(game: Rc<RefCell<Game>>, last: Rc<RefCell<f64>>) {
    let f = Rc::new(RefCell::new(None::<Closure<dyn FnMut(f64)>>));
    let f2 = f.clone();
    *f2.borrow_mut() = Some(Closure::new(move |now: f64| {
        let dt = (((now - *last.borrow()) / 1000.0) as f32).min(0.1);
        *last.borrow_mut() = now;
        {
            let mut s = game.borrow_mut();
            resize(&s.canvas);
            update(&mut s, dt);
            draw::render(&mut s);
        }
        web_sys::window().unwrap().request_animation_frame(f.borrow().as_ref().unwrap().as_ref().unchecked_ref()).unwrap();
    }));
    web_sys::window().unwrap().request_animation_frame(f2.borrow().as_ref().unwrap().as_ref().unchecked_ref()).unwrap();
}

fn held(g: &Game, keys: &[&str]) -> bool { keys.iter().any(|k| g.keys.contains(*k)) }

pub fn rnd(g: &mut Game) -> f32 {
    let mut x = g.seed; x ^= x << 13; x ^= x >> 17; x ^= x << 5; g.seed = x;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

impl Game {
    pub fn my_ent(&self) -> Option<&Ent> { self.ents.get(&self.me) }
    pub fn walkable(&self, p: Vec2, r: f32) -> bool {
        match (self.world, &self.map) {
            (Some(WorldKind::Cathedral), _) => cathedral_walkable(p, r, true, self.gate, self.mist),
            (_, Some(m)) => m.walkable(p, r),
            _ => true,
        }
    }
    /// Lazily bake terrain chunks around the camera, a couple per frame, and keep a bounded cache.
    pub fn ensure_chunks(&mut self, want: &[(i32, i32)]) {
        let mut baked = 0;
        for &k in want {
            if self.chunks.contains_key(&k) { continue; }
            if baked >= 2 { break; }
            if let Some(map) = &self.map {
                let c = gfx::bake_chunk(map, &self.tex, k.0, k.1);
                self.chunks.insert(k, c);
                self.chunk_order.push_back(k);
                baked += 1;
            }
        }
        let mut guard = 0;
        while self.chunk_order.len() > 90 && guard < 200 {
            guard += 1;
            if let Some(old) = self.chunk_order.pop_front() {
                if want.contains(&old) { self.chunk_order.push_back(old); } else { self.chunks.remove(&old); }
            }
        }
    }
}

fn handle(g: &mut Game, text: &str) {
    let Ok(msg) = serde_json::from_str::<ServerMessage>(text) else { return };
    match msg {
        ServerMessage::Welcome { id, .. } => { g.me = id; g.status.clear(); }
        ServerMessage::Enter { world, seed, .. } => {
            g.world = Some(world);
            g.map = if world == WorldKind::Cathedral { None } else { Some(TileMap::generate(world, seed)) };
            g.minimap = if world == WorldKind::Realm { g.map.as_ref().map(gfx::bake_minimap) } else { None };
            g.chunks.clear(); g.chunk_order.clear();
            g.ents.clear(); g.projs.clear(); g.portals.clear();
            g.have_pred = false;
            g.floats.clear();
        }
        ServerMessage::Snap(snap) => {
            g.snap_age = 0.0;
            g.me_net = snap.me;
            g.gate = snap.gate; g.mist = snap.mist; g.stage = snap.stage; g.phase2 = snap.phase2; g.online = snap.online;
            let mut seen = HashSet::new();
            for a in snap.actors {
                seen.insert(a.u);
                let p = Vec2::new(a.x, a.y);
                match g.ents.get_mut(&a.u) {
                    Some(e) => { e.net = a; }
                    None => { let z = a.z; g.ents.insert(a.u, Ent { net: a, pos: p, z, walk: 0.0, moving: false }); }
                }
            }
            g.ents.retain(|k, _| seen.contains(k));
            g.projs = snap.projs;
            g.portals = snap.portals;
            for ev in snap.events {
                match ev {
                    EventNet::Dmg { x, y, z, v, p } => g.floats.push(Float { pos: Vec2::new(x, y), z, text: if p { format!("-{v}") } else { v.to_string() }, color: if p { "#ff5a5a" } else { "#f3e9d2" }, t: 0.0 }),
                    EventNet::Hurt => g.hurt = 1.0,
                    EventNet::Msg { text, secs } => g.banner = Some((text, secs)),
                    EventNet::Shake { a } => g.shake = g.shake.max(a),
                    EventNet::Stagger { x, y } => g.floats.push(Float { pos: Vec2::new(x, y), z: 2.4, text: "BROKEN".into(), color: "#ffc857", t: 0.0 }),
                    EventNet::Level { lv } => { let p = g.pred; g.floats.push(Float { pos: p, z: 2.6, text: format!("LEVEL {lv}!"), color: "#ffe08a", t: 0.0 }) }
                }
            }
        }
        ServerMessage::Died { name, level, place } => { g.died = Some((name, level, place)); }
        ServerMessage::Memorial { fallen } => g.memorial = fallen,
        ServerMessage::Error { message } => g.status = message,
        ServerMessage::Pong { .. } => {}
    }
}

fn update(g: &mut Game, dt: f32) {
    g.time += dt;
    g.snap_age += dt;
    g.camera.viewport = viewport(&g.canvas);
    let msgs: Vec<String> = std::mem::take(&mut *g.inbox.borrow_mut());
    for m in msgs { handle(g, &m); }

    let x = held(g, &["d", "arrowright"]) as i32 as f32 - held(g, &["a", "arrowleft"]) as i32 as f32;
    let y = held(g, &["s", "arrowdown"]) as i32 as f32 - held(g, &["w", "arrowup"]) as i32 as f32;
    let mv = Camera::from_view(Vec2::new(x, y)).normalized();
    let aim = g.camera.unproject_ground(g.mouse) * (1.0 / M);

    // Smooth remote actors toward the server's latest position (extrapolated by velocity).
    let age = g.snap_age.min(0.15);
    let me = g.me;
    for (uid, e) in g.ents.iter_mut() {
        if *uid == me { continue; }
        let target = Vec2::new(e.net.x + e.net.vx * age, e.net.y + e.net.vy * age);
        let before = e.pos;
        if e.pos.distance(target) > 4.0 { e.pos = target; } else { e.pos = e.pos.lerp(target, 1.0 - (-14.0 * dt).exp()); }
        e.z += (e.net.z - e.z) * (1.0 - (-20.0 * dt).exp());
        let moved = e.pos.distance(before);
        e.moving = moved > 0.4 * dt;
        e.walk += moved;
    }

    // Predict our own walking; follow the server for everything it controls (attacks, rolls, knockback).
    let mine = g.ents.get(&me).map(|e| (Vec2::new(e.net.x + e.net.vx * age, e.net.y + e.net.vy * age), State::from_u8(e.net.s), e.net.z, Vec2::new(e.net.vx, e.net.vy)));
    if let Some((server, state, net_z, net_vel)) = mine {
        let speed = 4.2 * if state == State::Heal { 0.35 } else { 1.0 };
        if !g.have_pred { g.pred = server; g.pred_vel = Vec2::ZERO; g.have_pred = true; }
        let before = g.pred;
        if matches!(state, State::Idle | State::Heal) && g.died.is_none() {
            let want = mv * speed;
            let accel = if want.length() > 0.01 { 32.0 } else { 40.0 };
            let diff = want - g.pred_vel;
            let step = accel * dt;
            g.pred_vel = if diff.length() <= step { want } else { g.pred_vel + diff.normalized() * step };
            let d = g.pred_vel * dt;
            let mut np = g.pred + d;
            if !g.walkable(np, 0.38) {
                let nx = Vec2::new(g.pred.x + d.x, g.pred.y);
                let ny = Vec2::new(g.pred.x, g.pred.y + d.y);
                np = if g.walkable(nx, 0.38) { nx } else if g.walkable(ny, 0.38) { ny } else { g.pred };
            }
            g.pred = np;
            let err = g.pred.distance(server);
            if err > 2.0 { g.pred = server; } else { g.pred = g.pred.lerp(server, ((1.0 - (-3.0 * dt).exp()) * (0.4 + err)).min(1.0)); }
        } else {
            g.pred_vel = net_vel;
            g.pred = g.pred.lerp(server, 1.0 - (-18.0 * dt).exp());
        }
        let moved = g.pred.distance(before);
        let pred = g.pred;
        if let Some(e) = g.ents.get_mut(&me) {
            e.pos = pred;
            e.z += (net_z - e.z) * (1.0 - (-25.0 * dt).exp());
            e.moving = moved > 0.4 * dt;
            e.walk += moved;
        }
    }

    // Send inputs at ~30 Hz; button presses accumulate until sent.
    g.send_acc += dt;
    if g.send_acc >= 1.0 / 30.0 {
        g.send_acc = 0.0;
        let mut b = g.presses;
        if held(g, &[" "]) { b |= BTN_JUMP_HELD; }
        g.presses = 0;
        g.seq = g.seq.wrapping_add(1);
        let blocked = g.died.is_some();
        let msg = ClientMessage::Input { seq: g.seq, mx: if blocked { 0.0 } else { mv.x }, my: if blocked { 0.0 } else { mv.y }, ax: aim.x, ay: aim.y, b: if blocked { 0 } else { b } };
        if let (Some(ws), Ok(text)) = (&g.ws, serde_json::to_string(&msg)) {
            if ws.ready_state() == WebSocket::OPEN { ws.send_with_str(&text).ok(); }
        }
    }

    for f in &mut g.floats { f.t += dt; }
    g.floats.retain(|f| f.t < 1.1);
    for p in &mut g.projs { p.x += p.dx * dt; p.y += p.dy * dt; }
    if let Some((_, t)) = &mut g.banner { *t -= dt; if *t <= 0.0 { g.banner = None; } }
    g.hurt = (g.hurt - dt * 2.2).max(0.0);
    g.shake = (g.shake - dt * 3.0).max(0.0);

    // Ambient particles: embers in the cathedral and the high bands, ash on the wind.
    let p = g.pred;
    let fiery = matches!(g.world, Some(WorldKind::Cathedral)) || g.map.as_ref().map_or(false, |m| m.kind == WorldKind::Realm && m.band_at(p) >= 3);
    if fiery && g.embers.len() < 90 {
        let (a, b, c) = (rnd(g), rnd(g), rnd(g));
        let ash = c < 0.4;
        g.embers.push(Ember { pos: p + Vec2::new((a - 0.5) * 30.0, (b - 0.5) * 22.0), z: if ash { 6.0 } else { 0.2 }, vz: if ash { -0.5 } else { 0.8 + c }, life: 5.0, ash });
    }
    if !fiery { g.embers.clear(); }
    for e in &mut g.embers { e.z += e.vz * dt; e.pos.x += (g.time * 0.7 + e.pos.y).sin() * 0.3 * dt; e.life -= dt; }
    g.embers.retain(|e| e.life > 0.0 && e.z > -0.1 && e.z < 8.0);

    // Asymmetric framing: the camera leads toward the aim so there's more room ahead of you.
    let lead_dir = aim - p;
    let lead = lead_dir.normalized() * lead_dir.length().min(9.0) * 0.28;
    let target = (p + lead) * M;
    if g.camera.position.distance(target) > 600.0 { g.camera.position = target; }
    g.camera.position = g.camera.position.lerp(target, 1.0 - (-6.0 * dt).exp());
}
