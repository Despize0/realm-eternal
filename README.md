# Realm Eternal

A cooperative dark-fantasy MMORPG. Rust authoritative server, Rust/WASM client.

## World

- **Eternal Concourse:** the safe hub. The server blocks all damage there. It has the realm portal, a fountain, and the Memorial Hall listing fallen characters.
- **The Ashen Realm:** one shared open world (320 × 320 m island) generated from a seed on both server and client. Bands run from the Shore through the Lowlands, Midlands and Highlands to the Godlands at the centre, and foes get tougher further in. Experience is shared with every player nearby, up to level 20.
- **Cathedral of the Vigil:** realm foes sometimes drop a portal into a shared dungeon instance (up to 6 players) holding the Ashen Vigil encounter and the Bellwarden.
- **Permadeath:** dying outside the Concourse ends the character. Its name goes into the Memorial Hall and the player returns as a level-1 recruit.
- **Networking:** the server simulates every world at 60 Hz and sends each client an interest-managed snapshot (22 m radius) at 30 Hz. The client predicts only its own walking.

An original Rust/WASM multiplayer bullet-hell MMORPG vertical slice.

## Gameplay: the Ashen Vigil encounter

Realm Eternal's combat is ported from the Ashen Vigil prototype (Godot 4.7) into Rust.
The rules live in `shared/src/combat.rs` and run at a fixed 60 Hz tick in the browser.

- **Knight:** 100 HP, 100 stamina (regenerates 25/s, 0.7 s after the last spend, only while free or airborne), 20 poise.
- **Vigil Longsword:** Vigil Cut (light, 20 stamina, 18 dmg) and Ashfall Cleave (heavy, 35 stamina, 34 dmg).
- **Dodge roll:** 25 stamina, 0.40 s, i-frames 0.06-0.24 s. With no direction it is a backstep.
- **Jump:** hold for full height. Feet above 0.9 m clear every `LOW` attack.
- **Ember Flasks:** 3, heal 45 at 0.6 s into a 1.0 s drink. Clearing a wave gives one back.
- **The nave:** three waves (Husks, Duelists, Cultists, a Pyreguard). Dying restarts the current wave.
- **The belfry:** the gate opens after wave 3. Crossing in raises the mist and the Bellwarden (420 HP) drops in.
  Dying there resets only the boss. Phase 2 at half health opens with Grave Toll and swaps in new combos.
- **Vigil Altar:** rest with E. You cannot rest while the Bellwarden lives; resting after it falls resets the cathedral.
- **Fair telegraphs:** every enemy attack shows a tell word, a floor footprint or projectile lane, and a wind-up.
  Bellwarden combos end on 0.9 s+ recovery, wind up 0.6 s+, and come from a shuffled bag (see `validate_combos`).

## Local build

Install Rust, the wasm target, and wasm-bindgen-cli:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked
```

Build the browser client:

```bash
cargo build --release -p realm-client --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir web/pkg \
  target/wasm32-unknown-unknown/release/realm_client.wasm
```

Run the server:

```bash
cargo run -p realm-server
```

Open `http://localhost:8080` in Chrome.

## Railway

Create a Railway service from this repository. Railway uses the included Dockerfile.
The server reads `PORT` and serves both the site and `/ws` from one origin.

## Controls

- WASD / arrows: move · Mouse: aim
- Left click / J: light attack · Right click / K: heavy attack
- Shift: dodge roll · Space: jump (hold for height)
- Q: drink an Ember Flask · E: rest at the Vigil Altar
- Mouse wheel: zoom · H: toggle help · F5: restart the whole encounter

## Architecture

- `shared`: projection, protocol, and the combat simulation (`combat.rs`) shared by native and WASM
- `server`: Axum static server and authoritative WebSocket world
- `client`: Rust/WASM Canvas2D renderer and playable local simulation
- `web`: browser shell and generated WASM destination

Networking is wired and the server owns connected player movement. The vertical slice
keeps PvE simulation client-side so it can be played immediately; moving all enemy,
combat, loot and death decisions to the server is the first production milestone.
