use realm_shared::{Camera, Vec2};
use std::{cell::RefCell, collections::HashSet, f64::consts::TAU, rc::Rc};
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, KeyboardEvent, MouseEvent, WheelEvent};

#[derive(Clone)]
struct Projectile { pos: Vec2, vel: Vec2, life: f32, friendly: bool, damage: f32 }
#[derive(Clone)]
struct Enemy { pos: Vec2, hp: f32, max_hp: f32, cooldown: f32, kind: u8, phase: u8 }
#[derive(Clone)]
struct Loot { pos: Vec2, rarity: u8 }

struct Game {
    canvas: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
    keys: HashSet<String>,
    mouse: Vec2,
    down: bool,
    player: Vec2,
    camera: Camera,
    hp: f32,
    mana: f32,
    xp: u32,
    level: u32,
    fire_cd: f32,
    time: f32,
    score: u32,
    wave: u32,
    dead: bool,
    projectiles: Vec<Projectile>,
    enemies: Vec<Enemy>,
    loot: Vec<Loot>,
}

thread_local! { static GAME: RefCell<Option<Rc<RefCell<Game>>>> = const { RefCell::new(None) }; }

#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    let window = web_sys::window().unwrap();
    let document = window.document().unwrap();
    let canvas: HtmlCanvasElement = document.get_element_by_id("game").unwrap().dyn_into()?;
    let ctx: CanvasRenderingContext2d = canvas.get_context("2d")?.unwrap().dyn_into()?;
    ctx.set_image_smoothing_enabled(false);
    resize(&canvas);
    let camera = Camera { position: Vec2::ZERO, zoom: 1.15, viewport: viewport(&canvas) };
    let game = Rc::new(RefCell::new(Game {
        canvas: canvas.clone(), ctx, keys: HashSet::new(), mouse: Vec2::new(640.0, 360.0),
        down: false, player: Vec2::ZERO, camera, hp: 100.0, mana: 100.0, xp: 0, level: 1,
        fire_cd: 0.0, time: 0.0, score: 0, wave: 1, dead: false,
        projectiles: Vec::new(), enemies: Vec::new(), loot: Vec::new(),
    }));
    spawn_wave(&mut game.borrow_mut());
    install_events(game.clone())?;
    GAME.with(|slot| *slot.borrow_mut() = Some(game.clone()));
    let last = Rc::new(RefCell::new(window.performance().unwrap().now()));
    animation_loop(game, last);
    Ok(())
}

fn viewport(canvas: &HtmlCanvasElement) -> Vec2 { Vec2::new(canvas.width() as f32, canvas.height() as f32) }
fn resize(canvas: &HtmlCanvasElement) {
    let window = web_sys::window().unwrap();
    let dpr = window.device_pixel_ratio();
    let w = window.inner_width().unwrap().as_f64().unwrap();
    let h = window.inner_height().unwrap().as_f64().unwrap();
    canvas.set_width((w * dpr) as u32); canvas.set_height((h * dpr) as u32);
    canvas.style().set_property("width", &format!("{w}px")).ok();
    canvas.style().set_property("height", &format!("{h}px")).ok();
}

fn install_events(game: Rc<RefCell<Game>>) -> Result<(), JsValue> {
    let window = web_sys::window().unwrap();
    let g = game.clone();
    let keydown = Closure::<dyn FnMut(_)>::new(move |e: KeyboardEvent| {
        e.prevent_default();
        let mut s = g.borrow_mut();
        if e.key().eq_ignore_ascii_case("r") && s.dead { reset(&mut s); }
        s.keys.insert(e.key().to_lowercase());
    });
    window.add_event_listener_with_callback("keydown", keydown.as_ref().unchecked_ref())?;
    keydown.forget();
    let g = game.clone();
    let keyup = Closure::<dyn FnMut(_)>::new(move |e: KeyboardEvent| { g.borrow_mut().keys.remove(&e.key().to_lowercase()); });
    window.add_event_listener_with_callback("keyup", keyup.as_ref().unchecked_ref())?; keyup.forget();
    let g = game.clone();
    let mousemove = Closure::<dyn FnMut(_)>::new(move |e: MouseEvent| {
        let rect = g.borrow().canvas.get_bounding_client_rect();
        let mut s = g.borrow_mut();
        s.mouse = Vec2::new((e.client_x() as f64 - rect.left()) as f32 * s.canvas.width() as f32 / rect.width() as f32,
                            (e.client_y() as f64 - rect.top()) as f32 * s.canvas.height() as f32 / rect.height() as f32);
    });
    window.add_event_listener_with_callback("mousemove", mousemove.as_ref().unchecked_ref())?; mousemove.forget();
    let g = game.clone();
    let down = Closure::<dyn FnMut(_)>::new(move |_e: MouseEvent| g.borrow_mut().down = true);
    window.add_event_listener_with_callback("mousedown", down.as_ref().unchecked_ref())?; down.forget();
    let g = game.clone();
    let up = Closure::<dyn FnMut(_)>::new(move |_e: MouseEvent| g.borrow_mut().down = false);
    window.add_event_listener_with_callback("mouseup", up.as_ref().unchecked_ref())?; up.forget();
    let g = game;
    let wheel = Closure::<dyn FnMut(_)>::new(move |e: WheelEvent| {
        e.prevent_default(); let mut s = g.borrow_mut();
        s.camera.zoom = (s.camera.zoom - e.delta_y() as f32 * 0.0005).clamp(0.65, 2.1);
    });
    window.add_event_listener_with_callback("wheel", wheel.as_ref().unchecked_ref())?; wheel.forget();
    Ok(())
}

