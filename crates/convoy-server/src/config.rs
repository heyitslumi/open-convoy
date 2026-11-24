//! Server configuration.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub name: String,
    pub password: String,
    pub max_players: u16,
    pub port: u16,
    pub query_port: u16,
    pub virtual_port: u16,
    pub server_logon_token: Option<String>,
    pub game_version: String,
    pub map_name: String,
    pub map_dimensions: [i32; 4],
    pub moderators: Vec<u64>,
    pub banlist_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConfigFile {
    server: Option<ServerSection>,
    network: Option<NetworkSection>,
    compatibility: Option<CompatibilitySection>,
    #[serde(flatten)]
    flat: ServerConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ServerSection {
    name: Option<String>,
    password: Option<String>,
    max_players: Option<u16>,
    server_logon_token: Option<String>,
    moderators: Option<Vec<u64>>,
    banlist_path: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct NetworkSection {
    port: Option<u16>,
    query_port: Option<u16>,
    virtual_port: Option<u16>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct CompatibilitySection {
    game_version: Option<String>,
    map_name: Option<String>,
    map_dimensions: Option<[i32; 4]>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            name: "OpenConvoy".into(),
            password: String::new(),
            max_players: 8,
            port: 27015,
            query_port: 27016,
            virtual_port: 100,
            server_logon_token: None,
            game_version: "1.62s".into(),
            map_name: "/map/europe.mbd".into(),
            map_dimensions: [-24576000, -31744000, 20480000, 22528000],
            moderators: Vec::new(),
            banlist_path: None,
        }
    }
}

impl ServerConfig {
    pub fn load_from_file(path: &str) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.into(),
            source,
        })?;

        // First attempt parsing structured/nested toml sections, falling back to flat
        if let Ok(file) = toml::from_str::<ConfigFile>(&text) {
            let mut cfg = file.flat;
            if let Some(srv) = file.server {
                if let Some(v) = srv.name { cfg.name = v; }
                if let Some(v) = srv.password { cfg.password = v; }
                if let Some(v) = srv.max_players { cfg.max_players = v; }
                if srv.server_logon_token.is_some() { cfg.server_logon_token = srv.server_logon_token; }
                if let Some(v) = srv.moderators { cfg.moderators = v; }
                if srv.banlist_path.is_some() { cfg.banlist_path = srv.banlist_path; }
            }
            if let Some(net) = file.network {
                if let Some(v) = net.port { cfg.port = v; }
                if let Some(v) = net.query_port { cfg.query_port = v; }
                if let Some(v) = net.virtual_port { cfg.virtual_port = v; }
            }
            if let Some(comp) = file.compatibility {
                if let Some(v) = comp.game_version { cfg.game_version = v; }
                if let Some(v) = comp.map_name { cfg.map_name = v; }
                if let Some(v) = comp.map_dimensions { cfg.map_dimensions = v; }
            }
            return Ok(cfg);
        }

        toml::from_str(&text).map_err(|source| ConfigError::Parse {
            path: path.into(),
            source,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config file {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config file {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: toml::de::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let config = ServerConfig::default();
        assert_eq!(config.port, 27015);
        assert_eq!(config.query_port, 27016);
        assert_eq!(config.virtual_port, 100);
        assert_eq!(config.game_version, "1.62s");
        assert_eq!(config.map_name, "/map/europe.mbd");
        assert_eq!(config.server_logon_token, None);
    }

    #[test]
    fn file_overrides_only_the_keys_it_sets() {
        let dir = std::env::temp_dir().join(format!("convoy-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("server.toml");
        std::fs::write(&path, "name = \"Test\"\nmax_players = 4\n").unwrap();

        let config = ServerConfig::load_from_file(path.to_str().unwrap()).unwrap();
        assert_eq!(config.name, "Test");
        assert_eq!(config.max_players, 4);
        assert_eq!(config.port, 27015);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_file_is_an_error() {
        assert!(ServerConfig::load_from_file("/nonexistent/convoy.toml").is_err());
    }

    #[test]
    fn loads_example_config() {
        let config = ServerConfig::load_from_file("../../config.example.toml").unwrap();
        assert_eq!(config.name, "OpenConvoy");
        assert_eq!(config.port, 27015);
        assert_eq!(config.query_port, 27016);
        assert_eq!(config.virtual_port, 100);
        assert_eq!(config.game_version, "1.62s");
        assert_eq!(config.map_name, "/map/europe.mbd");
        assert_eq!(config.map_dimensions, [-24576000, -31744000, 20480000, 22528000]);
        assert_eq!(config.banlist_path, Some("banlist.txt".into()));
    }
}
