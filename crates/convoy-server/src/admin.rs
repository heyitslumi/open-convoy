//! Admin moderation, slash command dispatching, and banlist manager.

use std::fs::File;
use std::io::{self, BufRead, BufReader};

use crate::server::ConvoyServer;

/// An entry in `banlist.txt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BanEntry {
    pub steam_id: u64,
    pub username: String,
    /// 0 denotes permanent ban; non-zero represents absolute Unix epoch seconds.
    pub end_unix_timestamp: u64,
    pub reason: String,
}

/// Manages parsed entries from `banlist.txt`.
#[derive(Debug, Clone, Default)]
pub struct BanlistManager {
    bans: Vec<BanEntry>,
}

impl BanlistManager {
    pub fn new() -> Self {
        Self { bans: Vec::new() }
    }

    /// Load and parse banlist file.
    /// Lines starting with `#` or `===` are ignored as headers/comments.
    /// Format: `steam_id; username; end_unix_timestamp; reason`
    pub fn load_from_file(path: &str) -> Result<Self, io::Error> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut bans = Vec::new();

        for line_res in reader.lines() {
            let line = line_res?;
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("===") || trimmed.starts_with("===") {
                continue;
            }

            let parts: Vec<&str> = trimmed.split(';').map(|s| s.trim()).collect();
            if parts.len() >= 4 {
                if let (Ok(steam_id), Ok(end_ts)) = (parts[0].parse::<u64>(), parts[2].parse::<u64>()) {
                    bans.push(BanEntry {
                        steam_id,
                        username: parts[1].to_string(),
                        end_unix_timestamp: end_ts,
                        reason: parts[3..].join("; "),
                    });
                }
            }
        }

        Ok(Self { bans })
    }

    /// Checks if a SteamID is currently banned.
    /// Returns the active `BanEntry` if banned, or `None` if not banned or ban is expired.
    pub fn is_banned(&self, steam_id: u64, now_unix: u64) -> Option<&BanEntry> {
        self.bans.iter().find(|entry| {
            if entry.steam_id != steam_id {
                return false;
            }
            if entry.end_unix_timestamp == 0 {
                true // permanent ban
            } else {
                now_unix < entry.end_unix_timestamp
            }
        })
    }

    /// Add a ban entry to the manager.
    pub fn add_ban(&mut self, entry: BanEntry) {
        self.bans.retain(|b| b.steam_id != entry.steam_id);
        self.bans.push(entry);
    }
}

/// Result of an executed admin command.
#[derive(Debug, Clone, PartialEq)]
pub enum AdminResponse {
    Warning(String),
    TimeSet { hours: u8, minutes: u8 },
    RainFactorSet(f32),
    Help(Vec<String>),
}

/// Errors occurring during admin command dispatch.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum AdminError {
    #[error("caller is not an authorized moderator")]
    Unauthorized,
    #[error("unknown command: {0}")]
    UnknownCommand(String),
    #[error("invalid arguments: {0}")]
    InvalidArguments(String),
}

/// Dispatch and execute an admin slash command.
pub fn handle_admin_command(
    server: &mut ConvoyServer,
    sender_steamid: u64,
    text: &str,
) -> Result<AdminResponse, AdminError> {
    if !server.config().moderators.contains(&sender_steamid) {
        return Err(AdminError::Unauthorized);
    }

    let trimmed = text.trim();
    if !trimmed.starts_with('/') {
        return Err(AdminError::UnknownCommand(trimmed.to_string()));
    }

    let mut parts = trimmed.splitn(2, ' ');
    let cmd = parts.next().unwrap_or("");
    let args = parts.next().unwrap_or("").trim();

    match cmd {
        "/help" => Ok(AdminResponse::Help(vec![
            "/warn <message>".into(),
            "/set_time <HH:MM>".into(),
            "/set_rain_factor <value>".into(),
            "/help".into(),
        ])),
        "/warn" => {
            if args.is_empty() {
                return Err(AdminError::InvalidArguments("warning message cannot be empty".into()));
            }
            Ok(AdminResponse::Warning(args.to_string()))
        }
        "/set_time" => {
            let time_parts: Vec<&str> = args.split(':').collect();
            if time_parts.len() != 2 {
                return Err(AdminError::InvalidArguments("expected format HH:MM".into()));
            }
            let hours: u8 = time_parts[0]
                .parse()
                .map_err(|_| AdminError::InvalidArguments("invalid hour".into()))?;
            let minutes: u8 = time_parts[1]
                .parse()
                .map_err(|_| AdminError::InvalidArguments("invalid minute".into()))?;

            if hours > 23 || minutes > 59 {
                return Err(AdminError::InvalidArguments("time out of 24h range".into()));
            }

            server.set_world_clock(hours, minutes);
            Ok(AdminResponse::TimeSet { hours, minutes })
        }
        "/set_rain_factor" => {
            let val: f32 = args
                .parse()
                .map_err(|_| AdminError::InvalidArguments("invalid float factor".into()))?;
            if !(0.0..=1.0).contains(&val) {
                return Err(AdminError::InvalidArguments("rain factor must be between 0.0 and 1.0".into()));
            }

            server.weather_mut().set_rain_factor(val);
            Ok(AdminResponse::RainFactorSet(val))
        }
        _ => Err(AdminError::UnknownCommand(cmd.to_string())),
    }
}
