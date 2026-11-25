//! Canvas2D renderer: oblique three-quarter projection, depth-sorted props, actors and projectiles.

use crate::{gfx, Ent, Game, M};
use realm_shared::{
    combat::{self, phase_of, Attack, Kind, Phase, State, ALTAR_REACH, BELFRY_ALTAR, BELFRY_CENTER, BELFRY_PILLARS, BELFRY_R,
        CORRIDOR_END_Y, GATE_HALF_W, NAVE_ALTAR, NAVE_MAX, NAVE_MIN, PILLARS, PILLAR_R, PORTAL_REACH},
    world::{band_name, Tile, WorldKind, NEXUS_FOUNTAIN},
    Camera, Vec2, CAMERA_Y_SCALE,
};
use std::f64::consts::{PI, TAU};
use web_sys::CanvasRenderingContext2d;

const MEMORIAL_POS: Vec2 = Vec2::new(5.5, 15.0);

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

fn attack_of(e: &Ent) -> Option<(&'static Attack, Phase)> {
    if e.net.a < 0 { return None; }
    let atk = *combat::ALL_ATTACKS.get(e.net.a as usize)?;
    Some((atk, phase_of(atk, e.net.t)))
}

fn flicker(g: &Game, seed: f32) -> f64 { (0.85 + 0.15 * ((g.time * 9.0 + seed).sin() * (g.time * 13.7 + seed * 2.0).sin())) as f64 }

/// Visible world rectangle in metres, padded.
fn view_rect(g: &Game, cm: &Camera) -> (Vec2, Vec2) {
    let (w, h) = (g.canvas.width() as f32, g.canvas.height() as f32);
    let a = cm.unproject_ground(Vec2::new(0.0, 0.0)) * (1.0 / M);
    let b = cm.unproject_ground(Vec2::new(w, h)) * (1.0 / M);
    (Vec2::new(a.x - 2.0, a.y - 2.0), Vec2::new(b.x + 2.0, b.y + 5.0))
}

pub fn render(g: &mut Game) {
    let cm = cam(g);
    let (w, h) = (g.canvas.width() as f64, g.canvas.height() as f64);
    g.ctx.set_image_smoothing_enabled(false);
    g.ctx.set_fill_style_str("#06070a");
    g.ctx.fill_rect(0.0, 0.0, w, h);

    if g.world.is_none() {
        let msg = if g.status.is_empty() { "Entering the realm…".to_string() } else { g.status.clone() };
        draw_status(g, &msg);
        return;
    }
    let world = g.world.unwrap();
    if world == WorldKind::Cathedral {
        draw_cathedral_floor(g, &cm);
        draw_altar(g, &cm, NAVE_ALTAR);
        draw_altar(g, &cm, BELFRY_ALTAR);
    } else {
        draw_terrain(g, &cm);
    }
    let g: &Game = g;
    draw_telegraphs(g, &cm);

    enum Item<'a> { Actor(&'a Ent), Prop(gfx::PropSprite, Vec2, f32), Pillar(Vec2), Brazier(Vec2), Wall(i32, i32, f32), Portal(usize), Fountain, Memorial }
    let mut items: Vec<(f32, Item)> = Vec::new();
    for e in g.ents.values() { items.push((Camera::depth(e.pos), Item::Actor(e))); }
    for (i, p) in g.portals.iter().enumerate() { items.push((p.y, Item::Portal(i))); }
    match world {
        WorldKind::Cathedral => {
            for &p in PILLARS.iter().chain(BELFRY_PILLARS.iter()) { items.push((p.y, Item::Pillar(p))); }
            for &b in cathedral_braziers().iter() { items.push((b.y, Item::Brazier(b))); }
        }
        WorldKind::Realm => {
            let (a, b) = view_rect(g, &cm);
            if let Some(map) = &g.map {
                for ty in a.y.floor() as i32..=b.y.ceil() as i32 { for tx in a.x.floor() as i32..=b.x.ceil() as i32 {
                    if let Some(pr) = map.prop_at(tx, ty) {
                        if let Some(sp) = g.props.get(&(pr.kind as u8, pr.variant)) {
                            let sprite = gfx::PropSprite { canvas: sp.canvas.clone(), w: sp.w, h: sp.h, anchor_y: sp.anchor_y };
                            let scale = 1.0 + (realm_shared::world::hash01(tx, ty, 99) - 0.5) * 0.3;
                            items.push((pr.pos.y, Item::Prop(sprite, pr.pos, scale)));
                        }
                    }
                    if map.get(tx, ty) == Tile::Rock && realm_shared::world::hash01(tx, ty, 41) < 0.8 {
                        if let Some(sp) = g.props.get(&(realm_shared::world::PropKind::Boulder as u8, (tx.rem_euclid(3)) as u8)) {
                            let sprite = gfx::PropSprite { canvas: sp.canvas.clone(), w: sp.w, h: sp.h, anchor_y: sp.anchor_y };
                            items.push((ty as f32 + 0.6, Item::Prop(sprite, Vec2::new(tx as f32 + 0.5, ty as f32 + 0.6), 1.5)));
                        }
                    }
                }}
            }
        }
        WorldKind::Concourse => {
            if let Some(map) = &g.map {
                for ty in 0..map.h { for tx in 0..map.w {
                    if map.get(tx, ty) != Tile::StoneWall { continue; }
                    let hgt = if ty == map.h - 1 { 0.5 } else { 2.2 };
                    items.push((ty as f32 + 1.0, Item::Wall(tx, ty, hgt)));
                }}
            }
            items.push((NEXUS_FOUNTAIN.y + 2.2, Item::Fountain));
            items.push((MEMORIAL_POS.y, Item::Memorial));
            for &b in concourse_braziers().iter() { items.push((b.y, Item::Brazier(b))); }
        }
    }
    items.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, it) in &items {
        match it {
            Item::Actor(e) => draw_actor(g, &cm, e),
            Item::Prop(sp, p, s) => draw_prop(g, &cm, sp, *p, *s),
            Item::Pillar(p) => draw_pillar(g, &cm, *p),
            Item::Brazier(b) => draw_brazier(g, &cm, *b),
            Item::Wall(x, y, hgt) => draw_wall_tile(g, &cm, *x, *y, *hgt),
            Item::Portal(i) => draw_portal(g, &cm, *i),
            Item::Fountain => draw_fountain(g, &cm),
            Item::Memorial => draw_memorial(g, &cm),
        }
    }
    if world == WorldKind::Cathedral { draw_cathedral_front(g, &cm); }
    draw_projectiles(g, &cm);
    for e in &g.embers {
        let (x, y) = proj(&cm, e.pos, e.z);
        g.ctx.set_fill_style_str(if e.ash { "rgba(170,160,150,0.35)" } else { "rgba(255,150,60,0.8)" });
        g.ctx.fill_rect(x, y, 2.0, 2.0);
    }
    draw_overheads(g, &cm);
    draw_lighting(g);
    draw_hud(g);
    if !g.status.is_empty() { draw_status(g, &g.status); }
}

// ------------------------------------------------------------ terrain ----

