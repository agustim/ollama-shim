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

// helpers used by the new `sql` command-line subcommands
fn ensure_sqlite(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)
        .with_context(|| format!("failed to open sqlite database '{}'", path))?;
    // our desired schema has a username column so the CLI can refer to users
    // by name.  existing databases created by older versions of the tool may
    // only have a single "key" column; creation in that case will be a no-op.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS api_keys(
            username TEXT PRIMARY KEY,
            key TEXT NOT NULL
        )",
        [],
    )?;
    // ensure there's an index on key so the old-style lookup remains fast and
    // unique behaviour is preserved.  again, `IF NOT EXISTS` avoids errors
    // against legacy tables.
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_api_keys_key ON api_keys(key)",
        [],
    )?;
    Ok(conn)
}

/// internal utility: check whether the table includes a given column.
fn has_column(conn: &Connection, col: &str) -> Result<bool> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(api_keys)")
        .context("failed to query table info")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?; // second column is name
        if name == col {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Add a user+key pair to the sqlite database. If the file does not exist it
/// will be created along with the necessary table.  For backwards
/// compatibility with older databases that only contain a single `key`
/// column, the username parameter is ignored and the key value is inserted as
/// before.
pub fn add_key_to_sqlite(path: &str, username: &str, key: &str) -> Result<()> {
    let conn = ensure_sqlite(path)?;
    if has_column(&conn, "username")? {
        conn.execute(
            "INSERT OR IGNORE INTO api_keys(username, key) VALUES (?1, ?2)",
            [username, key],
        )
        .context("failed to insert user/key into sqlite database")?;
    } else {
        // fall back for legacy schema
        conn.execute(
            "INSERT OR IGNORE INTO api_keys(key) VALUES (?1)",
            [key],
        )
        .context("failed to insert key into sqlite database")?;
    }
    Ok(())
}

/// Remove a user (by username) from the sqlite database.  Returns `true` if a
/// row was deleted.  In legacy databases without a username column the
/// supplied value is treated as the key itself.
pub fn remove_key_from_sqlite(path: &str, username: &str) -> Result<bool> {
    let conn = ensure_sqlite(path)?;
    let n = if has_column(&conn, "username")? {
        conn.execute("DELETE FROM api_keys WHERE username = ?1", [username])
    } else {
        conn.execute("DELETE FROM api_keys WHERE key = ?1", [username])
    }
    .context("failed to delete entry from sqlite database")?;
    Ok(n > 0)
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
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
    }

    #[test]
    fn sqlite_add_remove_key() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // clear any preexisting configuration variables to avoid races
        unsafe {
            env::remove_var("API_KEYS_SQLITE");
            env::remove_var("API_KEYS_FILE");
            env::remove_var("API_KEYS");
        }

        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap();
        // database does not yet exist - adding should create both file and table
        add_key_to_sqlite(path, "alice", "key1").unwrap();
        // loading should return the inserted value (only the key portion is used
        // by AppConfig::load)
        unsafe { env::set_var("API_KEYS_SQLITE", path); }
        let cfg = AppConfig::load().expect("load");
        assert_eq!(cfg.valid_keys, vec!["key1"]);
        // add a second user and attempt to re-add the first
        add_key_to_sqlite(path, "bob", "key2").unwrap();
        add_key_to_sqlite(path, "alice", "key1").unwrap(); // duplicate ignored
        let cfg2 = AppConfig::load().expect("load");
        assert_eq!(cfg2.valid_keys, vec!["key1", "key2"]);
        // removal by user name
        assert!(remove_key_from_sqlite(path, "alice").unwrap());
        assert!(!remove_key_from_sqlite(path, "alice").unwrap());
        let cfg3 = AppConfig::load().expect("load");
        assert_eq!(cfg3.valid_keys, vec!["key2"]);
    }

    #[test]
    fn sqlite_legacy_schema_compatibility() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap();
        // manually create legacy table with only key column
        let conn = Connection::open(path).unwrap();
        conn.execute("CREATE TABLE api_keys(key TEXT)", []).unwrap();
        // calling new helper should fall back to inserting key value
        add_key_to_sqlite(path, "ignored_user", "legacykey").unwrap();
        // make sure the loader looks at our temp file
        unsafe { env::set_var("API_KEYS_SQLITE", path); }
        let cfg = AppConfig::load().expect("load");
        assert_eq!(cfg.valid_keys, vec!["legacykey"]);
        // removal should treat the provided username as the key
        assert!(remove_key_from_sqlite(path, "legacykey").unwrap());
        let cfg2 = AppConfig::load().expect("load");
        assert_eq!(cfg2.valid_keys, Vec::<String>::new());
    }
}
