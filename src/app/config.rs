use serde::Deserialize;

use crate::shared::error::AppError;

/// Complete application configuration.
///
/// Precedence: environment variables > `config/default.toml` > built-in
/// defaults. `DATABASE_URL` is environment-only (never stored in the repo).
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub mempool: MempoolConfig,
    pub synchronization: SynchronizationConfig,
    pub history: HistoryConfig,
    pub api: ApiConfig,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    /// From the environment only (`DATABASE_URL`). `None` in tests that
    /// inject their own URL directly.
    pub url: Option<String>,
    pub read_pool_max_connections: u32,
    pub write_pool_max_connections: u32,
    pub acquire_timeout_seconds: u64,
}

#[derive(Debug, Clone)]
pub struct MempoolConfig {
    pub base_url: String,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone)]
pub struct SynchronizationConfig {
    pub interval_seconds: u64,
    pub batch_size: usize,
    pub sync_on_startup: bool,
    pub reject_empty_response: bool,
}

#[derive(Debug, Clone)]
pub struct HistoryConfig {
    pub retention_days: u32,
    pub minimum_successful_snapshots: u32,
}

#[derive(Debug, Clone)]
pub struct ApiConfig {
    /// `true` (default) extends `GET /nodes` with `saved_at`.
    /// `false` restores the original four-field challenge contract.
    pub include_saved_at: bool,
}

/// Maximum bind parameters per statement in PostgreSQL is 65535; node inserts
/// use 5 columns per row.
pub const MAX_INSERT_PARAMS: usize = 65_535;
pub const NODE_INSERT_COLUMNS: usize = 5;
/// Highest usable insert batch size under the parameter limit.
pub const MAX_BATCH_SIZE: usize = MAX_INSERT_PARAMS / NODE_INSERT_COLUMNS;

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                host: "0.0.0.0".to_owned(),
                port: 3000,
            },
            database: DatabaseConfig {
                url: None,
                read_pool_max_connections: 10,
                write_pool_max_connections: 2,
                acquire_timeout_seconds: 5,
            },
            mempool: MempoolConfig {
                base_url: "https://mempool.space".to_owned(),
                timeout_seconds: 10,
            },
            synchronization: SynchronizationConfig {
                interval_seconds: 600,
                batch_size: 500,
                sync_on_startup: true,
                reject_empty_response: true,
            },
            history: HistoryConfig {
                retention_days: 30,
                minimum_successful_snapshots: 3,
            },
            api: ApiConfig {
                include_saved_at: true,
            },
        }
    }
}

// Raw TOML shapes; every field optional so partial files merge over defaults.
#[derive(Deserialize, Default)]
struct TomlConfig {
    server: Option<TomlServer>,
    database: Option<TomlDatabase>,
    mempool: Option<TomlMempool>,
    synchronization: Option<TomlSynchronization>,
    history: Option<TomlHistory>,
    api: Option<TomlApi>,
}
#[derive(Deserialize, Default)]
struct TomlServer {
    host: Option<String>,
    port: Option<u16>,
}
#[derive(Deserialize, Default)]
struct TomlDatabase {
    read_pool_max_connections: Option<u32>,
    write_pool_max_connections: Option<u32>,
    acquire_timeout_seconds: Option<u64>,
}
#[derive(Deserialize, Default)]
struct TomlMempool {
    base_url: Option<String>,
    timeout_seconds: Option<u64>,
}
#[derive(Deserialize, Default)]
struct TomlSynchronization {
    interval_seconds: Option<u64>,
    batch_size: Option<usize>,
    sync_on_startup: Option<bool>,
    reject_empty_response: Option<bool>,
}
#[derive(Deserialize, Default)]
struct TomlHistory {
    retention_days: Option<u32>,
    minimum_successful_snapshots: Option<u32>,
}
#[derive(Deserialize, Default)]
struct TomlApi {
    include_saved_at: Option<bool>,
}

impl AppConfig {
    /// Load configuration for a production run: `.env`, then
    /// `config/default.toml`, then environment overrides.
    pub fn load() -> Result<Self, AppError> {
        dotenvy::dotenv().ok();
        let file = std::fs::read_to_string("config/default.toml").unwrap_or_default();
        Self::build(Some(&file), |key| std::env::var(key).ok())
    }

