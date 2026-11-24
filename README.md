# Realm Eternal

An original Rust/WASM multiplayer bullet-hell MMORPG vertical slice.

## Included

- Asymmetric three-quarter affine camera with the player below center
- WASD movement, mouse aiming, click/Space firing, wheel zoom
- Procedural pixel-art rendering with world-space projectile simulation
- Enemy waves, ranged enemies, boss phases, loot, XP, levels, HP and mana
- Rust authoritative WebSocket server protocol and movement validation
- Docker and Railway deployment configuration
- Unit tests for projection and movement validation

This is a playable foundation, not a completed production MMORPG. The full requested
content scope requires iterative development, authored art/audio, persistence, security
hardening, load testing, and balancing.

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

- WASD or arrow keys: move
- Mouse: aim
- Left click or Space: fire
- Mouse wheel: zoom
- R: restart after death

## Architecture

- `shared`: projection, protocol and simulation types shared by native and WASM
- `server`: Axum static server and authoritative WebSocket world
- `client`: Rust/WASM Canvas2D renderer and playable local simulation
- `web`: browser shell and generated WASM destination

Networking is wired and the server owns connected player movement. The vertical slice
keeps PvE simulation client-side so it can be played immediately; moving all enemy,
combat, loot and death decisions to the server is the first production milestone.
