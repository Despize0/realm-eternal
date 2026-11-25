//! Realm Eternal browser client: input, fixed-step simulation driver and Canvas2D renderer.
//! The combat rules live in `realm_shared::combat` (ported from Ashen Vigil).

use realm_shared::combat::{
    self, Actor, Area, Event, Input, Kind, Phase, Sim, Stage, State, BELFRY_ALTAR, BELFRY_CENTER, BELFRY_PILLARS,
    BELFRY_R, CORRIDOR_END_Y, GATE_HALF_W, NAVE_ALTAR, NAVE_MAX, NAVE_MIN, PILLARS, PILLAR_R, PX_PER_M, TICK,
};
use realm_shared::{Camera, Vec2};
use std::{cell::RefCell, collections::HashSet, f64::consts::TAU, rc::Rc};
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, KeyboardEvent, MouseEvent, WheelEvent};

const M: f32 = PX_PER_M;

struct Float { pos: Vec2, z: f32, text: String, color: &'static str, t: f32 }
struct Ember { pos: Vec2, z: f32, vz: f32, life: f32, ash: bool }

#[derive(Default)]
struct Pressed { light: bool, heavy: bool, dodge: bool, jump: bool, flask: bool, rest: bool }

struct Game {
    canvas: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
    keys: HashSet<String>,
    pressed: Pressed,
    mouse: Vec2,
    sim: Sim,
    camera: Camera,
    acc: f32,
    time: f32,
    floats: Vec<Float>,
    embers: Vec<Ember>,
    hurt: f32,
    shake: f32,
    seed: u32,
    help: bool,
}

thread_local! { static GAME: RefCell<Option<Rc<RefCell<Game>>>> = const { RefCell::new(None) }; }

#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    let window = web_sys::window().unwrap();
    let document = window.document().unwrap();
    let canvas: HtmlCanvasElement = document.get_element_by_id("game").unwrap().dyn_into()?;
    let ctx: CanvasRenderingContext2d = canvas.get_context("2d")?.unwrap().dyn_into()?;
    resize(&canvas);
    let sim = Sim::new();
    let ppos = sim.player().pos * M;
    let camera = Camera { position: ppos, zoom: 1.25, viewport: viewport(&canvas) };
    let game = Rc::new(RefCell::new(Game {
        canvas: canvas.clone(), ctx, keys: HashSet::new(), pressed: Pressed::default(),
        mouse: Vec2::new(640.0, 300.0), sim, camera, acc: 0.0, time: 0.0, floats: Vec::new(),
        embers: Vec::new(), hurt: 0.0, shake: 0.0, seed: 12345, help: true,
    }));
    install_events(game.clone())?;
    GAME.with(|slot| *slot.borrow_mut() = Some(game.clone()));
    let last = Rc::new(RefCell::new(window.performance().unwrap().now()));
    animation_loop(game, last);
    Ok(())
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
        if k != "f12" && k != "f11" { e.prevent_default(); }
        let mut s = g.borrow_mut();
        if !e.repeat() {
            match k.as_str() {
                "j" => s.pressed.light = true,
                "k" => s.pressed.heavy = true,
                "shift" => s.pressed.dodge = true,
                " " => s.pressed.jump = true,
                "q" => s.pressed.flask = true,
                "e" => s.pressed.rest = true,
                "f1" | "h" => s.help = !s.help,
                "f5" => s.sim.restart(),
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
        match e.button() { 0 => s.pressed.light = true, 2 => s.pressed.heavy = true, _ => {} }
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
        s.camera.zoom = (s.camera.zoom - e.delta_y() as f32 * 0.0006).clamp(0.8, 2.0);
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
            render(&s);
        }
        web_sys::window().unwrap().request_animation_frame(f.borrow().as_ref().unwrap().as_ref().unchecked_ref()).unwrap();
    }));
    web_sys::window().unwrap().request_animation_frame(f2.borrow().as_ref().unwrap().as_ref().unchecked_ref()).unwrap();
}

fn held(g: &Game, keys: &[&str]) -> bool { keys.iter().any(|k| g.keys.contains(*k)) }

