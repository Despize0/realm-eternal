//! Procedural pixel-art textures, terrain chunks and prop sprites.
//! Everything is generated once in Rust pixel buffers and uploaded to offscreen canvases.

use realm_shared::world::{hash01, value_noise, PropKind, Tile, TileMap};
use std::collections::HashMap;
use wasm_bindgen::{Clamped, JsCast};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

pub const TEX: usize = 128;
/// Ground pixels per metre (chunks are drawn with smoothing off for crisp pixels).
pub const PPM: usize = 16;
pub const CHUNK_M: i32 = 16;
pub const CHUNK_PX: usize = CHUNK_M as usize * PPM;

pub fn canvas(w: u32, h: u32) -> (HtmlCanvasElement, CanvasRenderingContext2d) {
    let doc = web_sys::window().unwrap().document().unwrap();
    let c: HtmlCanvasElement = doc.create_element("canvas").unwrap().dyn_into().unwrap();
    c.set_width(w); c.set_height(h);
    let ctx: CanvasRenderingContext2d = c.get_context("2d").unwrap().unwrap().dyn_into().unwrap();
    ctx.set_image_smoothing_enabled(false);
    (c, ctx)
}

pub fn upload(rgba: &[u8], w: u32, h: u32) -> HtmlCanvasElement {
    let (c, ctx) = canvas(w, h);
    if let Ok(img) = ImageData::new_with_u8_clamped_array_and_sh(Clamped(rgba), w, h) { ctx.put_image_data(&img, 0.0, 0.0).ok(); }
    c
}

/// Tileable value noise: the lattice wraps every `period` cells.
fn pnoise(x: f32, y: f32, period: i32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - xi as f32, y - yi as f32);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let h = |a: i32, b: i32| hash01(a.rem_euclid(period), b.rem_euclid(period), seed);
    let top = h(xi, yi) + (h(xi + 1, yi) - h(xi, yi)) * sx;
    let bot = h(xi, yi + 1) + (h(xi + 1, yi + 1) - h(xi, yi + 1)) * sx;
    top + (bot - top) * sy
}
fn pfbm(u: f32, v: f32, base: i32, seed: u32, oct: u32) -> f32 {
    let (mut s, mut a, mut n, mut p) = (0.0, 0.5, 0.0, base);
    for o in 0..oct {
        let f = p as f32;
        s += pnoise(u * f, v * f, p, seed + o * 31) * a;
        n += a; a *= 0.5; p *= 2;
    }
    s / n
}

#[derive(Clone)]
pub struct Texture { pub px: Vec<[u8; 3]> }

fn quant(c: f32, levels: f32) -> f32 { (c * levels).round() / levels }

fn make_tex(f: impl Fn(f32, f32, usize, usize) -> [f32; 3]) -> Texture {
    let mut px = vec![[0u8; 3]; TEX * TEX];
    for y in 0..TEX { for x in 0..TEX {
        let (u, v) = (x as f32 / TEX as f32, y as f32 / TEX as f32);
        let c = f(u, v, x, y);
        px[y * TEX + x] = [c[0].clamp(0.0, 255.0) as u8, c[1].clamp(0.0, 255.0) as u8, c[2].clamp(0.0, 255.0) as u8];
    }}
    Texture { px }
}

fn mul(c: [f32; 3], k: f32) -> [f32; 3] { [c[0] * k, c[1] * k, c[2] * k] }
fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] { [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t] }

pub struct Textures { pub by_tile: HashMap<u8, Texture>, pub water: HtmlCanvasElement, pub stone_canvas: HtmlCanvasElement }