fn draw_terrain(g: &mut Game, cm: &Camera) {
    let (a, b) = view_rect(g, cm);
    let cs = gfx::CHUNK_M as f32;
    let mut want = Vec::new();
    for cy in (a.y / cs).floor() as i32..=(b.y / cs).floor() as i32 {
        for cx in (a.x / cs).floor() as i32..=(b.x / cs).floor() as i32 { want.push((cx, cy)); }
    }
    // Bake nearest chunks first.
    let center = g.pred;
    want.sort_by(|p, q| {
        let dp = Vec2::new((p.0 as f32 + 0.5) * cs, (p.1 as f32 + 0.5) * cs).distance(center);
        let dq = Vec2::new((q.0 as f32 + 0.5) * cs, (q.1 as f32 + 0.5) * cs).distance(center);
        dp.total_cmp(&dq)
    });
    g.ensure_chunks(&want);
    let c = &g.ctx;
    let sx = (cm.zoom * M / gfx::PPM as f32) as f64;
    let sy = sx * CAMERA_Y_SCALE as f64;

    // Animated water underneath everything; chunks leave water transparent.
    if let Ok(Some(pat)) = c.create_pattern_with_html_canvas_element(&g.tex.water, "repeat") {
        let drift = (g.time as f64 * 6.0, (g.time as f64 * 0.8).sin() * 10.0);
        let (ox, oy) = proj(cm, Vec2::ZERO, 0.0);
        c.set_transform(sx, 0.0, 0.0, sy, ox + drift.0 * sx, oy + drift.1 * sy).ok();
        c.set_fill_style_canvas_pattern(&pat);
        let ppm = gfx::PPM as f64;
        c.fill_rect(a.x as f64 * ppm - drift.0 - 64.0, a.y as f64 * ppm - drift.1 - 64.0, (b.x - a.x) as f64 * ppm + 128.0, (b.y - a.y) as f64 * ppm + 128.0);
        c.reset_transform().ok();
    }
    for k in &want {
        if let Some(chunk) = g.chunks.get(k) {
            let (ox, oy) = proj(cm, Vec2::new(k.0 as f32 * cs, k.1 as f32 * cs), 0.0);
            c.set_transform(sx, 0.0, 0.0, sy, ox, oy).ok();
            let n = gfx::CHUNK_PX as f64;
            c.draw_image_with_html_canvas_element_and_dw_and_dh(chunk, 0.0, 0.0, n + 0.6, n + 0.6).ok();
            c.reset_transform().ok();
        }
    }
}

fn draw_prop(g: &Game, cm: &Camera, sp: &gfx::PropSprite, p: Vec2, scale: f32) {
    let c = &g.ctx;
    let s = cm.zoom as f64 * 1.6 * scale as f64;
    let (x, y) = proj(cm, p, 0.0);
    // Contact shadow.
    c.set_fill_style_str("rgba(0,0,0,0.32)");
    c.begin_path(); c.ellipse(x + 3.0 * s, y, sp.w * 0.32 * s, sp.w * 0.12 * s, 0.0, 0.0, TAU).ok(); c.fill();
    c.draw_image_with_html_canvas_element_and_dw_and_dh(&sp.canvas, x - sp.w * 0.5 * s, y - sp.anchor_y * s, sp.w * s, sp.h * s).ok();
}

// ---------------------------------------------------------- concourse ----

fn concourse_braziers() -> [Vec2; 4] { [Vec2::new(4.0, 4.0), Vec2::new(32.0, 4.0), Vec2::new(4.0, 26.0), Vec2::new(32.0, 26.0)] }

fn draw_wall_face(g: &Game, cm: &Camera, a: Vec2, b: Vec2, h: f32, color: &str) {
    let c = &g.ctx;
    let (ax, ay) = proj(cm, a, 0.0); let (bx, by) = proj(cm, b, 0.0);
    let (ax2, ay2) = proj(cm, a, h); let (bx2, by2) = proj(cm, b, h);
    c.set_fill_style_str(color);
    c.begin_path(); c.move_to(ax, ay); c.line_to(bx, by); c.line_to(bx2, by2); c.line_to(ax2, ay2); c.close_path(); c.fill();
    c.set_stroke_style_str("rgba(0,0,0,0.35)");
    c.set_line_width(1.0);
    let rows = (h * 3.0).ceil() as i32;
    for k in 1..rows {
        let t = k as f64 / rows as f64;
        c.begin_path();
        c.move_to(ax + (ax2 - ax) * t, ay + (ay2 - ay) * t);
        c.line_to(bx + (bx2 - bx) * t, by + (by2 - by) * t);
        c.stroke();
        // Staggered vertical joints.
        let n = ((bx - ax).abs() / 26.0).max(1.0) as i32;
        for j in 0..n {
            let u = (j as f64 + if k % 2 == 0 { 0.5 } else { 0.0 }) / n as f64;
            let x = ax + (bx - ax) * u;
            let y0 = ay + (ay2 - ay) * t; let y1 = ay + (ay2 - ay) * ((k - 1) as f64 / rows as f64);
            c.begin_path(); c.move_to(x, y0); c.line_to(x, y1); c.stroke();
        }
    }
}

fn draw_wall_tile(g: &Game, cm: &Camera, x: i32, y: i32, h: f32) {
    let map = g.map.as_ref().unwrap();
    let (x0, y0) = (x as f32, y as f32);
    let below_open = map.get(x, y + 1) != Tile::StoneWall;
    if below_open { draw_wall_face(g, cm, Vec2::new(x0, y0 + 1.0), Vec2::new(x0 + 1.0, y0 + 1.0), h, "#3c3b3a"); }
    let c = &g.ctx;
    c.set_fill_style_str("#58564f");
    poly(c, cm, &[Vec2::new(x0, y0), Vec2::new(x0 + 1.0, y0), Vec2::new(x0 + 1.0, y0 + 1.0), Vec2::new(x0, y0 + 1.0)], h);
    c.fill();
    c.set_stroke_style_str("rgba(20,18,16,0.6)"); c.set_line_width(1.0); c.stroke();
}

fn draw_fountain(g: &Game, cm: &Camera) {
    let c = &g.ctx;
    let s = cm.zoom as f64;
    // Rim of cut stones.
    for k in 0..24 {
        let a = k as f32 / 24.0 * std::f32::consts::TAU;
        let p = NEXUS_FOUNTAIN + Vec2::new(a.cos(), a.sin()) * 2.35;
        let (x, y) = proj(cm, p, 0.35);
        c.set_fill_style_str(if k % 2 == 0 { "#7a776f" } else { "#6a675f" });
        c.fill_rect(x - 9.0 * s, y - 4.0 * s, 18.0 * s, 10.0 * s);
    }
    // Central basin and spout.
    let (x, y) = proj(cm, NEXUS_FOUNTAIN, 0.0);
    let (_, top) = proj(cm, NEXUS_FOUNTAIN, 1.8);
    c.set_fill_style_str("#6f6c64"); c.fill_rect(x - 8.0 * s, top, 16.0 * s, y - top);
    c.set_fill_style_str("#86837a"); c.fill_rect(x - 16.0 * s, top - 4.0 * s, 32.0 * s, 6.0 * s);
    for k in 0..10 {
        let a = k as f64 / 10.0 * TAU + g.time as f64 * 0.6;
        let ph = ((g.time as f64 * 1.6 + k as f64 * 0.37) % 1.0) as f64;
        let px = x + a.cos() * 40.0 * s * ph;
        let py = top - 10.0 * s + (ph * ph * 60.0 - ph * 30.0) * s;
        c.set_fill_style_str("rgba(170,215,240,0.8)");
        c.fill_rect(px, py, 2.5 * s, 2.5 * s);
    }
}