fn rnd(g: &mut Game) -> f32 {
    let mut x = g.seed; x ^= x << 13; x ^= x >> 17; x ^= x << 5; g.seed = x;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

fn update(g: &mut Game, dt: f32) {
    g.time += dt;
    g.camera.viewport = viewport(&g.canvas);

    let x = held(g, &["d", "arrowright"]) as i32 as f32 - held(g, &["a", "arrowleft"]) as i32 as f32;
    let y = held(g, &["s", "arrowdown"]) as i32 as f32 - held(g, &["w", "arrowup"]) as i32 as f32;
    let aim = g.camera.unproject_ground(g.mouse) * (1.0 / M);

    g.acc += dt;
    let mut first = true;
    while g.acc >= TICK {
        g.acc -= TICK;
        let input = Input {
            mv: Vec2::new(x, y), aim,
            light: first && g.pressed.light, heavy: first && g.pressed.heavy, dodge: first && g.pressed.dodge,
            jump: first && g.pressed.jump, jump_held: held(g, &[" "]), flask: first && g.pressed.flask, rest: first && g.pressed.rest,
        };
        if first { g.pressed = Pressed::default(); }
        first = false;
        g.sim.step(&input);
    }

    for ev in std::mem::take(&mut g.sim.events) {
        match ev {
            Event::Damage { pos, z, amount, to_player } => g.floats.push(Float {
                pos, z, text: if to_player { format!("-{amount}") } else { amount.to_string() },
                color: if to_player { "#ff5a5a" } else { "#f3e9d2" }, t: 0.0,
            }),
            Event::Hurt => g.hurt = 1.0,
            Event::Shake(s) => g.shake = g.shake.max(s),
            Event::Stagger { pos } => g.floats.push(Float { pos, z: 2.4, text: "BROKEN".into(), color: "#ffc857", t: 0.0 }),
            _ => {}
        }
    }
    for f in &mut g.floats { f.t += dt; }
    g.floats.retain(|f| f.t < 1.1);
    g.hurt = (g.hurt - dt * 2.2).max(0.0);
    g.shake = (g.shake - dt * 3.0).max(0.0);

    // Embers drift up from the braziers, ash drifts down.
    let p = g.sim.player().pos;
    if g.embers.len() < 90 {
        let (a, b, c) = (rnd(g), rnd(g), rnd(g));
        let ash = c < 0.4;
        g.embers.push(Ember { pos: p + Vec2::new((a - 0.5) * 28.0, (b - 0.5) * 20.0), z: if ash { 6.0 } else { 0.2 }, vz: if ash { -0.5 } else { 0.8 + c }, life: 5.0, ash });
    }
    for e in &mut g.embers { e.z += e.vz * dt; e.pos.x += (g.time * 0.7 + e.pos.y).sin() * 0.3 * dt; e.life -= dt; }
    g.embers.retain(|e| e.life > 0.0 && e.z > -0.1 && e.z < 8.0);

    // Camera follows the knight with a slight lead toward the cursor.
    let lead = (aim - p).normalized() * 1.2;
    let target = (p + lead) * M;
    g.camera.position = g.camera.position.lerp(target, 1.0 - (-6.0 * dt).exp());
}

// ------------------------------------------------------------ drawing ----

fn cam(g: &Game) -> Camera {
    let mut c = g.camera;
    if g.shake > 0.0 {
        let s = g.shake * 6.0;
        c.position += Vec2::new((g.time * 91.0).sin() * s, (g.time * 73.0).cos() * s);
    }
    c
}

fn proj(c: &Camera, p: Vec2, z: f32) -> (f64, f64) { let s = c.project(p * M, z * M); (s.x as f64, s.y as f64) }

fn poly(ctx: &CanvasRenderingContext2d, c: &Camera, pts: &[Vec2], z: f32) {
    ctx.begin_path();
    for (i, &p) in pts.iter().enumerate() {
        let (x, y) = proj(c, p, z);
        if i == 0 { ctx.move_to(x, y) } else { ctx.line_to(x, y) }
    }
    ctx.close_path();
}

fn render(g: &Game) {
    let c = &g.ctx;
    let cm = cam(g);
    let (w, h) = (g.canvas.width() as f64, g.canvas.height() as f64);
    c.set_fill_style_str("#07080b");
    c.fill_rect(0.0, 0.0, w, h);

    draw_floor(g, &cm);
    draw_altar(g, &cm, NAVE_ALTAR);
    draw_altar(g, &cm, BELFRY_ALTAR);
    draw_telegraphs(g, &cm);

    // Depth-sorted actors, pillars and braziers.
    enum Item<'a> { Actor(&'a Actor), Pillar(Vec2), Brazier(Vec2) }
    let mut items: Vec<(f32, Item)> = Vec::new();
    for a in &g.sim.actors { items.push((a.pos.y, Item::Actor(a))); }
    for &p in PILLARS.iter().chain(BELFRY_PILLARS.iter()) { items.push((p.y, Item::Pillar(p))); }
    for &b in braziers().iter() { items.push((b.y, Item::Brazier(b))); }
    items.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, it) in &items {
        match it {
            Item::Actor(a) => draw_actor(g, &cm, a),
            Item::Pillar(p) => draw_pillar(g, &cm, *p),
            Item::Brazier(b) => draw_brazier(g, &cm, *b),
        }
    }
    draw_front_wall(g, &cm);
    draw_projectiles(g, &cm);
    for e in &g.embers {
        let (x, y) = proj(&cm, e.pos, e.z);
        c.set_fill_style_str(if e.ash { "rgba(170,160,150,0.35)" } else { "rgba(255,150,60,0.8)" });
        c.fill_rect(x, y, 2.0, 2.0);
    }
    draw_overheads(g, &cm);
    draw_vignette(g);
    draw_hud(g);
}

fn braziers() -> [Vec2; 6] {
    [Vec2::new(-9.5, -6.5), Vec2::new(9.5, -6.5), Vec2::new(-9.5, 6.5), Vec2::new(9.5, 6.5),
     BELFRY_CENTER + Vec2::new(-6.3, 1.5), BELFRY_CENTER + Vec2::new(6.3, 1.5)]
}

fn flicker(g: &Game, seed: f32) -> f64 { (0.85 + 0.15 * ((g.time * 9.0 + seed).sin() * (g.time * 13.7 + seed * 2.0).sin())) as f64 }

