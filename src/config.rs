use std::{env, fs, net::SocketAddr};

use anyhow::{Context, Result};
use rusqlite::Connection;

/// Application configuration, loaded at startup.
pub struct AppConfig {
    /// List of valid API keys.
    pub valid_keys: Vec<String>,
    /// Base URL for the Ollama service (no trailing slash).
    pub ollama_url: String,
    /// Address on which the proxy should listen.
    pub proxy_addr: SocketAddr,
}

impl AppConfig {
    /// Load configuration from environment / backing store.
    ///
    /// Priority of key sources:
    /// 1. `API_KEYS_SQLITE` pointing at a SQLite database with
    ///    a table `api_keys(key TEXT)`.
    /// 2. `API_KEYS_FILE` pointing at a newline- or comma-separated file.
    /// 3. `API_KEYS` environment variable containing comma-separated keys.
    pub fn load() -> Result<Self> {
        let ollama_url = env::var("OLLAMA_URL").unwrap_or_else(|_| {
            // default to localhost port used previously
            "http://127.0.0.1:11434".to_string()
        });

        // default listening address for the proxy
        let proxy_host = env::var("PROXY_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let proxy_port: u16 = env::var("PROXY_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3000);
        let proxy_addr = format!("{}:{}", proxy_host, proxy_port)
            .parse()
            .context("failed to parse PROXY_HOST:PROXY_PORT into SocketAddr")?;

        let valid_keys = if let Ok(sqlite_path) = env::var("API_KEYS_SQLITE") {
            load_keys_from_sqlite(&sqlite_path)?
        } else if let Ok(file_path) = env::var("API_KEYS_FILE") {
            load_keys_from_file(&file_path)?
        } else {
            let keys = env::var("API_KEYS").unwrap_or_default();
            keys.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        };

        Ok(AppConfig {
            valid_keys,
            ollama_url,
            proxy_addr,
        })
    }
}

/// Values that can be supplied via command-line flags; the loader reads
/// environment variables, but these overrides allow the CLI to take
/// precedence.
pub struct ConfigOverrides {
    pub ollama_url: Option<String>,
    pub proxy_host: Option<String>,
    pub proxy_port: Option<u16>,

    // API keys override; precedence is sqlite > file > explicit list.
    pub api_keys_sqlite: Option<String>,
    pub api_keys_file: Option<String>,
    pub api_keys: Option<Vec<String>>,
}

impl Default for ConfigOverrides {
    fn default() -> Self {
        ConfigOverrides {
            ollama_url: None,
            proxy_host: None,
            proxy_port: None,
            api_keys_sqlite: None,
            api_keys_file: None,
            api_keys: None,
        }
    }
}

impl AppConfig {
    /// Apply non-`None` values from `overrides` to `self`.
    ///
    /// Returns an `anyhow::Result<()>` because applying certain overrides may
    /// involve filesystem or database access (for key loading).
    pub fn apply_overrides(&mut self, overrides: &ConfigOverrides) -> Result<()> {
        if let Some(url) = &overrides.ollama_url {
            self.ollama_url = url.clone();
        }
        if overrides.proxy_host.is_some() || overrides.proxy_port.is_some() {
            let host = overrides
                .proxy_host
                .clone()
                .unwrap_or_else(|| self.proxy_addr.ip().to_string());
            let port = overrides.proxy_port.unwrap_or(self.proxy_addr.port());
            self.proxy_addr = format!("{}:{}", host, port)
                .parse()
                .expect("valid socket addr from overrides");
        }

        // handle api key overrides
        if overrides.api_keys_sqlite.is_some()
            || overrides.api_keys_file.is_some()
            || overrides.api_keys.is_some()
        {
            let keys = if let Some(path) = &overrides.api_keys_sqlite {
                load_keys_from_sqlite(path)?
            } else if let Some(path) = &overrides.api_keys_file {
                load_keys_from_file(path)?
            } else if let Some(list) = &overrides.api_keys {
                list.clone()
            } else {
                Vec::new()
            };
            self.valid_keys = keys;
        }

        Ok(())
    }
}

fn load_keys_from_file(path: &str) -> Result<Vec<String>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read API keys file '{}'", path))?;
    let keys = content
        .split(|c| c == ',' || c == '\n' || c == '\r')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    Ok(keys)
}