fn animation_loop(game: Rc<RefCell<Game>>, last: Rc<RefCell<f64>>) {
    let f = Rc::new(RefCell::new(None::<Closure<dyn FnMut(f64)>>));
    let f2 = f.clone();
    *f2.borrow_mut() = Some(Closure::new(move |now: f64| {
        let dt = (((now - *last.borrow()) / 1000.0) as f32).min(0.033);
        *last.borrow_mut() = now;
        { let mut s = game.borrow_mut(); update(&mut s, dt); render(&s); }
        web_sys::window().unwrap().request_animation_frame(f.borrow().as_ref().unwrap().as_ref().unchecked_ref()).unwrap();
    }));
    web_sys::window().unwrap().request_animation_frame(f2.borrow().as_ref().unwrap().as_ref().unchecked_ref()).unwrap();
}

fn update(g: &mut Game, dt: f32) {
    g.camera.viewport = viewport(&g.canvas); g.time += dt;
    if g.dead { return; }
    let x = key(g, "d", "arrowright") - key(g, "a", "arrowleft");
    let y = key(g, "s", "arrowdown") - key(g, "w", "arrowup");
    g.player += Vec2::new(x, y).normalized() * (145.0 + g.level as f32 * 2.0) * dt;
    let aim_world = g.camera.unproject_ground(g.mouse);
    let aim = (aim_world - g.player).normalized();
    g.camera.position = g.camera.position.lerp(g.player + aim * 65.0, 1.0 - (-7.0 * dt).exp());
    g.fire_cd -= dt; g.mana = (g.mana + 8.0 * dt).min(100.0);
    if (g.down || g.keys.contains(" ")) && g.fire_cd <= 0.0 {
        g.fire_cd = (0.23 - g.level as f32 * 0.005).max(0.11);
        g.projectiles.push(Projectile { pos: g.player + aim * 15.0, vel: aim * 330.0,
            life: 1.6, friendly: true, damage: 18.0 + g.level as f32 * 2.0 });
    }
    for p in &mut g.projectiles { p.pos += p.vel * dt; p.life -= dt; }
    for e in &mut g.enemies {
        let to_player = g.player - e.pos; let dist = to_player.length(); e.cooldown -= dt;
        if e.kind == 2 {
            e.phase = if e.hp < e.max_hp * 0.45 { 2 } else { 1 };
            e.pos += to_player.normalized() * if e.phase == 2 { 34.0 } else { 18.0 } * dt;
            if e.cooldown <= 0.0 {
                e.cooldown = if e.phase == 2 { 0.65 } else { 1.05 };
                let count = if e.phase == 2 { 18 } else { 10 };
                for i in 0..count { let a = i as f64 / count as f64 * TAU + g.time as f64;
                    g.projectiles.push(Projectile { pos: e.pos, vel: Vec2::new(a.cos() as f32, a.sin() as f32) * 115.0,
                        life: 4.0, friendly: false, damage: 11.0 }); }
            }
        } else {
            if dist > 95.0 { e.pos += to_player.normalized() * (48.0 + e.kind as f32 * 15.0) * dt; }
            if e.kind == 1 && e.cooldown <= 0.0 { e.cooldown = 1.4;
                g.projectiles.push(Projectile { pos: e.pos, vel: to_player.normalized() * 145.0,
                    life: 3.0, friendly: false, damage: 8.0 }); }
            if dist < 18.0 { g.hp -= 20.0 * dt; }
        }
    }
    let mut kills = Vec::new();
    for p in &mut g.projectiles {
        if p.friendly { for (i, e) in g.enemies.iter_mut().enumerate() {
            if p.life > 0.0 && p.pos.distance(e.pos) < if e.kind == 2 { 28.0 } else { 14.0 } {
                e.hp -= p.damage; p.life = 0.0; if e.hp <= 0.0 { kills.push(i); }
            }
        }} else if p.life > 0.0 && p.pos.distance(g.player) < 10.0 { g.hp -= p.damage; p.life = 0.0; }
    }
    kills.sort_unstable(); kills.dedup();
    for &i in kills.iter().rev() { let e = g.enemies.swap_remove(i); g.score += if e.kind == 2 { 1000 } else { 50 };
        g.xp += if e.kind == 2 { 300 } else { 25 }; g.loot.push(Loot { pos: e.pos, rarity: if e.kind == 2 { 5 } else { (g.score % 4) as u8 } }); }
    g.projectiles.retain(|p| p.life > 0.0);
    for l in &mut g.loot { if l.pos.distance(g.player) < 32.0 { l.rarity += 10; g.hp = (g.hp + 8.0).min(100.0); } }
    g.loot.retain(|l| l.rarity < 10);
    while g.xp >= g.level * 100 { g.xp -= g.level * 100; g.level += 1; g.hp = 100.0; }
    if g.enemies.is_empty() { g.wave += 1; spawn_wave(g); }
    if g.hp <= 0.0 { g.hp = 0.0; g.dead = true; }
}