fn draw_floor(g: &Game, cm: &Camera) {
    let c = &g.ctx;
    // Nave flagstones.
    let mut y = NAVE_MIN.y;
    let mut row = 0;
    while y < NAVE_MAX.y {
        let mut x = NAVE_MIN.x + if row % 2 == 0 { 0.0 } else { -0.6 };
        let mut col = 0;
        while x < NAVE_MAX.x {
            let x0 = x.max(NAVE_MIN.x); let x1 = (x + 1.2).min(NAVE_MAX.x);
            let shade = ((col * 7 + row * 13) % 5) as i32;
            let base = 34 + shade * 3;
            c.set_fill_style_str(&format!("rgb({},{},{})", base, base + 3, base + 2));
            poly(c, cm, &[Vec2::new(x0, y), Vec2::new(x1, y), Vec2::new(x1, y + 1.2), Vec2::new(x0, y + 1.2)], 0.0);
            c.fill();
            c.set_stroke_style_str("rgba(8,10,10,0.7)");
            c.set_line_width(1.5);
            c.stroke();
            x += 1.2; col += 1;
        }
        y += 1.2; row += 1;
    }
    // Worn path down the nave.
    c.set_fill_style_str("rgba(70,60,48,0.18)");
    poly(c, cm, &[Vec2::new(-1.6, NAVE_MIN.y), Vec2::new(1.6, NAVE_MIN.y), Vec2::new(1.6, NAVE_MAX.y), Vec2::new(-1.6, NAVE_MAX.y)], 0.0);
    c.fill();

    // Corridor through the gate.
    c.set_fill_style_str("#24272a");
    poly(c, cm, &[Vec2::new(-GATE_HALF_W, CORRIDOR_END_Y - 0.6), Vec2::new(GATE_HALF_W, CORRIDOR_END_Y - 0.6), Vec2::new(GATE_HALF_W, NAVE_MIN.y), Vec2::new(-GATE_HALF_W, NAVE_MIN.y)], 0.0);
    c.fill();

    // Belfry: an octagon.
    let oct: Vec<Vec2> = (0..8).map(|i| {
        let a = (i as f32 + 0.5) / 8.0 * std::f32::consts::TAU;
        BELFRY_CENTER + Vec2::new(a.cos(), a.sin()) * (BELFRY_R + 0.4)
    }).collect();
    c.set_fill_style_str("#2a2624");
    poly(c, cm, &oct, 0.0);
    c.fill();
    for r in [2.0f32, 4.5, 6.8] {
        c.set_stroke_style_str("rgba(10,8,8,0.6)");
        c.set_line_width(2.0);
        c.begin_path();
        for i in 0..=32 {
            let a = i as f32 / 32.0 * std::f32::consts::TAU;
            let (x, y) = proj(cm, BELFRY_CENTER + Vec2::new(a.cos(), a.sin()) * r, 0.0);
            if i == 0 { c.move_to(x, y) } else { c.line_to(x, y) }
        }
        c.stroke();
    }

    // Back walls (drawn behind everything): nave rear wall with the gate, belfry rim.
    let wall_h = 2.6;
    let rear = [(NAVE_MIN.x, -GATE_HALF_W), (GATE_HALF_W, NAVE_MAX.x)];
    for (a, b) in rear {
        draw_wall_face(g, cm, Vec2::new(a, NAVE_MIN.y), Vec2::new(b, NAVE_MIN.y), wall_h, "#3a3b3d");
    }
    draw_wall_face(g, cm, Vec2::new(NAVE_MIN.x, NAVE_MIN.y), Vec2::new(NAVE_MIN.x, NAVE_MAX.y), wall_h, "#2e2f31");
    draw_wall_face(g, cm, Vec2::new(NAVE_MAX.x, NAVE_MIN.y), Vec2::new(NAVE_MAX.x, NAVE_MAX.y), wall_h, "#2e2f31");
    // Portcullis.
    if !g.sim.gate_open {
        c.set_stroke_style_str("#6b5a44");
        c.set_line_width(3.0);
        let mut x = -GATE_HALF_W + 0.2;
        while x < GATE_HALF_W {
            let (x0, y0) = proj(cm, Vec2::new(x, NAVE_MIN.y), 0.0);
            let (x1, y1) = proj(cm, Vec2::new(x, NAVE_MIN.y), wall_h);
            c.begin_path(); c.move_to(x0, y0); c.line_to(x1, y1); c.stroke();
            x += 0.4;
        }
    }
    // Belfry rim (rear half).
    for i in 0..8 {
        let a0 = (i as f32 + 0.5) / 8.0 * std::f32::consts::TAU;
        let a1 = (i as f32 + 1.5) / 8.0 * std::f32::consts::TAU;
        let p0 = BELFRY_CENTER + Vec2::new(a0.cos(), a0.sin()) * (BELFRY_R + 0.4);
        let p1 = BELFRY_CENTER + Vec2::new(a1.cos(), a1.sin()) * (BELFRY_R + 0.4);
        if (p0.y + p1.y) * 0.5 < BELFRY_CENTER.y { draw_wall_face(g, cm, p0, p1, 2.0, "#34302d"); }
    }
    // Mist wall at the belfry threshold.
    if g.sim.mist {
        let y = CORRIDOR_END_Y + 0.5;
        for k in 0..6 {
            let a = 0.12 + 0.05 * ((g.time * 1.5 + k as f32).sin() as f64);
            c.set_fill_style_str(&format!("rgba(200,210,220,{a:.3})"));
            poly(c, cm, &[Vec2::new(-GATE_HALF_W - 0.4, y), Vec2::new(GATE_HALF_W + 0.4, y), Vec2::new(GATE_HALF_W + 0.4, y + 0.3)], 0.4 + k as f32 * 0.4);
            c.fill();
            poly(c, cm, &[Vec2::new(-GATE_HALF_W - 0.4, y), Vec2::new(GATE_HALF_W + 0.4, y), Vec2::new(GATE_HALF_W + 0.4, y + 0.01), Vec2::new(-GATE_HALF_W - 0.4, y + 0.01)], 0.0);
        }
        let (x0, y0) = proj(cm, Vec2::new(-GATE_HALF_W, y), 0.0);
        let (x1, y1) = proj(cm, Vec2::new(GATE_HALF_W, y), 3.0);
        c.set_fill_style_str("rgba(190,200,215,0.22)");
        c.fill_rect(x0, y1, x1 - x0, y0 - y1);
    }

    // Warm light pools under braziers.
    for (i, b) in braziers().iter().enumerate() {
        let (x, y) = proj(cm, *b, 0.0);
        let r = 150.0 * cm.zoom as f64 * flicker(g, i as f32);
        if let Ok(grad) = c.create_radial_gradient(x, y, 4.0, x, y, r) {
            grad.add_color_stop(0.0, "rgba(255,150,70,0.30)").ok();
            grad.add_color_stop(1.0, "rgba(255,120,40,0)").ok();
            c.set_fill_style_canvas_gradient(&grad);
            c.fill_rect(x - r, y - r, r * 2.0, r * 2.0);
        }
    }
}