    /// Build configuration from a TOML file and an env lookup. The lookup is
    /// injectable so tests can exercise precedence without touching real
    /// process environment.
    pub fn build(
        file: Option<&str>,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, AppError> {
        let mut config = Self::default();

        if let Some(content) = file {
            let toml: TomlConfig = toml::from_str(content)
                .map_err(|e| AppError::Configuration(format!("invalid config file: {e}")))?;
            if let Some(s) = toml.server {
                if let Some(v) = s.host {
                    config.server.host = v;
                }
                if let Some(v) = s.port {
                    config.server.port = v;
                }
            }
            if let Some(d) = toml.database {
                if let Some(v) = d.read_pool_max_connections {
                    config.database.read_pool_max_connections = v;
                }
                if let Some(v) = d.write_pool_max_connections {
                    config.database.write_pool_max_connections = v;
                }
                if let Some(v) = d.acquire_timeout_seconds {
                    config.database.acquire_timeout_seconds = v;
                }
            }
            if let Some(m) = toml.mempool {
                if let Some(v) = m.base_url {
                    config.mempool.base_url = v;
                }
                if let Some(v) = m.timeout_seconds {
                    config.mempool.timeout_seconds = v;
                }
            }
            if let Some(s) = toml.synchronization {
                if let Some(v) = s.interval_seconds {
                    config.synchronization.interval_seconds = v;
                }
                if let Some(v) = s.batch_size {
                    config.synchronization.batch_size = v;
                }
                if let Some(v) = s.sync_on_startup {
                    config.synchronization.sync_on_startup = v;
                }
                if let Some(v) = s.reject_empty_response {
                    config.synchronization.reject_empty_response = v;
                }
            }
            if let Some(h) = toml.history {
                if let Some(v) = h.retention_days {
                    config.history.retention_days = v;
                }
                if let Some(v) = h.minimum_successful_snapshots {
                    config.history.minimum_successful_snapshots = v;
                }
            }
            if let Some(a) = toml.api
                && let Some(v) = a.include_saved_at
            {
                config.api.include_saved_at = v;
            }
        }

        // Environment overrides win over the TOML file.
        if let Some(v) = env("DATABASE_URL") {
            config.database.url = Some(v);
        }
        if let Some(v) = env("SERVER_HOST") {
            config.server.host = v;
        }
        if let Some(v) = parse_env(&env, "SERVER_PORT")? {
            config.server.port = v;
        }
        if let Some(v) = parse_env(&env, "SYNC_INTERVAL_SECONDS")? {
            config.synchronization.interval_seconds = v;
        }
        if let Some(v) = parse_env(&env, "SYNC_BATCH_SIZE")? {
            config.synchronization.batch_size = v;
        }
        if let Some(v) = parse_env(&env, "MEMPOOL_TIMEOUT_SECONDS")? {
            config.mempool.timeout_seconds = v;
        }
        if let Some(v) = parse_env(&env, "HISTORY_RETENTION_DAYS")? {
            config.history.retention_days = v;
        }
        if let Some(v) = parse_env(&env, "HISTORY_MINIMUM_SUCCESSFUL_SNAPSHOTS")? {
            config.history.minimum_successful_snapshots = v;
        }
        if let Some(v) = parse_bool(&env, "SYNC_ON_STARTUP")? {
            config.synchronization.sync_on_startup = v;
        }
        if let Some(v) = parse_bool(&env, "REJECT_EMPTY_RESPONSE")? {
            config.synchronization.reject_empty_response = v;
        }
        if let Some(v) = parse_bool(&env, "API_INCLUDE_SAVED_AT")? {
            config.api.include_saved_at = v;
        }
        if let Some(v) = env("MEMPOOL_BASE_URL") {
            config.mempool.base_url = v;
        }

        config.validate()?;
        Ok(config)
    }

    /// Reject invalid values early with actionable messages.
    pub fn validate(&self) -> Result<(), AppError> {
        let s = &self.synchronization;
        if s.interval_seconds == 0 {
            return Err(AppError::Configuration(
                "synchronization.interval_seconds must be greater than 0".to_owned(),
            ));
        }
        if s.batch_size == 0 {
            return Err(AppError::Configuration(
                "synchronization.batch_size must be greater than 0".to_owned(),
            ));
        }
        if s.batch_size > MAX_BATCH_SIZE {
            return Err(AppError::Configuration(format!(
                "synchronization.batch_size must be at most {MAX_BATCH_SIZE} to respect the PostgreSQL parameter limit"
            )));
        }
        let d = &self.database;
        if d.read_pool_max_connections == 0 || d.write_pool_max_connections == 0 {
            return Err(AppError::Configuration(
                "database pool sizes must be greater than 0".to_owned(),
            ));
        }
        if d.acquire_timeout_seconds == 0 {
            return Err(AppError::Configuration(
                "database.acquire_timeout_seconds must be greater than 0".to_owned(),
            ));
        }
        if self.mempool.timeout_seconds == 0 {
            return Err(AppError::Configuration(
                "mempool.timeout_seconds must be greater than 0".to_owned(),
            ));
        }
        if self.history.minimum_successful_snapshots == 0 {
            return Err(AppError::Configuration(
                "history.minimum_successful_snapshots must be greater than 0".to_owned(),
            ));
        }
        if self.server.port == 0 {
            return Err(AppError::Configuration(
                "server.port must not be 0".to_owned(),
            ));
        }
        Ok(())
    }
}

fn parse_env<T: std::str::FromStr>(
    env: &impl Fn(&str) -> Option<String>,
    key: &str,
) -> Result<Option<T>, AppError> {
    match env(key) {
        None => Ok(None),
        Some(raw) => raw.trim().parse::<T>().map(Some).map_err(|_| {
            AppError::Configuration(format!(
                "environment variable {key}={raw:?} is not a valid value"
            ))
        }),
    }
}

fn parse_bool(env: &impl Fn(&str) -> Option<String>, key: &str) -> Result<Option<bool>, AppError> {
    match env(key) {
        None => Ok(None),
        Some(raw) => match raw.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => Ok(Some(true)),
            "false" | "0" | "no" | "off" => Ok(Some(false)),
            _ => Err(AppError::Configuration(format!(
                "environment variable {key}={raw:?} is not a valid boolean"
            ))),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup<'a>(map: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| {
            map.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn defaults_match_spec() {
        let config = AppConfig::build(None, |_| None).unwrap();
        assert_eq!(config.synchronization.interval_seconds, 600);
        assert_eq!(config.synchronization.batch_size, 500);
        assert!(config.synchronization.sync_on_startup);
        assert!(config.synchronization.reject_empty_response);
        assert!(config.api.include_saved_at);
        assert_eq!(config.history.retention_days, 30);
        assert_eq!(config.history.minimum_successful_snapshots, 3);
        assert_eq!(config.server.port, 3000);
    }

    #[test]
    fn toml_overrides_defaults() {
        let toml = r#"
            [server]
            port = 8080
            [synchronization]
            interval_seconds = 120
            batch_size = 250
        "#;
        let config = AppConfig::build(Some(toml), |_| None).unwrap();
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.synchronization.interval_seconds, 120);
        assert_eq!(config.synchronization.batch_size, 250);
        // Untouched keys keep their defaults.
        assert!(config.api.include_saved_at);
        assert_eq!(config.history.retention_days, 30);
    }

    #[test]
    fn env_overrides_toml() {
        let toml = r#"
            [synchronization]
            interval_seconds = 120
        "#;
        let env = lookup(&[("SYNC_INTERVAL_SECONDS", "42")]);
        let config = AppConfig::build(Some(toml), env).unwrap();
        assert_eq!(config.synchronization.interval_seconds, 42);
    }

    #[test]
    fn invalid_interval_rejected() {
        let config = AppConfig::build(Some("[synchronization]\ninterval_seconds = 0\n"), |_| None);
        assert!(matches!(config, Err(AppError::Configuration(_))));
    }

    #[test]
    fn invalid_batch_size_rejected() {
        let config = AppConfig::build(Some("[synchronization]\nbatch_size = 999999\n"), |_| None);
        assert!(matches!(config, Err(AppError::Configuration(_))));
    }

    #[test]
    fn invalid_pool_sizes_rejected() {
        let toml = "[database]\nread_pool_max_connections = 0\n";
        let config = AppConfig::build(Some(toml), |_| None);
        assert!(matches!(config, Err(AppError::Configuration(_))));
    }

    #[test]
    fn invalid_env_boolean_rejected() {
        let env = lookup(&[("API_INCLUDE_SAVED_AT", "maybe")]);
        let config = AppConfig::build(None, env);
        assert!(matches!(config, Err(AppError::Configuration(_))));
    }

    #[test]
    fn env_boolean_accepted() {
        let env = lookup(&[("API_INCLUDE_SAVED_AT", "false")]);
        let config = AppConfig::build(None, env).unwrap();
        assert!(!config.api.include_saved_at);
    }
}