fn load_keys_from_sqlite(path: &str) -> Result<Vec<String>> {
    let conn = Connection::open(path)
        .with_context(|| format!("failed to open sqlite database '{}'", path))?;
    let mut stmt = conn
        .prepare("SELECT key FROM api_keys")
        .context("failed to prepare select statement")?;
    let keys_iter = stmt
        .query_map([], |row| row.get(0))
        .context("query execution failed")?;

    let mut keys = Vec::new();
    for key in keys_iter {
        keys.push(key?);
    }
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;
    use std::sync::Mutex;

    // guard around tests that touch the process environment; tests may run in
    // parallel threads, so we serialize access to avoid races and flakiness.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn env_fallback() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // clear any previous configuration
        unsafe {
            env::remove_var("API_KEYS_SQLITE");
            env::remove_var("API_KEYS_FILE");
            env::remove_var("API_KEYS");
            env::remove_var("OLLAMA_URL");
            env::remove_var("PROXY_HOST");
            env::remove_var("PROXY_PORT");
        }
        unsafe {
            env::set_var("API_KEYS", "a,b, c");
        }
        let cfg = AppConfig::load().expect("load");
        assert_eq!(cfg.valid_keys, vec!["a", "b", "c"]);
    }

    #[test]
    fn file_keys() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // clear any previous configuration
        unsafe {
            env::remove_var("API_KEYS_SQLITE");
            env::remove_var("API_KEYS_FILE");
            env::remove_var("API_KEYS");
            env::remove_var("OLLAMA_URL");
            env::remove_var("PROXY_HOST");
            env::remove_var("PROXY_PORT");
        }
        let mut tmp = NamedTempFile::new().unwrap();
        writeln!(tmp, "foo,bar").unwrap();
        unsafe {
            env::set_var("API_KEYS_FILE", tmp.path());
        }
        unsafe {
            env::remove_var("API_KEYS");
        }
        let cfg = AppConfig::load().expect("load");
        assert_eq!(cfg.valid_keys, vec!["foo", "bar"]);
    }

    #[test]
    fn sqlite_keys() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // clear any previous configuration
        unsafe {
            env::remove_var("API_KEYS_SQLITE");
            env::remove_var("API_KEYS_FILE");
            env::remove_var("API_KEYS");
            env::remove_var("OLLAMA_URL");
            env::remove_var("PROXY_HOST");
            env::remove_var("PROXY_PORT");
        }
        let tmp = NamedTempFile::new().unwrap();
        let conn = Connection::open(tmp.path()).unwrap();
        conn.execute("CREATE TABLE api_keys(key TEXT)", []).unwrap();
        conn.execute("INSERT INTO api_keys(key) VALUES (?1)", ["x"])
            .unwrap();
        conn.execute("INSERT INTO api_keys(key) VALUES (?1)", ["y"])
            .unwrap();
        unsafe {
            env::set_var("API_KEYS_SQLITE", tmp.path());
        }
        unsafe {
            env::remove_var("API_KEYS_FILE");
        }
        unsafe {
            env::remove_var("API_KEYS");
        }
        let cfg = AppConfig::load().expect("load");
        assert_eq!(cfg.valid_keys, vec!["x", "y"]);
    }

    #[test]
    fn url_defaults_and_override() {
        unsafe {
            env::remove_var("OLLAMA_URL");
            env::remove_var("API_KEYS_SQLITE");
            env::remove_var("API_KEYS_FILE");
            env::remove_var("API_KEYS");
        }
        let cfg = AppConfig::load().expect("load");
        assert_eq!(cfg.ollama_url, "http://127.0.0.1:11434");
        unsafe {
            env::set_var("OLLAMA_URL", "http://example.com");
        }
        let cfg2 = AppConfig::load().expect("load");
        assert_eq!(cfg2.ollama_url, "http://example.com");
    }

    #[test]
    fn proxy_addr_defaults_and_override() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            env::remove_var("API_KEYS_SQLITE");
            env::remove_var("API_KEYS_FILE");
            env::remove_var("API_KEYS");
            env::remove_var("OLLAMA_URL");
            env::remove_var("PROXY_HOST");
            env::remove_var("PROXY_PORT");
        }
        let cfg = AppConfig::load().expect("load");
        assert_eq!(cfg.proxy_addr, "0.0.0.0:3000".parse().unwrap());

        unsafe {
            env::set_var("PROXY_HOST", "127.0.0.1");
            env::set_var("PROXY_PORT", "8080");
        }
        let cfg2 = AppConfig::load().expect("load");
        assert_eq!(cfg2.proxy_addr, "127.0.0.1:8080".parse().unwrap());
    }

    #[test]
    fn apply_overrides_test() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            env::remove_var("API_KEYS_SQLITE");
            env::remove_var("API_KEYS_FILE");
            env::remove_var("API_KEYS");
            env::remove_var("OLLAMA_URL");
            env::remove_var("PROXY_HOST");
            env::remove_var("PROXY_PORT");
        }
        let mut cfg = AppConfig::load().expect("load");
        let mut overrides = ConfigOverrides::default();
        overrides.ollama_url = Some("http://foo".into());
        overrides.proxy_host = Some("127.0.0.1".into());
        overrides.proxy_port = Some(1234);
        // check key vector override
        overrides.api_keys = Some(vec!["k1".into(), "k2".into()]);
        let _ = cfg.apply_overrides(&overrides);
        assert_eq!(cfg.ollama_url, "http://foo");
        assert_eq!(cfg.proxy_addr, "127.0.0.1:1234".parse().unwrap());
        assert_eq!(cfg.valid_keys, vec!["k1", "k2"]);
    }
}