fn draw_wall_face(g: &Game, cm: &Camera, a: Vec2, b: Vec2, h: f32, color: &str) {
    let c = &g.ctx;
    let (ax, ay) = proj(cm, a, 0.0); let (bx, by) = proj(cm, b, 0.0);
    let (ax2, ay2) = proj(cm, a, h); let (bx2, by2) = proj(cm, b, h);
    c.set_fill_style_str(color);
    c.begin_path(); c.move_to(ax, ay); c.line_to(bx, by); c.line_to(bx2, by2); c.line_to(ax2, ay2); c.close_path(); c.fill();
    // Coursed masonry lines.
    c.set_stroke_style_str("rgba(0,0,0,0.35)");
    c.set_line_width(1.0);
    for k in 1..5 {
        let t = k as f64 / 5.0;
        c.begin_path();
        c.move_to(ax + (ax2 - ax) * t, ay + (ay2 - ay) * t);
        c.line_to(bx + (bx2 - bx) * t, by + (by2 - by) * t);
        c.stroke();
    }
    c.set_fill_style_str("rgba(255,255,255,0.05)");
    c.begin_path(); c.move_to(ax2, ay2); c.line_to(bx2, by2); c.line_to(bx2, by2 + 3.0); c.line_to(ax2, ay2 + 3.0); c.close_path(); c.fill();
}

fn draw_front_wall(g: &Game, cm: &Camera) {
    // Low, broken foreground wall so it never hides a fighter.
    let mut x = NAVE_MIN.x;
    let mut k = 0;
    while x < NAVE_MAX.x {
        let h = [0.35f32, 0.6, 0.25, 0.5, 0.4][k % 5];
        draw_wall_face(g, cm, Vec2::new(x, NAVE_MAX.y), Vec2::new((x + 2.2).min(NAVE_MAX.x), NAVE_MAX.y), h, "#2a2b2d");
        x += 2.2; k += 1;
    }
}

fn draw_altar(g: &Game, cm: &Camera, at: Vec2) {
    let c = &g.ctx;
    let (x, y) = proj(cm, at, 0.0);
    let (_, top) = proj(cm, at, 1.0);
    let s = cm.zoom as f64;
    let r = 70.0 * s * flicker(g, at.y);
    if let Ok(grad) = c.create_radial_gradient(x, y, 2.0, x, y, r) {
        grad.add_color_stop(0.0, "rgba(255,190,110,0.35)").ok();
        grad.add_color_stop(1.0, "rgba(255,160,80,0)").ok();
        c.set_fill_style_canvas_gradient(&grad);
        c.fill_rect(x - r, y - r, r * 2.0, r * 2.0);
    }
    c.set_fill_style_str("#4a4540");
    c.fill_rect(x - 18.0 * s, top, 36.0 * s, y - top);
    c.set_fill_style_str("#5c564f");
    c.fill_rect(x - 22.0 * s, top - 5.0 * s, 44.0 * s, 7.0 * s);
    // Sword planted in the altar.
    c.set_stroke_style_str("#c8c2b4"); c.set_line_width(2.5 * s);
    c.begin_path(); c.move_to(x, top - 4.0 * s); c.line_to(x, top - 34.0 * s); c.stroke();
    c.begin_path(); c.move_to(x - 7.0 * s, top - 26.0 * s); c.line_to(x + 7.0 * s, top - 26.0 * s); c.stroke();
    c.set_fill_style_str(&format!("rgba(255,170,80,{:.2})", 0.7 * flicker(g, at.x + 3.0)));
    c.begin_path(); c.arc(x, top - 6.0 * s, 4.0 * s, 0.0, TAU).ok(); c.fill();
}

fn draw_pillar(g: &Game, cm: &Camera, p: Vec2) {
    let c = &g.ctx;
    let s = cm.zoom as f64;
    let (x, y) = proj(cm, p, 0.0);
    let (_, top) = proj(cm, p, 4.2);
    let r = PILLAR_R as f64 * M as f64 * s;
    c.set_fill_style_str("rgba(0,0,0,0.4)");
    c.begin_path(); c.ellipse(x, y, r * 1.5, r * 0.6, 0.0, 0.0, TAU).ok(); c.fill();
    c.set_fill_style_str("#46484a");
    c.fill_rect(x - r * 1.3, y - 8.0 * s, r * 2.6, 8.0 * s);
    let grad = c.create_linear_gradient(x - r, 0.0, x + r, 0.0);
    grad.add_color_stop(0.0, "#2c2e30").ok();
    grad.add_color_stop(0.45, "#55585a").ok();
    grad.add_color_stop(1.0, "#26282a").ok();
    c.set_fill_style_canvas_gradient(&grad);
    c.fill_rect(x - r, top, r * 2.0, y - 8.0 * s - top);
    c.set_fill_style_str("#4d4f51");
    c.fill_rect(x - r * 1.4, top - 6.0 * s, r * 2.8, 8.0 * s);
}

fn draw_brazier(g: &Game, cm: &Camera, b: Vec2) {
    let c = &g.ctx;
    let s = cm.zoom as f64;
    let (x, y) = proj(cm, b, 0.0);
    let (_, top) = proj(cm, b, 1.0);
    c.set_stroke_style_str("#3b3229"); c.set_line_width(3.0 * s);
    c.begin_path(); c.move_to(x - 8.0 * s, y); c.line_to(x, top); c.line_to(x + 8.0 * s, y); c.stroke();
    c.set_fill_style_str("#2a221c");
    c.fill_rect(x - 12.0 * s, top - 4.0 * s, 24.0 * s, 7.0 * s);
    let f = flicker(g, b.x + b.y);
    for k in 0..3 {
        let fx = x + ((g.time * 7.0 + k as f32 * 2.1).sin() as f64) * 4.0 * s;
        let fh = (16.0 + k as f64 * 5.0) * f * s;
        c.set_fill_style_str(["rgba(255,90,30,0.85)", "rgba(255,160,50,0.85)", "rgba(255,230,140,0.9)"][k]);
        c.begin_path();
        c.move_to(fx - (9.0 - k as f64 * 2.5) * s, top - 3.0 * s);
        c.quadratic_curve_to(fx, top - fh * 1.4, fx + (9.0 - k as f64 * 2.5) * s, top - 3.0 * s);
        c.fill();
    }
}

