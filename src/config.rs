use std::{env, fs};

use anyhow::{Context, Result};
use rusqlite::Connection;

/// Application configuration, loaded at startup.
pub struct AppConfig {
    /// List of valid API keys.
    pub valid_keys: Vec<String>,
    /// Base URL for the Ollama service (no trailing slash).
    pub ollama_url: String,
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
        })
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

    #[test]
    fn env_fallback() {
        unsafe {
            env::remove_var("API_KEYS_SQLITE");
        }
        unsafe {
            env::remove_var("API_KEYS_FILE");
        }
        unsafe {
            env::set_var("API_KEYS", "a,b, c");
        }
        let cfg = AppConfig::load().expect("load");
        assert_eq!(cfg.valid_keys, vec!["a", "b", "c"]);
    }

    #[test]
    fn file_keys() {
        let mut tmp = NamedTempFile::new().unwrap();
        writeln!(tmp, "foo,bar").unwrap();
        unsafe {
            env::remove_var("API_KEYS_SQLITE");
        }
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
        }
        let cfg = AppConfig::load().expect("load");
        assert_eq!(cfg.ollama_url, "http://127.0.0.1:11434");
        unsafe {
            env::set_var("OLLAMA_URL", "http://example.com");
        }
        let cfg2 = AppConfig::load().expect("load");
        assert_eq!(cfg2.ollama_url, "http://example.com");
    }
}