pub fn build_textures() -> Textures {
    let grass = make_tex(|u, v, x, y| {
        let n = pfbm(u, v, 4, 11, 4);
        let base = mix([46.0, 82.0, 38.0], [86.0, 128.0, 52.0], quant(n, 5.0));
        let blade = hash01(x as i32, y as i32 / 2, 5);
        if blade > 0.93 { mul(base, 1.25) } else if blade < 0.06 { mul(base, 0.72) } else { base }
    });
    let meadow = make_tex(|u, v, x, y| {
        let n = pfbm(u, v, 4, 13, 4);
        let base = mix([78.0, 118.0, 50.0], [128.0, 150.0, 64.0], quant(n, 5.0));
        let f = hash01(x as i32, y as i32, 21);
        if f > 0.994 { [235.0, 225.0, 170.0] } else if f > 0.988 { [200.0, 140.0, 210.0] } else if f < 0.05 { mul(base, 0.8) } else { base }
    });
    let forest = make_tex(|u, v, x, y| {
        let n = pfbm(u, v, 4, 17, 4);
        let litter = pfbm(u, v, 8, 19, 2);
        let base = mix([28.0, 48.0, 28.0], [52.0, 76.0, 38.0], quant(n, 4.0));
        let base = if litter > 0.62 { mix(base, [78.0, 58.0, 34.0], 0.6) } else { base };
        if hash01(x as i32, y as i32, 23) < 0.05 { mul(base, 0.7) } else { base }
    });
    let sand = make_tex(|u, v, x, y| {
        let n = pfbm(u, v, 4, 29, 3);
        let ripple = ((v * 6.0 + n * 1.5) * std::f32::consts::TAU).sin() * 0.5 + 0.5;
        let base = mix([196.0, 170.0, 118.0], [222.0, 202.0, 150.0], quant(n * 0.6 + ripple * 0.4, 4.0));
        let g = hash01(x as i32, y as i32, 31);
        if g > 0.97 { mul(base, 0.85) } else if g < 0.02 { [150.0, 136.0, 110.0] } else { base }
    });
    let highland = make_tex(|u, v, x, y| {
        let n = pfbm(u, v, 4, 37, 4);
        let pebble = pnoise(u * 24.0, v * 24.0, 24, 41);
        let base = mix([96.0, 84.0, 66.0], [134.0, 120.0, 96.0], quant(n, 4.0));
        let base = if pebble > 0.72 { mix(base, [150.0, 146.0, 140.0], 0.55) } else if pebble < 0.2 { mul(base, 0.82) } else { base };
        if hash01(x as i32, y as i32, 43) < 0.04 { mul(base, 0.75) } else { base }
    });
    let rock = make_tex(|u, v, _, _| {
        let n = pfbm(u, v, 4, 47, 4);
        let crack = (pfbm(u, v, 8, 53, 2) - 0.5).abs();
        let base = mix([80.0, 80.0, 86.0], [124.0, 122.0, 126.0], quant(n, 4.0));
        if crack < 0.02 { mul(base, 0.55) } else { base }
    });
    let ash = make_tex(|u, v, x, y| {
        let n = pfbm(u, v, 4, 59, 4);
        let crack = (pfbm(u, v, 4, 61, 3) - 0.5).abs();
        let base = mix([44.0, 40.0, 40.0], [74.0, 66.0, 62.0], quant(n, 4.0));
        if crack < 0.012 { [150.0, 58.0, 26.0] } else if crack < 0.03 { mul(base, 0.6) }
        else if hash01(x as i32, y as i32, 67) > 0.997 { [230.0, 120.0, 40.0] } else { base }
    });
    let stone = make_tex(|u, v, x, y| {
        // Cut slabs with mortar joints, each slab with its own tint.
        let slab = 32;
        let row = y / slab;
        let off = if row % 2 == 0 { 0 } else { slab / 2 };
        let sx = (x + off) % TEX / slab; let lx = (x + off) % slab; let ly = y % slab;
        let tint = hash01(sx as i32, row as i32, 71) * 0.18 + 0.91;
        let n = pfbm(u, v, 8, 73, 3);
        let base = mul(mix([88.0, 86.0, 82.0], [118.0, 114.0, 106.0], quant(n, 4.0)), tint);
        if lx == 0 || ly == 0 { [34.0, 32.0, 30.0] }
        else if lx == 1 || ly == 1 { mul(base, 1.18) }
        else if lx == slab - 1 || ly == slab - 1 { mul(base, 0.78) }
        else if hash01(x as i32, y as i32, 79) < 0.03 { mul(base, 0.8) } else { base }
    });
    let water = make_tex(|u, v, _, _| {
        let n = pfbm(u, v, 4, 83, 3);
        let caustic = 1.0 - (pfbm(u, v, 4, 89, 3) - 0.5).abs() * 2.0;
        let base = mix([28.0, 70.0, 108.0], [44.0, 96.0, 138.0], quant(n, 4.0));
        if caustic > 0.93 { mix(base, [150.0, 200.0, 220.0], 0.55) } else { base }
    });
    let mut by_tile = HashMap::new();
    by_tile.insert(Tile::Grass as u8, grass);
    by_tile.insert(Tile::Meadow as u8, meadow);
    by_tile.insert(Tile::Forest as u8, forest);
    by_tile.insert(Tile::Sand as u8, sand);
    by_tile.insert(Tile::Highland as u8, highland);
    by_tile.insert(Tile::Rock as u8, rock);
    by_tile.insert(Tile::Ash as u8, ash);
    by_tile.insert(Tile::Stone as u8, stone.clone());
    by_tile.insert(Tile::StoneWall as u8, stone.clone());
    let to_canvas = |t: &Texture| {
        let mut rgba = Vec::with_capacity(TEX * TEX * 4);
        for p in &t.px { rgba.extend_from_slice(&[p[0], p[1], p[2], 255]); }
        upload(&rgba, TEX as u32, TEX as u32)
    };
    let water_canvas = to_canvas(&water);
    let stone_canvas = to_canvas(&stone);
    Textures { by_tile, water: water_canvas, stone_canvas }
}