fn kind_colors(k: Kind) -> (&'static str, &'static str) {
    match k {
        Kind::Knight => ("#6f7681", "#9b1d20"),
        Kind::Husk => ("#7b7468", "#4a443c"),
        Kind::Duelist => ("#8a8578", "#4b4b52"),
        Kind::Cultist => ("#5d4a5e", "#ff7a2f"),
        Kind::Pyreguard => ("#5b4a3c", "#ff6a1a"),
        Kind::Bellwarden => ("#3a3633", "#8c6a3a"),
    }
}

fn draw_actor(g: &Game, cm: &Camera, a: &Actor) {
    let c = &g.ctx;
    let s = cm.zoom as f64;
    let st = a.stats;
    let scale = if a.kind == Kind::Bellwarden { 1.75 } else if a.kind == Kind::Pyreguard { 1.15 } else { 1.0 };
    let (fx, fy) = proj(cm, a.pos, 0.0);
    // Shadow stays on the floor while jumping.
    let shadow_r = st.hurt_r as f64 * M as f64 * s * 1.3;
    c.set_fill_style_str("rgba(0,0,0,0.45)");
    c.begin_path(); c.ellipse(fx, fy, shadow_r, shadow_r * 0.4, 0.0, 0.0, TAU).ok(); c.fill();

    let mut height = st.height;
    let mut z = a.z;
    let mut alpha = 1.0;
    if a.rise > 0.0 { height *= 1.0 - (a.rise / 1.2).clamp(0.0, 1.0); }
    if a.drop > 0.0 { z += a.drop * a.drop * 2.5; }
    if a.state == State::Dead { height *= 0.25; alpha = (1.0 - (a.t - 0.6).max(0.0) as f64).clamp(0.0, 1.0); if a.kind == Kind::Bellwarden { alpha = 1.0; } }
    if a.state == State::Dodge { height *= 0.55; }
    if height < 0.05 { return; }
    c.set_global_alpha(alpha);

    let (bx, by) = proj(cm, a.pos, z);
    let (_, ty) = proj(cm, a.pos, z + height);
    let body_h = by - ty;
    let half_w = 0.32 * M as f64 * s * scale;
    let (base, accent) = kind_colors(a.kind);
    let lean = if a.state == State::Stagger { 0.25 * (a.t * 20.0).sin() as f64 } else { 0.0 };
    let face = a.facing;

    c.save();
    c.translate(bx, by).ok();
    c.rotate(lean).ok();
    // Cape / robe behind the body when facing away.
    if a.kind == Kind::Knight || a.kind == Kind::Cultist || a.kind == Kind::Bellwarden {
        c.set_fill_style_str(accent);
        let cape_side = -face.x as f64 * half_w * 0.4;
        c.fill_rect(-half_w * 0.9 + cape_side, -body_h * 0.82, half_w * 1.8, body_h * 0.72);
    }
    // Legs and torso.
    c.set_fill_style_str(if a.flash > 0.0 { "#ffffff" } else { base });
    c.fill_rect(-half_w * 0.7, -body_h * 0.45, half_w * 0.6, body_h * 0.45);
    c.fill_rect(half_w * 0.1, -body_h * 0.45, half_w * 0.6, body_h * 0.45);
    c.fill_rect(-half_w, -body_h * 0.82, half_w * 2.0, body_h * 0.42);
    // Head / helm.
    let head_r = half_w * 0.62;
    c.begin_path(); c.arc(0.0, -body_h * 0.82 - head_r * 0.7, head_r, 0.0, TAU).ok(); c.fill();
    if a.kind == Kind::Bellwarden {
        // Bell helm.
        c.set_fill_style_str(if a.flash > 0.0 { "#fff" } else { "#6b5435" });
        c.begin_path();
        c.move_to(-head_r * 1.3, -body_h * 0.82);
        c.quadratic_curve_to(-head_r * 1.1, -body_h * 0.82 - head_r * 2.4, 0.0, -body_h * 0.82 - head_r * 2.2);
        c.quadratic_curve_to(head_r * 1.1, -body_h * 0.82 - head_r * 2.4, head_r * 1.3, -body_h * 0.82);
        c.fill();
    }
    // Eyes: ember for the dead, visor slit for the knight.
    if face.y > -0.3 {
        let ey = -body_h * 0.82 - head_r * 0.75;
        if a.kind == Kind::Knight {
            c.set_fill_style_str("#111"); c.fill_rect(-head_r * 0.7, ey - 1.5, head_r * 1.4, 3.0);
        } else {
            c.set_fill_style_str("#ff7a2f");
            c.fill_rect(-head_r * 0.5, ey - 1.5, 3.0 * s, 3.0 * s);
            c.fill_rect(head_r * 0.25, ey - 1.5, 3.0 * s, 3.0 * s);
        }
    }
    c.restore();

    // Weapon: swings across the facing direction during an attack.
    if a.state != State::Dead && a.rise <= 0.0 {
        let mut ang = face.y.atan2(face.x);
        let mut len = match a.kind { Kind::Bellwarden => 2.6, Kind::Pyreguard => 1.5, Kind::Cultist => 1.2, Kind::Duelist => 1.1, _ => 1.0 };
        if let (Some(ph), Some(run)) = (a.phase(), a.run.as_ref()) {
            let atk = run.atk;
            let wind = (a.t / atk.windup).clamp(0.0, 1.0);
            let act = ((a.t - atk.windup) / atk.active).clamp(0.0, 1.0);
            ang += match ph { Phase::Windup => -1.3 * wind, Phase::Active => -1.3 + 2.6 * act, Phase::Recovery => 1.3 };
            if atk.id.contains("thrust") { len *= 1.0 + 0.6 * act; }
        } else {
            ang += 0.6;
        }
        let hand = a.pos + Vec2::new(face.y, -face.x) * -0.25 * scale as f32;
        let tip = hand + Vec2::new(ang.cos(), ang.sin()) * len;
        let (hx, hy) = proj(cm, hand, z + height * 0.5);
        let (tx, tyy) = proj(cm, tip, z + height * 0.55);
        c.set_stroke_style_str(match a.kind { Kind::Bellwarden => "#7a5c30", Kind::Cultist => "#4b3a2c", _ => "#cfc9bd" });
        c.set_line_width((if a.kind == Kind::Bellwarden { 6.0 } else { 3.0 }) * s);
        c.begin_path(); c.move_to(hx, hy); c.line_to(tx, tyy); c.stroke();
        if a.kind == Kind::Bellwarden {
            c.set_fill_style_str(if a.flash > 0.0 { "#fff" } else { "#8c6a3a" });
            c.begin_path(); c.arc(tx, tyy, 13.0 * s, 0.0, TAU).ok(); c.fill();
            c.set_fill_style_str("#2a1d10");
            c.begin_path(); c.arc(tx, tyy + 6.0 * s, 6.0 * s, 0.0, std::f64::consts::PI).ok(); c.fill();
        }
        if a.kind == Kind::Cultist {
            c.set_fill_style_str(&format!("rgba(255,140,50,{:.2})", flicker(g, a.uid as f32)));
            c.begin_path(); c.arc(tx, tyy, 4.0 * s, 0.0, TAU).ok(); c.fill();
        }
        if a.kind == Kind::Knight {
            // Shield on the off hand.
            let sh = a.pos + Vec2::new(face.y, -face.x) * 0.3 + face * 0.15;
            let (sx, sy) = proj(cm, sh, z + height * 0.45);
            c.set_fill_style_str("#4b4f57");
            c.fill_rect(sx - 6.0 * s, sy - 9.0 * s, 12.0 * s, 16.0 * s);
            c.set_fill_style_str("#9b1d20");
            c.fill_rect(sx - 2.0 * s, sy - 6.0 * s, 4.0 * s, 10.0 * s);
        }
    }
    // Knight's lantern glow keeps the player readable.
    if a.player && a.alive() {
        if let Ok(grad) = c.create_radial_gradient(bx, by - body_h * 0.5, 2.0, bx, by - body_h * 0.5, 90.0 * s) {
            grad.add_color_stop(0.0, "rgba(255,210,150,0.16)").ok();
            grad.add_color_stop(1.0, "rgba(255,200,140,0)").ok();
            c.set_fill_style_canvas_gradient(&grad);
            c.fill_rect(bx - 90.0 * s, by - body_h * 0.5 - 90.0 * s, 180.0 * s, 180.0 * s);
        }
    }
    c.set_global_alpha(1.0);
}