fn draw_memorial(g: &Game, cm: &Camera) {
    let c = &g.ctx;
    let s = cm.zoom as f64;
    let (x, y) = proj(cm, MEMORIAL_POS, 0.0);
    let (_, top) = proj(cm, MEMORIAL_POS, 3.0);
    c.set_fill_style_str("rgba(0,0,0,0.4)");
    c.begin_path(); c.ellipse(x, y, 26.0 * s, 9.0 * s, 0.0, 0.0, TAU).ok(); c.fill();
    c.set_fill_style_str("#3e3c40");
    c.begin_path(); c.move_to(x - 14.0 * s, y); c.line_to(x - 9.0 * s, top + 10.0 * s); c.line_to(x, top); c.line_to(x + 9.0 * s, top + 10.0 * s); c.line_to(x + 14.0 * s, y); c.close_path(); c.fill();
    c.set_fill_style_str("#57545a");
    c.begin_path(); c.move_to(x - 14.0 * s, y); c.line_to(x - 9.0 * s, top + 10.0 * s); c.line_to(x, top); c.line_to(x, y); c.close_path(); c.fill();
    c.set_fill_style_str(&format!("rgba(255,190,110,{:.2})", 0.7 * flicker(g, 4.0)));
    c.begin_path(); c.arc(x, top + 20.0 * s, 3.0 * s, 0.0, TAU).ok(); c.fill();
}

// ------------------------------------------------------------ portals ----

fn draw_portal(g: &Game, cm: &Camera, i: usize) {
    let p = &g.portals[i];
    let c = &g.ctx;
    let s = cm.zoom as f64;
    let pos = Vec2::new(p.x, p.y);
    let (x, y) = proj(cm, pos, 0.0);
    let (inner, outer) = match p.to { 0 => ("#c9a6ff", "#5b2ca8"), 1 => ("#a8e6ff", "#2a6fa8"), _ => ("#ffb08a", "#a8301e") };
    if let Ok(grad) = c.create_radial_gradient(x, y - 26.0 * s, 4.0, x, y - 26.0 * s, 70.0 * s) {
        grad.add_color_stop(0.0, &format!("{inner}88")).ok();
        grad.add_color_stop(1.0, "rgba(0,0,0,0)").ok();
        c.set_fill_style_canvas_gradient(&grad);
        c.fill_rect(x - 70.0 * s, y - 96.0 * s, 140.0 * s, 140.0 * s);
    }
    // Stone arch for permanent portals.
    if p.ttl < 0.0 {
        c.set_fill_style_str("#4a4744");
        c.fill_rect(x - 30.0 * s, y - 62.0 * s, 9.0 * s, 62.0 * s);
        c.fill_rect(x + 21.0 * s, y - 62.0 * s, 9.0 * s, 62.0 * s);
        c.set_stroke_style_str("#4a4744"); c.set_line_width(9.0 * s);
        c.begin_path(); c.arc(x, y - 62.0 * s, 25.5 * s, PI, TAU).ok(); c.stroke();
    }
    // Swirling vortex.
    for k in 0..5 {
        let r = (22.0 - k as f64 * 4.0) * s;
        let rot = g.time as f64 * (1.5 + k as f64 * 0.4) * if k % 2 == 0 { 1.0 } else { -1.0 };
        c.set_stroke_style_str(if k % 2 == 0 { inner } else { outer });
        c.set_line_width(3.0 * s);
        c.begin_path(); c.ellipse(x, y - 34.0 * s, r * 0.75, r * 1.25, 0.0, rot, rot + PI * 1.4).ok(); c.stroke();
    }
    let label = match p.to { 0 => "The Ashen Realm".to_string(), 1 => "Eternal Concourse".to_string(), _ => format!("Cathedral of the Vigil  {:.0}s", p.ttl.max(0.0)) };
    c.set_font(&format!("bold {:.0}px Georgia, serif", 13.0 * s.max(1.0)));
    c.set_text_align("center");
    c.set_fill_style_str("rgba(0,0,0,0.7)"); c.fill_text(&label, x + 1.0, y - 74.0 * s + 1.0).ok();
    c.set_fill_style_str(inner); c.fill_text(&label, x, y - 74.0 * s).ok();
    if g.pred.distance(pos) < PORTAL_REACH {
        c.set_fill_style_str("#f3e3c0");
        c.fill_text("[E] Enter", x, y + 18.0 * s).ok();
    }
    c.set_text_align("start");
}

// ---------------------------------------------------------- cathedral ----

fn cathedral_braziers() -> [Vec2; 6] {
    [Vec2::new(-9.5, -6.5), Vec2::new(9.5, -6.5), Vec2::new(-9.5, 6.5), Vec2::new(9.5, 6.5),
     BELFRY_CENTER + Vec2::new(-6.3, 1.5), BELFRY_CENTER + Vec2::new(6.3, 1.5)]
}