/// Bakes one terrain chunk. Tile edges are warped by noise so biomes blend organically;
/// water pixels are left transparent so the animated water layer shows through.
pub fn bake_chunk(map: &TileMap, tex: &Textures, cx: i32, cy: i32) -> HtmlCanvasElement {
    let mut rgba = vec![0u8; CHUNK_PX * CHUNK_PX * 4];
    let inv = 1.0 / PPM as f32;
    let ox = (cx * CHUNK_M) as f32; let oy = (cy * CHUNK_M) as f32;
    let is_water = |t: Tile| matches!(t, Tile::Water | Tile::DeepWater);
    for py in 0..CHUNK_PX { for px in 0..CHUNK_PX {
        let wx = ox + (px as f32 + 0.5) * inv;
        let wy = oy + (py as f32 + 0.5) * inv;
        let warp_x = (value_noise(wx * 0.9, wy * 0.9, 7) - 0.5) * 0.8 + (value_noise(wx * 3.1, wy * 3.1, 9) - 0.5) * 0.25;
        let warp_y = (value_noise(wx * 0.9 + 40.0, wy * 0.9, 8) - 0.5) * 0.8 + (value_noise(wx * 3.1, wy * 3.1 + 17.0, 10) - 0.5) * 0.25;
        let (sx, sy) = (wx + warp_x, wy + warp_y);
        let t = map.get(sx.floor() as i32, sy.floor() as i32);
        let i = (py * CHUNK_PX + px) * 4;
        if is_water(t) || t == Tile::Void {
            // Deep water gets a dark wash over the animated water layer.
            if t == Tile::DeepWater { rgba[i..i + 4].copy_from_slice(&[6, 18, 40, 110]); }
            else if t == Tile::Void { rgba[i..i + 4].copy_from_slice(&[4, 6, 10, 255]); }
            continue;
        }
        let tx = ((wx * PPM as f32) as i64).rem_euclid(TEX as i64) as usize;
        let ty = ((wy * PPM as f32) as i64).rem_euclid(TEX as i64) as usize;
        let tile_key = if t == Tile::StoneWall { Tile::Stone as u8 } else { t as u8 };
        let Some(texture) = tex.by_tile.get(&tile_key) else { continue };
        let c = texture.px[ty * TEX + tx];
        // Broad light variation (cloud shadow / terrain undulation), quantised for a painted look.
        let shade = 0.86 + quant(realm_shared::world::fbm(wx / 14.0, wy / 14.0, 3, 3), 4.0) * 0.26;
        let mut col = [c[0] as f32 * shade, c[1] as f32 * shade, c[2] as f32 * shade];
        // Wet sand and foam along the shoreline.
        if t == Tile::Sand {
            let near = [(0.45, 0.0), (-0.45, 0.0), (0.0, 0.45), (0.0, -0.45)].iter().any(|(dx, dy)| is_water(map.get((sx + dx).floor() as i32, (sy + dy).floor() as i32)));
            if near {
                col = mul(col, 0.78);
                let edge = [(0.18, 0.0), (-0.18, 0.0), (0.0, 0.18), (0.0, -0.18)].iter().any(|(dx, dy)| is_water(map.get((sx + dx).floor() as i32, (sy + dy).floor() as i32)));
                if edge && hash01(px as i32 / 2, py as i32 / 2, cx as u32 ^ (cy as u32) << 8) > 0.35 { col = [228.0, 236.0, 236.0]; }
            }
        }
        rgba[i] = col[0].clamp(0.0, 255.0) as u8;
        rgba[i + 1] = col[1].clamp(0.0, 255.0) as u8;
        rgba[i + 2] = col[2].clamp(0.0, 255.0) as u8;
        rgba[i + 3] = 255;
    }}
    upload(&rgba, CHUNK_PX as u32, CHUNK_PX as u32)
}