fn draw_telegraphs(g: &Game, cm: &Camera) {
    let c = &g.ctx;
    for a in g.sim.actors.iter().skip(1) {
        let (Some(Phase::Windup), Some(run)) = (a.phase(), a.run.as_ref()) else { continue };
        let atk = run.atk;
        let k = (a.t / atk.windup).clamp(0.0, 1.0) as f64;
        let pulse = 0.18 + 0.32 * k;
        if atk.is_melee() {
            // Footprint of the strike on the floor.
            let f = a.facing; let side = Vec2::new(-f.y, f.x);
            let ctr = a.pos + f * atk.fwd;
            let hw = side * (atk.w * 0.5); let hd = f * (atk.d * 0.5);
            let pts = [ctr - hw - hd, ctr + hw - hd, ctr + hw + hd, ctr - hw + hd];
            let low = atk.is_low();
            c.set_fill_style_str(&format!("rgba({},{:.3})", if low { "255,200,80" } else { "255,70,50" }, pulse));
            poly(c, cm, &pts, 0.02);
            c.fill();
            c.set_stroke_style_str(&format!("rgba(255,230,200,{:.3})", 0.3 + 0.5 * k));
            c.set_line_width(2.0);
            if low { c.set_line_dash(&js_sys::Array::of2(&8.0.into(), &6.0.into())).ok(); }
            c.stroke();
            c.set_line_dash(&js_sys::Array::new()).ok();
        }
        if let Some(spec) = atk.proj {
            let origin = a.pos + a.facing * atk.muzzle_fwd;
            if atk.ring {
                c.set_stroke_style_str(&format!("rgba(255,120,40,{:.3})", 0.25 + 0.5 * k));
                c.set_line_width(3.0);
                c.set_line_dash(&js_sys::Array::of2(&10.0.into(), &8.0.into())).ok();
                let r = spec.range * k as f32;
                c.begin_path();
                for i in 0..=40 {
                    let an = i as f32 / 40.0 * std::f32::consts::TAU;
                    let (x, y) = proj(cm, origin + Vec2::new(an.cos(), an.sin()) * r.max(0.3), 0.02);
                    if i == 0 { c.move_to(x, y) } else { c.line_to(x, y) }
                }
                c.stroke();
                c.set_line_dash(&js_sys::Array::new()).ok();
            } else {
                let base = if a.kind == Kind::Cultist { (run.aim - origin).normalized() } else { a.facing };
                for i in 0..atk.count {
                    let off = if atk.count > 1 { (i as f32 / (atk.count - 1) as f32 - 0.5) * atk.spread_deg.to_radians() } else { 0.0 };
                    let (s, co) = off.sin_cos();
                    let d = Vec2::new(base.x * co - base.y * s, base.x * s + base.y * co);
                    let end = lane_end(origin, d, spec.range);
                    let w = spec.radius;
                    let side = Vec2::new(-d.y, d.x) * w;
                    c.set_fill_style_str(&format!("rgba({},{:.3})", if spec.look == 1 { "180,120,255" } else { "255,60,60" }, pulse * 0.7));
                    poly(c, cm, &[origin - side, origin + side, end + side, end - side], 0.02);
                    c.fill();
                }
            }
        }
    }
}

/// Lanes stop where the shot would hit a pillar or a wall.
fn lane_end(origin: Vec2, d: Vec2, range: f32) -> Vec2 {
    let mut t = 0.0;
    while t < range {
        let p = origin + d * t;
        if combat::all_pillars().any(|pl| pl.distance(p) < PILLAR_R) { return p; }
        let in_nave = p.x > NAVE_MIN.x && p.x < NAVE_MAX.x && p.y > NAVE_MIN.y && p.y < NAVE_MAX.y;
        let in_belfry = p.distance(BELFRY_CENTER) < BELFRY_R;
        let in_corr = p.x.abs() < GATE_HALF_W && p.y <= NAVE_MIN.y && p.y >= CORRIDOR_END_Y;
        if !(in_nave || in_belfry || in_corr) { return p; }
        t += 0.2;
    }
    origin + d * range
}