fn key(g: &Game, a: &str, b: &str) -> f32 { if g.keys.contains(a) || g.keys.contains(b) { 1.0 } else { 0.0 } }
fn spawn_wave(g: &mut Game) {
    let count = 3 + g.wave.min(12);
    for i in 0..count { let a = i as f64 / count as f64 * TAU; let r = 210.0 + (i % 3) as f32 * 45.0;
        g.enemies.push(Enemy { pos: g.player + Vec2::new(a.cos() as f32, a.sin() as f32) * r,
            hp: 35.0 + g.wave as f32 * 8.0, max_hp: 35.0 + g.wave as f32 * 8.0,
            cooldown: i as f32 * 0.16, kind: (i % 2) as u8, phase: 0 }); }
    if g.wave % 3 == 0 { g.enemies.push(Enemy { pos: g.player + Vec2::new(0.0, -330.0),
        hp: 650.0 + g.wave as f32 * 70.0, max_hp: 650.0 + g.wave as f32 * 70.0,
        cooldown: 1.0, kind: 2, phase: 1 }); }
}
fn reset(g: &mut Game) { g.player = Vec2::ZERO; g.camera.position = Vec2::ZERO; g.hp = 100.0; g.mana = 100.0;
    g.xp = 0; g.level = 1; g.score = 0; g.wave = 1; g.dead = false; g.projectiles.clear(); g.enemies.clear(); g.loot.clear(); spawn_wave(g); }

fn render(g: &Game) {
    let c = &g.ctx; let w = g.canvas.width() as f64; let h = g.canvas.height() as f64;
    c.set_fill_style_str("#07131d"); c.fill_rect(0.0, 0.0, w, h);
    draw_ground(g);
    let mut order: Vec<(f32, u8, usize)> = g.enemies.iter().enumerate().map(|(i,e)|(g.camera.project(e.pos,0.0).y,1,i)).collect();
    order.push((g.camera.project(g.player,0.0).y,2,0)); order.sort_by(|a,b| a.0.total_cmp(&b.0));
    for (_, kind, i) in order { if kind == 2 { draw_actor(g, g.player, "#7ce8ff", 10.0, true); }
        else { let e=&g.enemies[i]; draw_actor(g,e.pos,if e.kind==2{"#ff486d"}else if e.kind==1{"#d87cff"}else{"#f4a261"},if e.kind==2{26.0}else{12.0},false); draw_enemy_hp(g,e); } }
    for p in &g.projectiles { let s=g.camera.project(p.pos,5.0); c.begin_path(); c.set_fill_style_str(if p.friendly{"#8ffcff"}else{"#ff5a70"});
        c.arc(s.x as f64,s.y as f64,if p.friendly{3.0}else{4.0},0.0,TAU).ok(); c.fill(); }
    for l in &g.loot { let s=g.camera.project(l.pos,4.0); c.set_fill_style_str(["#aaa","#61d27d","#4ba3ff","#bb75ff","#ffb84d","#fff06a"][(l.rarity.min(5)) as usize]); c.fill_rect(s.x as f64-5.0,s.y as f64-7.0,10.0,10.0); }
    draw_hud(g);
}
fn draw_ground(g: &Game) { let c=&g.ctx; let spacing=64.0; let center=g.camera.position;
    for iy in -10..11 { for ix in -16..17 { let p=Vec2::new((center.x/spacing).floor()*spacing+ix as f32*spacing,(center.y/spacing).floor()*spacing+iy as f32*spacing);
        let s=g.camera.project(p,0.0); let s2=g.camera.project(p+Vec2::new(spacing,spacing),0.0); c.set_fill_style_str(if (ix+iy)&1==0{"#173a35"}else{"#1b443b"}); c.fill_rect(s.x as f64,s.y as f64,(s2.x-s.x) as f64+1.0,(s2.y-s.y) as f64+1.0); }
    }
    for i in 0..28 { let x=((i*137)%900) as f32-450.0; let y=((i*251)%800) as f32-400.0; let p=Vec2::new(x,y); let base=g.camera.project(p,0.0); let top=g.camera.project(p,28.0);
        c.set_stroke_style_str("#285c45"); c.set_line_width(4.0); c.begin_path(); c.move_to(base.x as f64,base.y as f64); c.line_to(top.x as f64,top.y as f64); c.stroke(); c.set_fill_style_str("#3d8b5c"); c.fill_rect(top.x as f64-5.0,top.y as f64-4.0,10.0,8.0); }
}
fn draw_actor(g:&Game,p:Vec2,color:&str,r:f64,player:bool){let c=&g.ctx;let feet=g.camera.project(p,0.0);let head=g.camera.project(p,18.0);
    c.set_fill_style_str("rgba(0,0,0,.35)");c.begin_path();c.ellipse(feet.x as f64,feet.y as f64,r, r*0.35,0.0,0.0,TAU).ok();c.fill();
    c.set_fill_style_str(color);c.fill_rect(head.x as f64-r*0.55,head.y as f64,r*1.1,(feet.y-head.y) as f64);
    c.begin_path();c.arc(head.x as f64,head.y as f64,r*0.62,0.0,TAU).ok();c.fill(); if player{c.set_stroke_style_str("#fff");c.stroke();}}
