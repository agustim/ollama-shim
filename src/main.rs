mod config;
mod proxy;
mod state;

use axum::{Router, routing::any};
use axum_server::Server;
use clap::{Parser, Subcommand};

use crate::config::{AppConfig, ConfigOverrides};
use crate::proxy::proxy_handler;
use crate::state::AppState;

/// Top-level CLI.  We support two modes of operation:
///
/// * `server` is the existing behaviour which spins up the proxy.
/// * `sql` provides helpers to manipulate a sqlite api-keys database.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None, subcommand_required = false)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// run the proxy server (this is the default behaviour in previous
    /// versions of the tool)
    Server(ServerOpts),

    /// perform a small SQL operation on the API keys sqlite database
    Sql {
        #[command(subcommand)]
        action: SqlAction,

        /// path to sqlite database with table `api_keys(key TEXT)`
        #[arg(long)]
        sqlite: Option<String>,
    },
}

/// options used when running the proxy server
#[derive(Parser, Debug)]
struct ServerOpts {
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

#[derive(Subcommand, Debug)]
enum SqlAction {
    /// add a username/api-key pair
    AddUser {
        username: String,
        api_key: String,
    },
    /// delete an existing username
    DelUser {
        username: String,
    },
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_opts_parsing() {
        let cli = Cli::parse_from([
            "prog",
            "server",
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
        if let Command::Server(opts) = cli.command.unwrap() {
            assert_eq!(opts.ollama_url.as_deref(), Some("http://example"));
            assert_eq!(opts.api_keys.as_ref().map(|v| v.as_slice()), Some(&["a".to_string(),"b".to_string(),"c".to_string()][..]));
            assert_eq!(opts.api_keys_file.as_deref(), Some("/tmp/k"));
            assert_eq!(opts.api_keys_sqlite.as_deref(), Some("/tmp/db"));
            assert_eq!(opts.proxy_host.as_deref(), Some("1.2.3.4"));
            assert_eq!(opts.proxy_port, Some(5555));
        } else {
            panic!("expected server command");
        }
    }

    #[test]
    fn sql_add_parsing() {
        let cli = Cli::parse_from(["prog", "sql", "--sqlite", "/tmp/db", "add-user", "foo", "bar"]);
        if let Command::Sql { action, sqlite } = cli.command.unwrap() {
            assert_eq!(sqlite.as_deref(), Some("/tmp/db"));
            match action {
                SqlAction::AddUser { username, api_key } => {
                    assert_eq!(username, "foo");
                    assert_eq!(api_key, "bar");
                }
                _ => panic!("wrong subcommand"),
            }
        } else {
            panic!("expected sql command");
        }
    }

    #[test]
    fn sql_del_parsing() {
        let cli = Cli::parse_from(["prog", "sql", "del-user", "foo"]);
        if let Command::Sql { action, sqlite } = cli.command.unwrap() {
            assert!(sqlite.is_none());
            match action {
                SqlAction::DelUser { username } => assert_eq!(username, "foo"),
                _ => panic!("wrong subcommand"),
            }
        } else {
            panic!("expected sql command");
        }
    }

    #[test]
    fn default_server_command() {
        let cli = Cli::parse_from(["prog"]);
        // mimic the fallback logic used in main()
        let command = cli.command.unwrap_or(Command::Server(ServerOpts {
            ollama_url: None,
            api_keys: None,
            api_keys_file: None,
            api_keys_sqlite: None,
            proxy_host: None,
            proxy_port: None,
        }));
        if let Command::Server(opts) = command {
            assert!(opts.ollama_url.is_none());
            assert!(opts.api_keys.is_none());
            assert!(opts.api_keys_file.is_none());
            assert!(opts.api_keys_sqlite.is_none());
            assert!(opts.proxy_host.is_none());
            assert!(opts.proxy_port.is_none());
        } else {
            panic!("expected server command");
        }
    }
}


#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Command::Server(ServerOpts {
        ollama_url: None,
        api_keys: None,
        api_keys_file: None,
        api_keys_sqlite: None,
        proxy_host: None,
        proxy_port: None,
    })) {
        Command::Server(opts) => {
            // build configuration as before
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
        Command::Sql { action, sqlite } => {
            let path = if let Some(p) = sqlite {
                p
            } else if let Ok(envp) = std::env::var("API_KEYS_SQLITE") {
                envp
            } else {
                eprintln!("error: no sqlite path provided; use --sqlite or set API_KEYS_SQLITE");
                std::process::exit(1);
            };

            match action {
                SqlAction::AddUser { username, api_key } => {
                    if let Err(e) = config::add_key_to_sqlite(&path, &username, &api_key) {
                        eprintln!("failed to add user: {}", e);
                        std::process::exit(1);
                    }
                    println!("user '{}' added", username);
                }
                SqlAction::DelUser { username } => {
                    let removed = match config::remove_key_from_sqlite(&path, &username) {
                        Ok(r) => r,
                        Err(e) => {
                            eprintln!("failed to remove user: {}", e);
                            std::process::exit(1);
                        }
                    };
                    if !removed {
                        eprintln!("no such user");
                        std::process::exit(2);
                    }
                    println!("user '{}' removed", username);
                }
            }
        }
    }
}