fn draw_projectiles(g: &Game, cm: &Camera) {
    let c = &g.ctx;
    let s = cm.zoom as f64;
    for p in &g.sim.projectiles {
        let (x, y) = proj(cm, p.pos, p.z);
        let (gx, gy) = proj(cm, p.pos, 0.0);
        let r = p.spec.radius as f64 * M as f64 * s;
        let (shell, core) = match p.spec.look { 0 => ("rgba(220,40,50,0.55)", "#ffd0a0"), 1 => ("rgba(170,110,255,0.55)", "#f0e0ff"), _ => ("rgba(255,110,30,0.6)", "#fff0c0") };
        c.set_fill_style_str("rgba(0,0,0,0.3)");
        c.begin_path(); c.ellipse(gx, gy, r, r * 0.4, 0.0, 0.0, TAU).ok(); c.fill();
        // Short tapered trail.
        let (bx, by) = proj(cm, p.pos - p.dir * 0.7, p.z);
        c.set_stroke_style_str(shell); c.set_line_width(r * 1.2);
        c.begin_path(); c.move_to(bx, by); c.line_to(x, y); c.stroke();
        c.set_fill_style_str(shell);
        c.begin_path(); c.arc(x, y, r, 0.0, TAU).ok(); c.fill();
        c.set_fill_style_str(core);
        c.begin_path(); c.arc(x, y, r * 0.45, 0.0, TAU).ok(); c.fill();
    }
}

fn draw_overheads(g: &Game, cm: &Camera) {
    let c = &g.ctx;
    let s = cm.zoom as f64;
    for a in g.sim.actors.iter().skip(1) {
        if !a.alive() || a.rise > 0.0 || a.drop > 0.0 { continue; }
        let (x, y) = proj(cm, a.pos, a.stats.height + 0.5);
        // Tell word over the head during the wind-up.
        if let (Some(Phase::Windup), Some(run)) = (a.phase(), a.run.as_ref()) {
            let k = (a.t / run.atk.windup).clamp(0.0, 1.0) as f64;
            let size = (16.0 + 8.0 * k) * s.max(1.0);
            c.set_font(&format!("bold {size:.0}px Georgia, serif"));
            c.set_text_align("center");
            c.set_fill_style_str("rgba(0,0,0,0.7)");
            c.fill_text(run.atk.tell, x + 2.0, y - 18.0 + 2.0).ok();
            c.set_fill_style_str(if run.atk.is_low() { "#ffd166" } else { "#ff5a4a" });
            c.fill_text(run.atk.tell, x, y - 18.0).ok();
            c.set_text_align("start");
        }
        if a.kind == Kind::Bellwarden { continue; }
        // Small bar with a poise line under it.
        let bw = 40.0 * s;
        c.set_fill_style_str("rgba(10,8,8,0.8)"); c.fill_rect(x - bw / 2.0, y, bw, 5.0);
        c.set_fill_style_str("rgba(240,220,200,0.6)"); c.fill_rect(x - bw / 2.0, y, bw * (a.chip / a.stats.hp) as f64, 5.0);
        c.set_fill_style_str("#c0392b"); c.fill_rect(x - bw / 2.0, y, bw * (a.hp / a.stats.hp) as f64, 5.0);
        c.set_fill_style_str("#d9b86c"); c.fill_rect(x - bw / 2.0, y + 6.0, bw * (a.poise / a.stats.poise) as f64, 1.5);
    }
    for f in &g.floats {
        let (x, y) = proj(cm, f.pos, f.z + f.t * 1.2);
        c.set_global_alpha((1.0 - f.t / 1.1).max(0.0) as f64);
        c.set_font(&format!("bold {:.0}px Georgia, serif", 18.0 * s.max(1.0)));
        c.set_text_align("center");
        c.set_fill_style_str("rgba(0,0,0,0.8)"); c.fill_text(&f.text, x + 1.5, y + 1.5).ok();
        c.set_fill_style_str(f.color); c.fill_text(&f.text, x, y).ok();
        c.set_text_align("start");
        c.set_global_alpha(1.0);
    }
    // Rest prompt near an altar.
    let p = g.sim.player();
    for at in [NAVE_ALTAR, BELFRY_ALTAR] {
        if p.alive() && p.pos.distance(at) < combat::ALTAR_REACH {
            let (x, y) = proj(cm, at, 2.4);
            c.set_font("bold 16px Georgia, serif"); c.set_text_align("center");
            c.set_fill_style_str("#f3e3c0"); c.fill_text("[E] Rest at the Vigil Altar", x, y).ok();
            c.set_text_align("start");
        }
    }
}

fn draw_vignette(g: &Game) {
    let c = &g.ctx;
    let (w, h) = (g.canvas.width() as f64, g.canvas.height() as f64);
    if let Ok(grad) = c.create_radial_gradient(w / 2.0, h * 0.55, h * 0.3, w / 2.0, h * 0.55, h.max(w) * 0.75) {
        grad.add_color_stop(0.0, "rgba(0,0,0,0)").ok();
        grad.add_color_stop(1.0, "rgba(0,0,0,0.75)").ok();
        c.set_fill_style_canvas_gradient(&grad);
        c.fill_rect(0.0, 0.0, w, h);
    }
    if g.hurt > 0.0 {
        if let Ok(grad) = c.create_radial_gradient(w / 2.0, h / 2.0, h * 0.25, w / 2.0, h / 2.0, h.max(w) * 0.7) {
            grad.add_color_stop(0.0, "rgba(160,0,0,0)").ok();
            grad.add_color_stop(1.0, &format!("rgba(170,0,0,{:.3})", 0.55 * g.hurt)).ok();
            c.set_fill_style_canvas_gradient(&grad);
            c.fill_rect(0.0, 0.0, w, h);
        }
    }
}

