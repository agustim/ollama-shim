mod config;
mod proxy;
mod state;

use axum::{Router, routing::any};
use axum_server::Server;
use std::net::SocketAddr;

use crate::config::AppConfig;
use crate::proxy::proxy_handler;
use crate::state::AppState;

#[tokio::main]
async fn main() {
    let config = AppConfig::load().expect("failed to load configuration");
    let state = AppState::new(&config);

    let app = Router::new()
        .route("/v1/{*path}", any(proxy_handler))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("Listening on {}", addr);
    Server::bind(addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
