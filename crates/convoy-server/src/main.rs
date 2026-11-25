//! CLI entrypoint: load config, initialise Steam, run the server.

use std::sync::Arc;

use convoy_server::{ConvoyServer, ServerConfig, SteamTicketVerifier};
use convoy_steam::gameserver::SteamServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args: Vec<String> = std::env::args().collect();
    let config = if args.len() >= 3 && args[1] == "--config" {
        ServerConfig::load_from_file(&args[2])?
    } else if args.len() >= 2 && !args[1].starts_with('-') {
        ServerConfig::load_from_file(&args[1])?
    } else if std::path::Path::new("config.toml").exists() {
        ServerConfig::load_from_file("config.toml")?
    } else {
        ServerConfig::default()
    };

    log::info!(
        "starting {} on port {} (query {}, map {})",
        config.name,
        config.port,
        config.query_port,
        config.map_name
    );

    let steam = Arc::new(SteamServer::init(
        config.port,
        config.query_port,
        &config.name,
        config.server_logon_token.as_deref(),
    )?);

    let mut verifier = SteamTicketVerifier::new(Arc::clone(&steam));
    if config.allow_unverified_clients {
        verifier = verifier.allowing_unverified();
    }
    let verifier = Box::new(verifier);
    ConvoyServer::new(config, verifier).run(steam).await?;
    Ok(())
}