// ------------------------------------------------------------ props ----

pub struct PropSprite { pub canvas: HtmlCanvasElement, pub w: f64, pub h: f64, pub anchor_y: f64 }

fn put(buf: &mut [u8], w: usize, x: i32, y: i32, c: [u8; 4]) {
    if x < 0 || y < 0 || x as usize >= w || y as usize >= buf.len() / 4 / w { return; }
    let i = (y as usize * w + x as usize) * 4;
    buf[i..i + 4].copy_from_slice(&c);
}

/// Shade a filled blob with light from the upper left, in a few hard steps, with a dark outline.
fn shade_blob(buf: &mut [u8], w: usize, h: usize, inside: &dyn Fn(f32, f32) -> Option<(f32, f32)>, pal: &[[u8; 3]; 4], outline: [u8; 3], seed: u32) {
    let mut mask = vec![false; w * h];
    let mut col = vec![[0u8; 4]; w * h];
    for y in 0..h { for x in 0..w {
        if let Some((nx, ny)) = inside(x as f32 + 0.5, y as f32 + 0.5) {
            mask[y * w + x] = true;
            let light = (-(nx * -0.6 + ny * -0.8)).clamp(-1.0, 1.0) * -1.0;
            let jitter = (hash01(x as i32, y as i32, seed) - 0.5) * 0.35;
            let k = ((light + jitter) * 0.5 + 0.5).clamp(0.0, 0.999);
            let c = pal[(k * 4.0) as usize];
            col[y * w + x] = [c[0], c[1], c[2], 255];
        }
    }}
    for y in 0..h { for x in 0..w {
        if !mask[y * w + x] { continue; }
        let edge = [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)].iter().any(|(dx, dy)| {
            let (nx, ny) = (x as i32 + dx, y as i32 + dy);
            nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 || !mask[ny as usize * w + nx as usize]
        });
        let c = if edge { [outline[0], outline[1], outline[2], 255] } else { col[y * w + x] };
        put(buf, w, x as i32, y as i32, c);
    }}
}

