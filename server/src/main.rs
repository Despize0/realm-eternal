use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use realm_shared::{
    validate_step, ClientMessage, PlayerSnapshot, ServerMessage, Vec2,
    PROTOCOL_VERSION, TICK_HZ,
};
use std::{collections::HashMap, env, sync::{Arc, atomic::{AtomicU64, Ordering}}};
use tokio::{sync::{mpsc, RwLock}, time::{self, Duration}};
use tower_http::{services::ServeDir, trace::TraceLayer};

#[derive(Clone)]
struct AppState {
    next_id: Arc<AtomicU64>,
    world: Arc<RwLock<World>>,
}

struct Player {
    name: String,
    position: Vec2,
    input: Vec2,
    acknowledged: u32,
    tx: mpsc::UnboundedSender<ServerMessage>,
}

#[derive(Default)]
struct World {
    tick: u64,
    players: HashMap<u64, Player>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let state = AppState {
        next_id: Arc::new(AtomicU64::new(1)),
        world: Arc::new(RwLock::new(World::default())),
    };
    tokio::spawn(world_loop(state.clone()));

    let static_dir = env::var("STATIC_DIR").unwrap_or_else(|_| "web".into());
    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/ws", get(ws_handler))
        .fallback_service(ServeDir::new(static_dir).append_index_html_on_directories(true))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let port: u16 = env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await.unwrap();
    tracing::info!(port, "Realm Eternal server listening");
    axum::serve(listener, app).await.unwrap();
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| session(socket, state)).into_response()
}

async fn session(socket: WebSocket, state: AppState) {
    let id = state.next_id.fetch_add(1, Ordering::Relaxed);
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMessage>();
    let writer = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if let Ok(text) = serde_json::to_string(&message) {
                if sink.send(Message::Text(text)).await.is_err() { break; }
            }
        }
    });

    tx.send(ServerMessage::Welcome { id, tick_hz: TICK_HZ }).ok();
    state.world.write().await.players.insert(id, Player {
        name: format!("Adventurer-{id}"),
        position: Vec2::new((id % 8) as f32 * 28.0, 0.0),
        input: Vec2::ZERO,
        acknowledged: 0,
        tx: tx.clone(),
    });

    while let Some(Ok(message)) = stream.next().await {
        let Message::Text(text) = message else { continue };
        let Ok(message) = serde_json::from_str::<ClientMessage>(&text) else { continue };
        match message {
            ClientMessage::Hello { version, name } => {
                if version != PROTOCOL_VERSION {
                    tx.send(ServerMessage::Error { message: "protocol mismatch".into() }).ok();
                    break;
                }
                if let Some(player) = state.world.write().await.players.get_mut(&id) {
                    player.name = name.chars().take(20).collect();
                }
            }
            ClientMessage::Input { sequence, dx, dy, .. } => {
                let mut world = state.world.write().await;
                if let Some(player) = world.players.get_mut(&id) {
                    player.input = Vec2::new(dx.clamp(-1.0, 1.0), dy.clamp(-1.0, 1.0));
                    player.acknowledged = sequence;
                }
            }
            ClientMessage::Ping { client_ms } => {
                tx.send(ServerMessage::Pong { client_ms }).ok();
            }
        }
    }
    state.world.write().await.players.remove(&id);
    writer.abort();
}

async fn world_loop(state: AppState) {
    let dt = 1.0 / TICK_HZ;
    let mut interval = time::interval(Duration::from_secs_f32(dt));
    loop {
        interval.tick().await;
        let mut world = state.world.write().await;
        world.tick += 1;
        for player in world.players.values_mut() {
            player.position = validate_step(player.position, player.input, 150.0, dt);
        }
        let snapshots: Vec<PlayerSnapshot> = world.players.iter().map(|(&id, p)| PlayerSnapshot {
            id, name: p.name.clone(), position: p.position, hp: 100,
        }).collect();
        for player in world.players.values() {
            player.tx.send(ServerMessage::Snapshot {
                tick: world.tick,
                acknowledged: player.acknowledged,
                players: snapshots.clone(),
            }).ok();
        }
    }
}
