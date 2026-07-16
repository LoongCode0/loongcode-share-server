mod auth;
mod cleanup;
mod config;
mod routes;
mod store;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = config::Config::from_env()?;
    let conn = store::open(&cfg.db_path)?;
    let state = routes::AppState {
        db: Arc::new(Mutex::new(conn)),
        cfg: Arc::new(cfg),
        ip_hits: Arc::new(Mutex::new(HashMap::new())),
    };
    cleanup::spawn(state.db.clone());
    let listener = tokio::net::TcpListener::bind(state.cfg.listen).await?;
    println!("share-server listening on {}", state.cfg.listen);
    let app = routes::router(state);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}