fn draw_enemy_hp(g:&Game,e:&Enemy){if e.hp>=e.max_hp{return}let c=&g.ctx;let s=g.camera.project(e.pos,if e.kind==2{42.0}else{28.0});let width=if e.kind==2{70.0}else{30.0};
    c.set_fill_style_str("#29141a");c.fill_rect(s.x as f64-width/2.0,s.y as f64,width,4.0);c.set_fill_style_str("#ff486d");c.fill_rect(s.x as f64-width/2.0,s.y as f64,width*(e.hp/e.max_hp) as f64,4.0);}
fn draw_hud(g:&Game){let c=&g.ctx;c.set_font("bold 16px monospace");c.set_fill_style_str("rgba(5,10,18,.82)");c.fill_rect(18.0,18.0,320.0,96.0);
    bar(c,34.0,38.0,270.0,g.hp/100.0,"#e94f64");bar(c,34.0,64.0,270.0,g.mana/100.0,"#4f9de9");bar(c,34.0,90.0,270.0,g.xp as f32/(g.level*100) as f32,"#f2c14e");
    c.set_fill_style_str("#fff");c.fill_text(&format!("Lv {}  Wave {}  Score {}",g.level,g.wave,g.score),360.0,42.0).ok();
    c.set_fill_style_str("#b9cddd");c.fill_text("WASD move · Mouse aim/fire · Wheel zoom",360.0,68.0).ok();
    if let Some(b)=g.enemies.iter().find(|e|e.kind==2){let bw=(g.canvas.width() as f64*0.48).min(700.0);let x=(g.canvas.width() as f64-bw)/2.0;c.set_fill_style_str("#160b12");c.fill_rect(x,18.0,bw,18.0);c.set_fill_style_str("#ff486d");c.fill_rect(x,18.0,bw*(b.hp/b.max_hp) as f64,18.0);c.set_fill_style_str("#fff");c.fill_text("THE THORNED ORACLE",x+bw/2.0-90.0,33.0).ok();}
    if g.dead{c.set_fill_style_str("rgba(0,0,0,.76)");c.fill_rect(0.0,0.0,g.canvas.width() as f64,g.canvas.height() as f64);c.set_font("bold 42px monospace");c.set_fill_style_str("#ff657d");c.fill_text("YOUR ADVENTURE ENDS",g.canvas.width() as f64/2.0-250.0,g.canvas.height() as f64/2.0-20.0).ok();c.set_font("20px monospace");c.set_fill_style_str("#fff");c.fill_text(&format!("Score {} · Wave {} · Press R to begin anew",g.score,g.wave),g.canvas.width() as f64/2.0-235.0,g.canvas.height() as f64/2.0+28.0).ok();}}
fn bar(c:&CanvasRenderingContext2d,x:f64,y:f64,w:f64,v:f32,color:&str){c.set_fill_style_str("#17222b");c.fill_rect(x,y,w,14.0);c.set_fill_style_str(color);c.fill_rect(x,y,w*v.clamp(0.0,1.0) as f64,14.0);}