pub fn build_props() -> HashMap<(u8, u8), PropSprite> {
    let mut out = HashMap::new();
    for v in 0..3u8 {
        // Broadleaf tree: clustered canopy over a trunk.
        let (w, h) = (40usize, 52usize);
        let mut buf = vec![0u8; w * h * 4];
        for y in 34..50 { for x in 18..23 { let c = if x == 18 { [52, 34, 22, 255] } else if x == 22 { [40, 26, 18, 255] } else { [86, 58, 36, 255] }; put(&mut buf, w, x, y, c); } }
        let blobs: Vec<(f32, f32, f32)> = (0..7).map(|k| {
            let a = k as f32 / 7.0 * std::f32::consts::TAU + v as f32;
            let r = if k == 0 { 0.0 } else { 7.0 + hash01(k, v as i32, 3) * 3.0 };
            (20.0 + a.cos() * r, 20.0 + a.sin() * r * 0.8, if k == 0 { 12.0 } else { 7.0 + hash01(k, v as i32, 5) * 3.0 })
        }).collect();
        let pal = if v == 2 { [[26, 52, 30], [40, 78, 38], [64, 108, 46], [104, 146, 62]] } else { [[22, 46, 24], [34, 70, 32], [56, 100, 42], [92, 138, 56]] };
        shade_blob(&mut buf, w, h, &|x, y| {
            let mut best: Option<(f32, f32)> = None;
            let mut bestd = f32::MAX;
            for &(cx, cy, r) in &blobs {
                let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
                if d < r && d - r < bestd { bestd = d - r; best = Some(((x - cx) / r, (y - cy) / r)); }
            }
            if y > 36.0 { None } else { best }
        }, &pal, [14, 26, 16], 100 + v as u32);
        out.insert((PropKind::Tree as u8, v), PropSprite { canvas: upload(&buf, w as u32, h as u32), w: w as f64, h: h as f64, anchor_y: 49.0 });

        // Pine: stacked tiers.
        let (w, h) = (32usize, 60usize);
        let mut buf = vec![0u8; w * h * 4];
        for y in 46..58 { for x in 14..18 { put(&mut buf, w, x, y, if x == 14 { [50, 32, 22, 255] } else { [80, 54, 34, 255] }); } }
        let pal = [[16, 40, 30], [26, 60, 40], [40, 84, 50], [66, 112, 64]];
        shade_blob(&mut buf, w, h, &|x, y| {
            for tier in 0..4 {
                let top = 4.0 + tier as f32 * 10.0;
                let bottom = top + 16.0;
                if y >= top && y <= bottom {
                    let half = (y - top) / (bottom - top) * (8.0 + tier as f32 * 2.5 + v as f32);
                    if (x - 16.0).abs() <= half { return Some(((x - 16.0) / (half + 0.1), (y - top) / 16.0 - 0.6)); }
                }
            }
            None
        }, &pal, [10, 22, 18], 200 + v as u32);
        out.insert((PropKind::Pine as u8, v), PropSprite { canvas: upload(&buf, w as u32, h as u32), w: w as f64, h: h as f64, anchor_y: 57.0 });

        // Dead tree: bare branches in ash.
        let (w, h) = (36usize, 50usize);
        let mut buf = vec![0u8; w * h * 4];
        let mut branch = |x0: f32, y0: f32, x1: f32, y1: f32, th: f32| {
            for s in 0..60 {
                let t = s as f32 / 60.0;
                let (x, y) = (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
                let r = th * (1.0 - t * 0.6);
                for dy in -2..=2 { for dx in -2..=2 {
                    if ((dx * dx + dy * dy) as f32).sqrt() <= r { put(&mut buf, w, (x + dx as f32) as i32, (y + dy as f32) as i32, if dx < 0 { [38, 32, 30, 255] } else { [64, 54, 48, 255] }); }
                }}
            }
        };
        branch(18.0, 48.0, 18.0, 20.0, 2.2);
        branch(18.0, 30.0, 8.0 + v as f32, 14.0, 1.4);
        branch(18.0, 26.0, 29.0 - v as f32, 10.0, 1.4);
        branch(18.0, 21.0, 15.0, 4.0, 1.1);
        out.insert((PropKind::DeadTree as u8, v), PropSprite { canvas: upload(&buf, w as u32, h as u32), w: w as f64, h: h as f64, anchor_y: 47.0 });

        // Boulder.
        let (w, h) = (34usize, 26usize);
        let mut buf = vec![0u8; w * h * 4];
        let pal = [[58, 56, 60], [86, 84, 88], [116, 114, 116], [150, 148, 146]];
        let sq = 1.0 + v as f32 * 0.12;
        shade_blob(&mut buf, w, h, &|x, y| {
            let (dx, dy) = ((x - 17.0) / (15.0 / sq), (y - 15.0) / 10.0);
            let wob = (hash01((x / 3.0) as i32, (y / 3.0) as i32, 300 + v as u32) - 0.5) * 0.15;
            if dx * dx + dy * dy < 1.0 + wob { Some((dx, dy)) } else { None }
        }, &pal, [30, 28, 32], 400 + v as u32);
        out.insert((PropKind::Boulder as u8, v), PropSprite { canvas: upload(&buf, w as u32, h as u32), w: w as f64, h: h as f64, anchor_y: 22.0 });
    }
    out
}

/// A 1-pixel-per-tile minimap of the realm.
pub fn bake_minimap(map: &TileMap) -> HtmlCanvasElement {
    let mut rgba = vec![0u8; (map.w * map.h * 4) as usize];
    for y in 0..map.h { for x in 0..map.w {
        let c: [u8; 3] = match map.get(x, y) {
            Tile::DeepWater => [14, 30, 64], Tile::Water => [30, 66, 112], Tile::Sand => [200, 180, 126],
            Tile::Grass => [76, 122, 54], Tile::Meadow => [110, 140, 64], Tile::Forest => [34, 74, 38],
            Tile::Highland => [118, 104, 82], Tile::Rock => [80, 80, 86], Tile::Ash => [60, 54, 52],
            Tile::Stone => [110, 108, 104], Tile::StoneWall => [50, 50, 50], Tile::Void => [0, 0, 0],
        };
        let i = ((y * map.w + x) * 4) as usize;
        rgba[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
    }}
    upload(&rgba, map.w as u32, map.h as u32)
}