fn bar(c: &CanvasRenderingContext2d, x: f64, y: f64, w: f64, h: f64, v: f32, chip: f32, color: &str) {
    c.set_fill_style_str("rgba(10,8,8,0.85)"); c.fill_rect(x - 2.0, y - 2.0, w + 4.0, h + 4.0);
    c.set_fill_style_str("#1a1614"); c.fill_rect(x, y, w, h);
    c.set_fill_style_str("rgba(240,225,200,0.55)"); c.fill_rect(x, y, w * chip.clamp(0.0, 1.0) as f64, h);
    c.set_fill_style_str(color); c.fill_rect(x, y, w * v.clamp(0.0, 1.0) as f64, h);
    c.set_stroke_style_str("#6b5a44"); c.set_line_width(1.0); c.stroke_rect(x - 2.0, y - 2.0, w + 4.0, h + 4.0);
}

fn draw_hud(g: &Game) {
    let c = &g.ctx;
    let (w, h) = (g.canvas.width() as f64, g.canvas.height() as f64);
    let ui = (h / 900.0).clamp(0.8, 2.0);
    let p = g.sim.player();
    let x0 = 28.0 * ui; let y0 = 26.0 * ui;
    bar(c, x0, y0, 300.0 * ui, 14.0 * ui, p.hp / p.stats.hp, p.chip / p.stats.hp, "#a8231f");
    bar(c, x0, y0 + 24.0 * ui, 240.0 * ui, 9.0 * ui, p.stamina / 100.0, p.stamina / 100.0, "#4f8a3a");
    c.set_font(&format!("{:.0}px Georgia, serif", 13.0 * ui));
    c.set_fill_style_str("#d8cbb0");
    c.fill_text(&format!("{:.0} / {:.0}", p.hp.max(0.0), p.stats.hp), x0 + 308.0 * ui, y0 + 12.0 * ui).ok();
    // Ember Flasks.
    for i in 0..3u8 {
        let fx = x0 + i as f64 * 22.0 * ui; let fy = y0 + 46.0 * ui;
        c.set_fill_style_str(if i < p.flasks { "#ff8c2a" } else { "#3a2f28" });
        c.begin_path(); c.arc(fx + 8.0 * ui, fy + 10.0 * ui, 7.0 * ui, 0.0, TAU).ok(); c.fill();
        c.fill_rect(fx + 5.0 * ui, fy, 6.0 * ui, 5.0 * ui);
    }
    c.fill_text(&format!("Ember Flasks  [Q]"), x0 + 72.0 * ui, y0 + 60.0 * ui).ok();

    // Stage line.
    let stage = match g.sim.stage {
        Stage::Wave(n) => format!("The Nave — wave {} of 3 · {} foes", n + 1, g.sim.enemies_alive()),
        Stage::GateOpen if g.sim.boss_defeated => "The cathedral is quiet".into(),
        Stage::GateOpen => "The belfry gate is open — go north".into(),
        Stage::BossArrival | Stage::Boss => "The Belfry".into(),
        Stage::Victory => "The bell is silent · rest at the belfry altar to begin anew".into(),
    };
    c.set_text_align("right");
    c.set_fill_style_str("#c9b896");
    c.fill_text(&stage, w - 28.0 * ui, 34.0 * ui).ok();
    c.set_text_align("start");

    if g.help {
        let lines = ["WASD move · Mouse aim", "LMB / J  light (20)   RMB / K  heavy (35)", "Shift  roll (25) · no direction = backstep",
            "Space  jump (hold for height) — jump LOW attacks", "Q  Ember Flask · E  rest at altar", "Wheel zoom · F5 restart · H hide help"];
        c.set_font(&format!("{:.0}px Georgia, serif", 12.0 * ui));
        c.set_fill_style_str("rgba(10,8,8,0.6)");
        c.fill_rect(w - 360.0 * ui, h - 120.0 * ui, 340.0 * ui, 104.0 * ui);
        c.set_fill_style_str("#b8a888");
        for (i, l) in lines.iter().enumerate() { c.fill_text(l, w - 350.0 * ui, h - 100.0 * ui + i as f64 * 15.0 * ui).ok(); }
    }

    // Boss bar.
    if let Some(b) = g.sim.boss_actor() {
        if b.drop <= 0.0 && (b.alive() || g.sim.stage == Stage::Boss) {
            let bw = (w * 0.56).min(900.0 * ui);
            let bx = (w - bw) / 2.0; let by = h - 70.0 * ui;
            c.set_font(&format!("{:.0}px Georgia, serif", 18.0 * ui));
            c.set_fill_style_str("#e6d7b5");
            c.fill_text(if g.sim.boss.phase2 { "The Bellwarden — Grave Toll" } else { "The Bellwarden" }, bx, by - 10.0 * ui).ok();
            bar(c, bx, by, bw, 12.0 * ui, b.hp / b.stats.hp, b.chip / b.stats.hp, "#8e1b17");
            c.set_text_align("right");
            c.set_font(&format!("{:.0}px Georgia, serif", 13.0 * ui));
            c.fill_text(&format!("{:.0} / {:.0}", b.hp.max(0.0), b.stats.hp), bx + bw, by - 10.0 * ui).ok();
            c.set_text_align("start");
        }
    }

    // Centre banner.
    if let Some((msg, t)) = &g.sim.banner {
        let dead = !p.alive();
        let a = t.min(1.0).max(0.0) as f64;
        if dead {
            c.set_fill_style_str(&format!("rgba(0,0,0,{:.2})", 0.55 * a));
            c.fill_rect(0.0, h * 0.4, w, h * 0.16);
        }
        c.set_font(&format!("{:.0}px Georgia, serif", if dead { 54.0 } else { 34.0 } * ui));
        c.set_text_align("center");
        c.set_fill_style_str(&format!("rgba({},{:.2})", if dead { "170,30,25" } else { "235,215,170" }, a));
        c.fill_text(msg, w / 2.0, h * 0.5).ok();
        c.set_text_align("start");
    }
    let _ = Area::Nave;
}
