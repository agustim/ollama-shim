use crate::config::AppConfig;
use reqwest::Client;

/// Shared state that is stored in `axum::Extension`/`State`.
#[derive(Clone)]
pub struct AppState {
    pub client: Client,
    pub valid_keys: Vec<String>,
    pub ollama_url: String,
}

impl AppState {
    pub fn new(cfg: &AppConfig) -> Self {
        AppState {
            client: Client::new(),
            valid_keys: cfg.valid_keys.clone(),
            ollama_url: cfg.ollama_url.clone(),
        }
    }
}