fn draw_cathedral_floor(g: &Game, cm: &Camera) {
    let c = &g.ctx;
    // Flagstones with the stone texture, screen-aligned pattern under each slab's tint.
    let pat = c.create_pattern_with_html_canvas_element(&g.tex.stone_canvas, "repeat").ok().flatten();
    let rects = [
        (NAVE_MIN, NAVE_MAX),
        (Vec2::new(-GATE_HALF_W, CORRIDOR_END_Y - 0.6), Vec2::new(GATE_HALF_W, NAVE_MIN.y)),
    ];
    for (a, b) in rects {
        poly(c, cm, &[a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)], 0.0);
        if let Some(p) = &pat {
            let (ox, oy) = proj(cm, Vec2::ZERO, 0.0);
            let sx = (cm.zoom * M / gfx::PPM as f32) as f64;
            c.set_transform(sx, 0.0, 0.0, sx * CAMERA_Y_SCALE as f64, ox, oy).ok();
            c.set_fill_style_canvas_pattern(p);
            let ppm = gfx::PPM as f64;
            c.fill_rect(a.x as f64 * ppm, a.y as f64 * ppm, (b.x - a.x) as f64 * ppm, (b.y - a.y) as f64 * ppm);
            c.reset_transform().ok();
        } else { c.set_fill_style_str("#2a2c2e"); c.fill(); }
    }
    c.set_fill_style_str("rgba(10,8,8,0.35)");
    poly(c, cm, &[NAVE_MIN, Vec2::new(NAVE_MAX.x, NAVE_MIN.y), NAVE_MAX, Vec2::new(NAVE_MIN.x, NAVE_MAX.y)], 0.0);
    c.fill();
    let oct: Vec<Vec2> = (0..8).map(|i| {
        let a = (i as f32 + 0.5) / 8.0 * std::f32::consts::TAU;
        BELFRY_CENTER + Vec2::new(a.cos(), a.sin()) * (BELFRY_R + 0.4)
    }).collect();
    c.set_fill_style_str("#2a2624");
    poly(c, cm, &oct, 0.0);
    c.fill();
    for r in [2.0f32, 4.5, 6.8] {
        c.set_stroke_style_str("rgba(10,8,8,0.6)"); c.set_line_width(2.0);
        c.begin_path();
        for i in 0..=32 {
            let a = i as f32 / 32.0 * std::f32::consts::TAU;
            let (x, y) = proj(cm, BELFRY_CENTER + Vec2::new(a.cos(), a.sin()) * r, 0.0);
            if i == 0 { c.move_to(x, y) } else { c.line_to(x, y) }
        }
        c.stroke();
    }
    let wall_h = 2.6;
    for (a, b) in [(NAVE_MIN.x, -GATE_HALF_W), (GATE_HALF_W, NAVE_MAX.x)] {
        draw_wall_face(g, cm, Vec2::new(a, NAVE_MIN.y), Vec2::new(b, NAVE_MIN.y), wall_h, "#3a3b3d");
    }
    draw_wall_face(g, cm, Vec2::new(NAVE_MIN.x, NAVE_MIN.y), Vec2::new(NAVE_MIN.x, NAVE_MAX.y), wall_h, "#2e2f31");
    draw_wall_face(g, cm, Vec2::new(NAVE_MAX.x, NAVE_MIN.y), Vec2::new(NAVE_MAX.x, NAVE_MAX.y), wall_h, "#2e2f31");
    if !g.gate {
        c.set_stroke_style_str("#6b5a44"); c.set_line_width(3.0);
        let mut x = -GATE_HALF_W + 0.2;
        while x < GATE_HALF_W {
            let (x0, y0) = proj(cm, Vec2::new(x, NAVE_MIN.y), 0.0);
            let (x1, y1) = proj(cm, Vec2::new(x, NAVE_MIN.y), wall_h);
            c.begin_path(); c.move_to(x0, y0); c.line_to(x1, y1); c.stroke();
            x += 0.4;
        }
    }
    for i in 0..8 {
        let a0 = (i as f32 + 0.5) / 8.0 * std::f32::consts::TAU;
        let a1 = (i as f32 + 1.5) / 8.0 * std::f32::consts::TAU;
        let p0 = BELFRY_CENTER + Vec2::new(a0.cos(), a0.sin()) * (BELFRY_R + 0.4);
        let p1 = BELFRY_CENTER + Vec2::new(a1.cos(), a1.sin()) * (BELFRY_R + 0.4);
        if (p0.y + p1.y) * 0.5 < BELFRY_CENTER.y { draw_wall_face(g, cm, p0, p1, 2.0, "#34302d"); }
    }
    if g.mist {
        let y = CORRIDOR_END_Y + 0.5;
        let (x0, y0) = proj(cm, Vec2::new(-GATE_HALF_W, y), 0.0);
        let (x1, y1) = proj(cm, Vec2::new(GATE_HALF_W, y), 3.0);
        let a = 0.22 + 0.06 * (g.time * 1.5).sin() as f64;
        c.set_fill_style_str(&format!("rgba(190,200,215,{a:.3})"));
        c.fill_rect(x0, y1, x1 - x0, y0 - y1);
    }
    for (i, b) in cathedral_braziers().iter().enumerate() {
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

fn draw_cathedral_front(g: &Game, cm: &Camera) {
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
    c.set_fill_style_str("#4a4540"); c.fill_rect(x - 18.0 * s, top, 36.0 * s, y - top);
    c.set_fill_style_str("#5c564f"); c.fill_rect(x - 22.0 * s, top - 5.0 * s, 44.0 * s, 7.0 * s);
    c.set_stroke_style_str("#c8c2b4"); c.set_line_width(2.5 * s);
    c.begin_path(); c.move_to(x, top - 4.0 * s); c.line_to(x, top - 34.0 * s); c.stroke();
    c.begin_path(); c.move_to(x - 7.0 * s, top - 26.0 * s); c.line_to(x + 7.0 * s, top - 26.0 * s); c.stroke();
}

fn draw_pillar(g: &Game, cm: &Camera, p: Vec2) {
    let c = &g.ctx;
    let s = cm.zoom as f64;
    let (x, y) = proj(cm, p, 0.0);
    let (_, top) = proj(cm, p, 4.2);
    let r = PILLAR_R as f64 * M as f64 * s;
    c.set_fill_style_str("rgba(0,0,0,0.4)");
    c.begin_path(); c.ellipse(x, y, r * 1.5, r * 0.6, 0.0, 0.0, TAU).ok(); c.fill();
    c.set_fill_style_str("#46484a"); c.fill_rect(x - r * 1.3, y - 8.0 * s, r * 2.6, 8.0 * s);
    let grad = c.create_linear_gradient(x - r, 0.0, x + r, 0.0);
    grad.add_color_stop(0.0, "#2c2e30").ok(); grad.add_color_stop(0.45, "#55585a").ok(); grad.add_color_stop(1.0, "#26282a").ok();
    c.set_fill_style_canvas_gradient(&grad);
    c.fill_rect(x - r, top, r * 2.0, y - 8.0 * s - top);
    c.set_fill_style_str("#4d4f51"); c.fill_rect(x - r * 1.4, top - 6.0 * s, r * 2.8, 8.0 * s);
}

fn draw_brazier(g: &Game, cm: &Camera, b: Vec2) {
    let c = &g.ctx;
    let s = cm.zoom as f64;
    let (x, y) = proj(cm, b, 0.0);
    let (_, top) = proj(cm, b, 1.0);
    let r = 110.0 * s * flicker(g, b.x);
    if let Ok(grad) = c.create_radial_gradient(x, top, 2.0, x, top, r) {
        grad.add_color_stop(0.0, "rgba(255,150,70,0.22)").ok();
        grad.add_color_stop(1.0, "rgba(255,120,40,0)").ok();
        c.set_fill_style_canvas_gradient(&grad);
        c.fill_rect(x - r, top - r, r * 2.0, r * 2.0);
    }
    c.set_stroke_style_str("#3b3229"); c.set_line_width(3.0 * s);
    c.begin_path(); c.move_to(x - 8.0 * s, y); c.line_to(x, top); c.line_to(x + 8.0 * s, y); c.stroke();
    c.set_fill_style_str("#2a221c"); c.fill_rect(x - 12.0 * s, top - 4.0 * s, 24.0 * s, 7.0 * s);
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

// ------------------------------------------------------------- actors ----

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

fn draw_actor(g: &Game, cm: &Camera, e: &Ent) {
    let kind = Kind::from_u8(e.net.k);
    if kind == Kind::Knight && g.warrior.complete() && g.warrior.natural_width() > 0 { draw_warrior(g, cm, e); return; }
    draw_figure(g, cm, e, kind);
}

/// The Warrior: 8-direction sprite sheet (west is the east row mirrored), 10-frame walk.
fn draw_warrior(g: &Game, cm: &Camera, e: &Ent) {
    let c = &g.ctx;
    let s = cm.zoom as f64;
    let state = State::from_u8(e.net.s);
    let (fx, fy) = (e.net.fx, e.net.fy);
    let sector = ((fy.atan2(fx) / std::f32::consts::FRAC_PI_4).round() as i32).rem_euclid(8);
    let (row, mirror) = match sector { 0 => (2, false), 1 => (7, false), 2 => (0, false), 3 => (1, false), 4 => (2, true), 5 => (3, false), 6 => (4, false), _ => (5, false) };
    let frame = if e.moving && matches!(state, State::Idle | State::Heal) { ((e.walk * 2.6) as i32).rem_euclid(10) } else { 0 };
    let (cw, ch) = (113.0, 115.0);
    let scale = 2.15 * M as f64 * 0.95 * s / 110.0;
    let (dw, dh) = (cw * scale, ch * scale);
    let (gx, gy) = proj(cm, e.pos, 0.0);
    let (x, y) = proj(cm, e.pos, e.z);
    let mut alpha = 1.0;
    if state == State::Dead { alpha = (1.0 - (e.net.t as f64 - 0.8).max(0.0)).clamp(0.0, 1.0); }
    if alpha <= 0.0 { return; }
    c.set_fill_style_str("rgba(0,0,0,0.42)");
    c.begin_path(); c.ellipse(gx, gy, 15.0 * s, 5.5 * s, 0.0, 0.0, TAU).ok(); c.fill();

    // Roll: afterimages trail behind.
    if state == State::Dodge {
        for k in 1..3 {
            let back = Vec2::new(e.net.vx, e.net.vy) * (-0.035 * k as f32);
            let (bx, by) = proj(cm, e.pos + back, e.z);
            c.set_global_alpha(0.22 / k as f64);
            blit_warrior(g, row, frame, mirror, bx, by, dw, dh, 0.0);
        }
    }
    c.set_global_alpha(alpha);
    let shake = if state == State::Stagger { (e.net.t * 60.0).sin() as f64 * 3.0 * s } else { 0.0 };
    let tilt = if state == State::Dead { PI / 2.0 * (e.net.t as f64 * 3.0).min(1.0) } else if state == State::Dodge { 0.25 * if mirror { -1.0 } else { 1.0 } } else { 0.0 };
    blit_warrior(g, row, frame, mirror, x + shake, y, dw, dh * if state == State::Dodge { 0.8 } else { 1.0 }, tilt);
    if e.net.fl {
        c.set_global_composite_operation("lighter").ok();
        c.set_global_alpha(0.55);
        blit_warrior(g, row, frame, mirror, x + shake, y, dw, dh, tilt);
        c.set_global_composite_operation("source-over").ok();
    }
    c.set_global_alpha(1.0);
    // Sword arc during the active frames of an attack.
    if let Some((atk, ph)) = attack_of(e) {
        if ph != Phase::Windup {
            let k = ((e.net.t - atk.windup) / atk.active).clamp(0.0, 1.0) as f64;
            let fade = if ph == Phase::Active { 0.85 } else { (0.85 - (e.net.t - atk.windup - atk.active) as f64 * 4.0).max(0.0) };
            if fade > 0.0 {
                let ang = (fy as f64).atan2(fx as f64);
                let reach = (atk.reach() as f64 + 0.2) * M as f64 * s;
                let (cx, cy) = proj(cm, e.pos, e.z + 1.0);
                c.save();
                c.translate(cx, cy).ok();
                c.scale(1.0, CAMERA_Y_SCALE as f64).ok();
                c.set_stroke_style_str(&format!("rgba(235,240,255,{fade:.2})"));
                c.set_line_width((if atk.heavy { 7.0 } else { 4.5 }) * s);
                c.begin_path();
                let span = if atk.heavy { 0.9 } else { 1.25 };
                c.arc(0.0, 0.0, reach, ang - span, ang - span + 2.0 * span * k.max(0.15)).ok();
                c.stroke();
                c.restore();
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn blit_warrior(g: &Game, row: i32, frame: i32, mirror: bool, x: f64, y: f64, dw: f64, dh: f64, tilt: f64) {
    let c = &g.ctx;
    let (cw, ch) = (113.0, 115.0);
    c.save();
    c.translate(x, y).ok();
    if tilt != 0.0 { c.rotate(tilt).ok(); }
    if mirror { c.scale(-1.0, 1.0).ok(); }
    c.draw_image_with_html_image_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
        &g.warrior, frame as f64 * cw, row as f64 * ch, cw, ch, -dw / 2.0, -dh + 2.0, dw, dh).ok();
    c.restore();
}

/// Enemies are still drawn in code until their sprite sheets exist.
fn draw_figure(g: &Game, cm: &Camera, e: &Ent, kind: Kind) {
    let c = &g.ctx;
    let s = cm.zoom as f64;
    let st = kind.stats();
    let state = State::from_u8(e.net.s);
    let scale = if kind == Kind::Bellwarden { 1.75 } else if kind == Kind::Pyreguard { 1.15 } else { 1.0 };
    let (fx, fy) = proj(cm, e.pos, 0.0);
    let shadow_r = st.hurt_r as f64 * M as f64 * s * 1.3;
    c.set_fill_style_str("rgba(0,0,0,0.45)");
    c.begin_path(); c.ellipse(fx, fy, shadow_r, shadow_r * 0.4, 0.0, 0.0, TAU).ok(); c.fill();
    let mut height = st.height;
    let mut z = e.z;
    let mut alpha = 1.0;
    if e.net.r > 0.0 { height *= 1.0 - (e.net.r / 1.2).clamp(0.0, 1.0); }
    if e.net.d > 0.0 { z += e.net.d * e.net.d * 2.5; }
    if state == State::Dead { height *= 0.25; alpha = (1.0 - (e.net.t - 0.6).max(0.0) as f64).clamp(0.0, 1.0); if kind == Kind::Bellwarden { alpha = 1.0; } }
    if state == State::Dodge { height *= 0.55; }
    if height < 0.05 { return; }
    c.set_global_alpha(alpha);
    let (bx, by) = proj(cm, e.pos, z);
    let (_, ty) = proj(cm, e.pos, z + height);
    let body_h = by - ty;
    let half_w = 0.32 * M as f64 * s * scale;
    let (base, accent) = kind_colors(kind);
    let lean = if state == State::Stagger { 0.25 * (e.net.t * 20.0).sin() as f64 } else { 0.0 };
    let face = Vec2::new(e.net.fx, e.net.fy);
    c.save();
    c.translate(bx, by).ok();
    c.rotate(lean).ok();
    if kind == Kind::Cultist || kind == Kind::Bellwarden {
        c.set_fill_style_str(accent);
        c.fill_rect(-half_w * 0.9 - face.x as f64 * half_w * 0.4, -body_h * 0.82, half_w * 1.8, body_h * 0.72);
    }
    c.set_fill_style_str(if e.net.fl { "#ffffff" } else { base });
    c.fill_rect(-half_w * 0.7, -body_h * 0.45, half_w * 0.6, body_h * 0.45);
    c.fill_rect(half_w * 0.1, -body_h * 0.45, half_w * 0.6, body_h * 0.45);
    c.fill_rect(-half_w, -body_h * 0.82, half_w * 2.0, body_h * 0.42);
    let head_r = half_w * 0.62;
    c.begin_path(); c.arc(0.0, -body_h * 0.82 - head_r * 0.7, head_r, 0.0, TAU).ok(); c.fill();
    if kind == Kind::Bellwarden {
        c.set_fill_style_str(if e.net.fl { "#fff" } else { "#6b5435" });
        c.begin_path();
        c.move_to(-head_r * 1.3, -body_h * 0.82);
        c.quadratic_curve_to(-head_r * 1.1, -body_h * 0.82 - head_r * 2.4, 0.0, -body_h * 0.82 - head_r * 2.2);
        c.quadratic_curve_to(head_r * 1.1, -body_h * 0.82 - head_r * 2.4, head_r * 1.3, -body_h * 0.82);
        c.fill();
    }
    if face.y > -0.3 {
        let ey = -body_h * 0.82 - head_r * 0.75;
        c.set_fill_style_str("#ff7a2f");
        c.fill_rect(-head_r * 0.5, ey - 1.5, 3.0 * s, 3.0 * s);
        c.fill_rect(head_r * 0.25, ey - 1.5, 3.0 * s, 3.0 * s);
    }
    c.restore();
    if state != State::Dead && e.net.r <= 0.0 {
        let mut ang = face.y.atan2(face.x);
        let mut len = match kind { Kind::Bellwarden => 2.6, Kind::Pyreguard => 1.5, Kind::Cultist => 1.2, Kind::Duelist => 1.1, _ => 1.0 };
        if let Some((atk, ph)) = attack_of(e) {
            let wind = (e.net.t / atk.windup).clamp(0.0, 1.0);
            let act = ((e.net.t - atk.windup) / atk.active).clamp(0.0, 1.0);
            ang += match ph { Phase::Windup => -1.3 * wind, Phase::Active => -1.3 + 2.6 * act, Phase::Recovery => 1.3 };
            if atk.id.contains("thrust") { len *= 1.0 + 0.6 * act; }
        } else { ang += 0.6; }
        let hand = e.pos + Vec2::new(face.y, -face.x) * -0.25 * scale as f32;
        let tip = hand + Vec2::new(ang.cos(), ang.sin()) * len;
        let (hx, hy) = proj(cm, hand, z + height * 0.5);
        let (tx, tyy) = proj(cm, tip, z + height * 0.55);
        c.set_stroke_style_str(match kind { Kind::Bellwarden => "#7a5c30", Kind::Cultist => "#4b3a2c", _ => "#cfc9bd" });
        c.set_line_width((if kind == Kind::Bellwarden { 6.0 } else { 3.0 }) * s);
        c.begin_path(); c.move_to(hx, hy); c.line_to(tx, tyy); c.stroke();
        if kind == Kind::Bellwarden {
            c.set_fill_style_str(if e.net.fl { "#fff" } else { "#8c6a3a" });
            c.begin_path(); c.arc(tx, tyy, 13.0 * s, 0.0, TAU).ok(); c.fill();
        }
        if kind == Kind::Cultist {
            c.set_fill_style_str(&format!("rgba(255,140,50,{:.2})", flicker(g, e.net.u as f32)));
            c.begin_path(); c.arc(tx, tyy, 4.0 * s, 0.0, TAU).ok(); c.fill();
        }
    }
    c.set_global_alpha(1.0);
}

// --------------------------------------------------------- telegraphs ----

fn lane_end(g: &Game, origin: Vec2, d: Vec2, range: f32) -> Vec2 {
    let mut t = 0.0;
    while t < range {
        let p = origin + d * t;
        let blocked = match (&g.map, g.world) {
            (_, Some(WorldKind::Cathedral)) => combat::all_pillars().any(|pl| pl.distance(p) < PILLAR_R) || !combat::cathedral_walkable(p, 0.0, false, g.gate, false),
            (Some(m), _) => m.at(p).solid(),
            _ => false,
        };
        if blocked { return p; }
        t += 0.2;
    }
    origin + d * range
}

fn draw_telegraphs(g: &Game, cm: &Camera) {
    let c = &g.ctx;
    for e in g.ents.values() {
        if Kind::from_u8(e.net.k) == Kind::Knight { continue; }
        let Some((atk, Phase::Windup)) = attack_of(e) else { continue };
        let k = (e.net.t / atk.windup).clamp(0.0, 1.0) as f64;
        let pulse = 0.18 + 0.32 * k;
        let f = Vec2::new(e.net.fx, e.net.fy).normalized();
        if atk.is_melee() {
            let side = Vec2::new(-f.y, f.x);
            let ctr = e.pos + f * atk.fwd;
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
            let origin = e.pos + f * atk.muzzle_fwd;
            if atk.ring {
                c.set_stroke_style_str(&format!("rgba(255,120,40,{:.3})", 0.25 + 0.5 * k));
                c.set_line_width(3.0);
                c.set_line_dash(&js_sys::Array::of2(&10.0.into(), &8.0.into())).ok();
                let r = (spec.range * k as f32).max(0.3);
                c.begin_path();
                for i in 0..=40 {
                    let an = i as f32 / 40.0 * std::f32::consts::TAU;
                    let (x, y) = proj(cm, origin + Vec2::new(an.cos(), an.sin()) * r, 0.02);
                    if i == 0 { c.move_to(x, y) } else { c.line_to(x, y) }
                }
                c.stroke();
                c.set_line_dash(&js_sys::Array::new()).ok();
            } else {
                let base = if Kind::from_u8(e.net.k) == Kind::Cultist { (Vec2::new(e.net.ax, e.net.ay) - origin).normalized() } else { f };
                for i in 0..atk.count {
                    let off = if atk.count > 1 { (i as f32 / (atk.count - 1) as f32 - 0.5) * atk.spread_deg.to_radians() } else { 0.0 };
                    let (sn, co) = off.sin_cos();
                    let d = Vec2::new(base.x * co - base.y * sn, base.x * sn + base.y * co);
                    let end = lane_end(g, origin, d, spec.range);
                    let side = Vec2::new(-d.y, d.x) * spec.radius;
                    c.set_fill_style_str(&format!("rgba({},{:.3})", if spec.look == 1 { "180,120,255" } else { "255,60,60" }, pulse * 0.7));
                    poly(c, cm, &[origin - side, origin + side, end + side, end - side], 0.02);
                    c.fill();
                }
            }
        }
    }
}

fn draw_projectiles(g: &Game, cm: &Camera) {
    let c = &g.ctx;
    let s = cm.zoom as f64;
    for p in &g.projs {
        let spec = combat::ALL_PROJECTILES[(p.l as usize).min(2)];
        let pos = Vec2::new(p.x, p.y);
        let dir = Vec2::new(p.dx, p.dy).normalized();
        let (x, y) = proj(cm, pos, p.z);
        let (gx, gy) = proj(cm, pos, 0.0);
        let r = spec.radius as f64 * M as f64 * s;
        let (shell, core) = match p.l { 0 => ("rgba(220,40,50,0.6)", "#ffd0a0"), 1 => ("rgba(170,110,255,0.6)", "#f0e0ff"), _ => ("rgba(255,110,30,0.65)", "#fff0c0") };
        c.set_fill_style_str("rgba(0,0,0,0.3)");
        c.begin_path(); c.ellipse(gx, gy, r, r * 0.4, 0.0, 0.0, TAU).ok(); c.fill();
        let (bx, by) = proj(cm, pos - dir * 0.7, p.z);
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
    for e in g.ents.values() {
        let kind = Kind::from_u8(e.net.k);
        let state = State::from_u8(e.net.s);
        if state == State::Dead || e.net.r > 0.0 || e.net.d > 0.0 { continue; }
        let (x, y) = proj(cm, e.pos, kind.stats().height + if kind == Kind::Knight { 0.7 } else { 0.5 });
        if kind == Kind::Knight {
            if e.net.u != g.me {
                c.set_font(&format!("bold {:.0}px Georgia, serif", 12.0 * s.max(1.0)));
                c.set_text_align("center");
                let label = format!("{}  Lv {}", e.net.n.clone().unwrap_or_default(), e.net.lv);
                c.set_fill_style_str("rgba(0,0,0,0.75)"); c.fill_text(&label, x + 1.0, y + 1.0).ok();
                c.set_fill_style_str("#e8dcc0"); c.fill_text(&label, x, y).ok();
                let bw = 34.0 * s;
                c.set_fill_style_str("rgba(10,8,8,0.8)"); c.fill_rect(x - bw / 2.0, y + 4.0, bw, 4.0);
                c.set_fill_style_str("#4caf50"); c.fill_rect(x - bw / 2.0, y + 4.0, bw * (e.net.hp / e.net.mhp.max(1.0)) as f64, 4.0);
                c.set_text_align("start");
            }
            continue;
        }
        if let Some((atk, Phase::Windup)) = attack_of(e) {
            let k = (e.net.t / atk.windup).clamp(0.0, 1.0) as f64;
            let size = (16.0 + 8.0 * k) * s.max(1.0);
            c.set_font(&format!("bold {size:.0}px Georgia, serif"));
            c.set_text_align("center");
            c.set_fill_style_str("rgba(0,0,0,0.7)"); c.fill_text(atk.tell, x + 2.0, y - 16.0).ok();
            c.set_fill_style_str(if atk.is_low() { "#ffd166" } else { "#ff5a4a" }); c.fill_text(atk.tell, x, y - 18.0).ok();
            c.set_text_align("start");
        }
        if kind == Kind::Bellwarden { continue; }
        let bw = 40.0 * s;
        let mhp = e.net.mhp.max(1.0);
        c.set_fill_style_str("rgba(10,8,8,0.8)"); c.fill_rect(x - bw / 2.0, y, bw, 5.0);
        c.set_fill_style_str("rgba(240,220,200,0.6)"); c.fill_rect(x - bw / 2.0, y, bw * (e.net.ch / mhp) as f64, 5.0);
        c.set_fill_style_str("#c0392b"); c.fill_rect(x - bw / 2.0, y, bw * (e.net.hp / mhp) as f64, 5.0);
        c.set_fill_style_str("#d9b86c"); c.fill_rect(x - bw / 2.0, y + 6.0, bw * e.net.po as f64, 1.5);
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
    if g.world == Some(WorldKind::Cathedral) {
        for at in [NAVE_ALTAR, BELFRY_ALTAR] {
            if g.pred.distance(at) < ALTAR_REACH {
                let (x, y) = proj(cm, at, 2.4);
                c.set_font("bold 16px Georgia, serif"); c.set_text_align("center");
                c.set_fill_style_str("#f3e3c0"); c.fill_text("[E] Rest at the Vigil Altar", x, y).ok();
                c.set_text_align("start");
            }
        }
    }
    if g.world == Some(WorldKind::Concourse) {
        let (x, y) = proj(cm, MEMORIAL_POS, 3.6);
        c.set_font(&format!("bold {:.0}px Georgia, serif", 13.0 * s.max(1.0))); c.set_text_align("center");
        c.set_fill_style_str("#c9b896"); c.fill_text("Memorial Hall", x, y).ok();
        c.set_text_align("start");
    }
}

// --------------------------------------------------------------- HUD ----

fn draw_lighting(g: &Game) {
    let c = &g.ctx;
    let (w, h) = (g.canvas.width() as f64, g.canvas.height() as f64);
    // Biome mood: the godlands are smoky red, the highlands dim; the shore is bright.
    if let (Some(WorldKind::Realm), Some(m)) = (g.world, &g.map) {
        let tint = match m.band_at(g.pred) { 4 => Some("rgba(60,12,6,0.30)"), 3 => Some("rgba(20,14,10,0.18)"), 2 => Some("rgba(4,14,8,0.10)"), _ => None };
        if let Some(t) = tint { c.set_fill_style_str(t); c.fill_rect(0.0, 0.0, w, h); }
    }
    let strength = match g.world { Some(WorldKind::Cathedral) => 0.75, Some(WorldKind::Concourse) => 0.5, _ => 0.45 };
    if let Ok(grad) = c.create_radial_gradient(w / 2.0, h * 0.58, h * 0.3, w / 2.0, h * 0.58, h.max(w) * 0.75) {
        grad.add_color_stop(0.0, "rgba(0,0,0,0)").ok();
        grad.add_color_stop(1.0, &format!("rgba(0,0,0,{strength})")).ok();
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

fn draw_status(g: &Game, msg: &str) {
    let c = &g.ctx;
    let (w, h) = (g.canvas.width() as f64, g.canvas.height() as f64);
    c.set_fill_style_str("rgba(0,0,0,0.55)"); c.fill_rect(0.0, h * 0.45, w, h * 0.1);
    c.set_font("22px Georgia, serif"); c.set_text_align("center");
    c.set_fill_style_str("#e6d7b5"); c.fill_text(msg, w / 2.0, h * 0.51).ok();
    c.set_text_align("start");
}

fn draw_hud(g: &Game) {
    let c = &g.ctx;
    let (w, h) = (g.canvas.width() as f64, g.canvas.height() as f64);
    let ui = (h / 900.0).clamp(0.8, 2.0);
    let Some(me) = g.my_ent() else { return };
    let x0 = 28.0 * ui; let y0 = 26.0 * ui;
    let mhp = me.net.mhp.max(1.0);
    c.set_font(&format!("bold {:.0}px Georgia, serif", 15.0 * ui));
    c.set_fill_style_str("#e8dcc0");
    c.fill_text(&format!("{} · Warrior · Level {}", g.me_net.name, g.me_net.level), x0, y0).ok();
    bar(c, x0, y0 + 10.0 * ui, 300.0 * ui, 14.0 * ui, me.net.hp / mhp, me.net.ch / mhp, "#a8231f");
    bar(c, x0, y0 + 34.0 * ui, 240.0 * ui, 9.0 * ui, g.me_net.stamina / 100.0, g.me_net.stamina / 100.0, "#4f8a3a");
    let xp = if g.me_net.next > 0 { g.me_net.xp as f32 / g.me_net.next as f32 } else { 1.0 };
    bar(c, x0, y0 + 52.0 * ui, 240.0 * ui, 5.0 * ui, xp, xp, "#d9b45a");
    c.set_font(&format!("{:.0}px Georgia, serif", 13.0 * ui));
    c.set_fill_style_str("#d8cbb0");
    c.fill_text(&format!("{:.0} / {:.0}", me.net.hp.max(0.0), mhp), x0 + 308.0 * ui, y0 + 22.0 * ui).ok();
    for i in 0..3u8 {
        let fx = x0 + i as f64 * 22.0 * ui; let fy = y0 + 66.0 * ui;
        c.set_fill_style_str(if i < g.me_net.flasks { "#ff8c2a" } else { "#3a2f28" });
        c.begin_path(); c.arc(fx + 8.0 * ui, fy + 10.0 * ui, 7.0 * ui, 0.0, TAU).ok(); c.fill();
        c.fill_rect(fx + 5.0 * ui, fy, 6.0 * ui, 5.0 * ui);
    }
    c.set_fill_style_str("#d8cbb0");
    c.fill_text("Ember Flasks [Q]", x0 + 72.0 * ui, y0 + 80.0 * ui).ok();

    // Where you are.
    let place = match (g.world, &g.map) {
        (Some(WorldKind::Concourse), _) => "Eternal Concourse — safe zone".to_string(),
        (Some(WorldKind::Realm), Some(m)) => format!("The Ashen Realm — {}", band_name(m.band_at(g.pred))),
        (Some(WorldKind::Cathedral), _) => {
            let st = g.stage.as_str();
            if let Some(n) = st.strip_prefix("wave:") { format!("Cathedral of the Vigil — wave {} of 3", n.parse::<u32>().unwrap_or(0) + 1) }
            else if st == "gate" { "Cathedral of the Vigil — the belfry gate is open".into() }
            else if st == "victory" { "Cathedral of the Vigil — the bell is silent".into() }
            else { "The Belfry".into() }
        }
        _ => String::new(),
    };
    c.set_text_align("right");
    c.set_fill_style_str("#c9b896");
    let mm = if g.minimap.is_some() { 200.0 * ui } else { 0.0 };
    c.fill_text(&place, w - 28.0 * ui, 34.0 * ui + mm).ok();
    c.fill_text(&format!("{} online", g.online), w - 28.0 * ui, 52.0 * ui + mm).ok();
    c.set_text_align("start");

    // Realm minimap.
    if let (Some(mini), Some(map)) = (&g.minimap, &g.map) {
        let size = 180.0 * ui;
        let (mx, my) = (w - size - 28.0 * ui, 16.0 * ui);
        c.set_fill_style_str("rgba(0,0,0,0.6)"); c.fill_rect(mx - 3.0, my - 3.0, size + 6.0, size + 6.0);
        c.set_global_alpha(0.92);
        c.draw_image_with_html_canvas_element_and_dw_and_dh(mini, mx, my, size, size).ok();
        c.set_global_alpha(1.0);
        let k = size / map.w as f64;
        for e in g.ents.values() {
            if Kind::from_u8(e.net.k) != Kind::Knight { continue; }
            c.set_fill_style_str(if e.net.u == g.me { "#ffffff" } else { "#ffd166" });
            c.fill_rect(mx + e.pos.x as f64 * k - 2.0, my + e.pos.y as f64 * k - 2.0, 4.0, 4.0);
        }
        for p in &g.portals {
            c.set_fill_style_str("#ff8a5a");
            c.fill_rect(mx + p.x as f64 * k - 2.5, my + p.y as f64 * k - 2.5, 5.0, 5.0);
        }
        c.set_stroke_style_str("#6b5a44"); c.stroke_rect(mx - 3.0, my - 3.0, size + 6.0, size + 6.0);
    }

    if g.help {
        let lines = ["WASD move · Mouse aim", "LMB / J  light (20)   RMB / K  heavy (35)", "Shift  roll (25) · Space  jump LOW attacks",
            "Q  Ember Flask · E  portal / rest", "R  return to the Concourse", "Death is permanent · H hide help"];
        c.set_font(&format!("{:.0}px Georgia, serif", 12.0 * ui));
        c.set_fill_style_str("rgba(10,8,8,0.6)");
        c.fill_rect(w - 330.0 * ui, h - 120.0 * ui, 310.0 * ui, 104.0 * ui);
        c.set_fill_style_str("#b8a888");
        for (i, l) in lines.iter().enumerate() { c.fill_text(l, w - 320.0 * ui, h - 100.0 * ui + i as f64 * 15.0 * ui).ok(); }
    }

    // Memorial Hall panel when standing at the obelisk.
    if g.world == Some(WorldKind::Concourse) && g.pred.distance(MEMORIAL_POS) < 3.0 {
        let pw = 320.0 * ui; let ph = (60.0 + g.memorial.len().min(12) as f64 * 18.0) * ui;
        let (px, py) = ((w - pw) / 2.0, h * 0.18);
        c.set_fill_style_str("rgba(12,10,10,0.88)"); c.fill_rect(px, py, pw, ph);
        c.set_stroke_style_str("#6b5a44"); c.stroke_rect(px, py, pw, ph);
        c.set_font(&format!("bold {:.0}px Georgia, serif", 17.0 * ui)); c.set_text_align("center");
        c.set_fill_style_str("#e6d7b5"); c.fill_text("The Fallen", px + pw / 2.0, py + 26.0 * ui).ok();
        c.set_font(&format!("{:.0}px Georgia, serif", 13.0 * ui));
        if g.memorial.is_empty() { c.set_fill_style_str("#a89878"); c.fill_text("No names are carved here yet.", px + pw / 2.0, py + 48.0 * ui).ok(); }
        for (i, f) in g.memorial.iter().take(12).enumerate() {
            c.set_fill_style_str("#c9b896");
            c.fill_text(&format!("{} — level {} — {}", f.name, f.level, f.place), px + pw / 2.0, py + (50.0 + i as f64 * 18.0) * ui).ok();
        }
        c.set_text_align("start");
    }

    if let Some(b) = g.ents.values().find(|e| Kind::from_u8(e.net.k) == Kind::Bellwarden && e.net.d <= 0.0) {
        let bw = (w * 0.56).min(900.0 * ui);
        let bx = (w - bw) / 2.0; let by = h - 70.0 * ui;
        c.set_font(&format!("{:.0}px Georgia, serif", 18.0 * ui));
        c.set_fill_style_str("#e6d7b5");
        c.fill_text(if g.phase2 { "The Bellwarden — Grave Toll" } else { "The Bellwarden" }, bx, by - 10.0 * ui).ok();
        bar(c, bx, by, bw, 12.0 * ui, b.net.hp / b.net.mhp.max(1.0), b.net.ch / b.net.mhp.max(1.0), "#8e1b17");
    }

    if let Some((msg, t)) = &g.banner {
        let a = t.clamp(0.0, 1.0) as f64;
        c.set_font(&format!("{:.0}px Georgia, serif", 32.0 * ui));
        c.set_text_align("center");
        c.set_fill_style_str(&format!("rgba(235,215,170,{a:.2})"));
        c.fill_text(msg, w / 2.0, h * 0.32).ok();
        c.set_text_align("start");
    }

    if let Some((name, level, place)) = &g.died {
        c.set_fill_style_str("rgba(0,0,0,0.78)"); c.fill_rect(0.0, 0.0, w, h);
        c.set_text_align("center");
        c.set_font(&format!("{:.0}px Georgia, serif", 54.0 * ui));
        c.set_fill_style_str("#a81e19"); c.fill_text(&format!("{name} HAS FALLEN"), w / 2.0, h * 0.42).ok();
        c.set_font(&format!("{:.0}px Georgia, serif", 20.0 * ui));
        c.set_fill_style_str("#e6d7b5");
        c.fill_text(&format!("Level {level} · {place}"), w / 2.0, h * 0.50).ok();
        c.fill_text("Their name is carved in the Memorial Hall of the Eternal Concourse.", w / 2.0, h * 0.55).ok();
        c.set_fill_style_str("#a89878");
        c.fill_text("Press any key to rise again as a new recruit", w / 2.0, h * 0.62).ok();
        c.set_text_align("start");
    }
}
