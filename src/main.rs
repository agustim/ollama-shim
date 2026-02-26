mod config;
mod proxy;
mod state;

use axum::{Router, routing::any};
use axum_server::Server;
use clap::Parser;

use crate::config::{AppConfig, ConfigOverrides};
use crate::proxy::proxy_handler;
use crate::state::AppState;

/// Command-line options.  Values provided here take precedence over
/// environment variables handled by `AppConfig::load`.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Opt {
    /// Base URL for the Ollama service (overrides OLLAMA_URL).
    #[arg(long)]
    ollama_url: Option<String>,

    /// comma-separated list of API keys (overrides all other sources)
    #[arg(long, value_delimiter = ',')]
    api_keys: Option<Vec<String>>,

    /// path to a newline-/comma-separated file containing keys
    #[arg(long)]
    api_keys_file: Option<String>,

    /// path to sqlite database with table `api_keys(key TEXT)`
    #[arg(long)]
    api_keys_sqlite: Option<String>,

    /// IP address to bind the proxy to (overrides PROXY_HOST).
    #[arg(long)]
    proxy_host: Option<String>,

    /// Port to bind the proxy to (overrides PROXY_PORT).
    #[arg(long)]
    proxy_port: Option<u16>,
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opt_parsing() {
        let opts = Opt::parse_from([
            "prog",
            "--ollama-url",
            "http://example",
            "--api-keys",
            "a,b,c",
            "--api-keys-file",
            "/tmp/k",
            "--api-keys-sqlite",
            "/tmp/db",
            "--proxy-host",
            "1.2.3.4",
            "--proxy-port",
            "5555",
        ]);
        assert_eq!(opts.ollama_url.as_deref(), Some("http://example"));
        assert_eq!(opts.api_keys.as_ref().map(|v| v.as_slice()), Some(&["a".to_string(),"b".to_string(),"c".to_string()][..]));
        assert_eq!(opts.api_keys_file.as_deref(), Some("/tmp/k"));
        assert_eq!(opts.api_keys_sqlite.as_deref(), Some("/tmp/db"));
        assert_eq!(opts.proxy_host.as_deref(), Some("1.2.3.4"));
        assert_eq!(opts.proxy_port, Some(5555));
    }
}

#[tokio::main]
async fn main() {
    let opts = Opt::parse();

    let mut config = AppConfig::load().expect("failed to load configuration");
    let overrides = ConfigOverrides {
        ollama_url: opts.ollama_url,
        proxy_host: opts.proxy_host,
        proxy_port: opts.proxy_port,
        api_keys_sqlite: opts.api_keys_sqlite,
        api_keys_file: opts.api_keys_file,
        api_keys: opts.api_keys,
    };
    config.apply_overrides(&overrides).expect("failed to apply overrides");

    let state = AppState::new(&config);

    let app = Router::new()
        .route("/v1/{*path}", any(proxy_handler))
        .with_state(state);

    let addr = config.proxy_addr;
    println!("Listening on {}", addr);
    Server::bind(addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
