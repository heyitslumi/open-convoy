use convoy_server::admin::{handle_admin_command, AdminError, AdminResponse, BanEntry, BanlistManager};
use convoy_server::config::ServerConfig;
use convoy_server::server::{ConvoyServer, ServerError, TicketVerifier};

struct MockTicketVerifier;
impl TicketVerifier for MockTicketVerifier {
    fn verify(&self, _peer: Option<u64>, ticket: &[u8]) -> Result<u64, ServerError> {
        let steam_id = ticket
            .get(..8)
            .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
            .ok_or(ServerError::AuthFailed)?;
        Ok(steam_id)
    }
}

fn create_test_server(moderators: Vec<u64>) -> ConvoyServer {
    let mut config = ServerConfig::default();
    config.moderators = moderators;
    ConvoyServer::new(config, Box::new(MockTicketVerifier))
}

#[test]
fn test_banlist_parsing_and_status() {
    let dir = std::env::temp_dir().join(format!("convoy-banlist-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let banlist_path = dir.join("banlist.txt");

    let banlist_content = r#"=== BAN LIST ===
# Format: steam_id; username; end_unix_timestamp; reason
76561198000000001; PermUser; 0; cheating
76561198000000002; TempUser; 1700000000; griefing
================
"#;
    std::fs::write(&banlist_path, banlist_content).unwrap();

    let banlist = BanlistManager::load_from_file(banlist_path.to_str().unwrap()).unwrap();

    // Permanent ban check
    let perm = banlist.is_banned(76561198000000001, 2000000000);
    assert!(perm.is_some());
    let entry = perm.unwrap();
    assert_eq!(entry.username, "PermUser");
    assert_eq!(entry.reason, "cheating");
    assert_eq!(entry.end_unix_timestamp, 0);

    // Active temporary ban check (current time < ban end)
    let active_temp = banlist.is_banned(76561198000000002, 1699999999);
    assert!(active_temp.is_some());
    assert_eq!(active_temp.unwrap().reason, "griefing");

    // Expired temporary ban check (current time >= ban end)
    let expired_temp = banlist.is_banned(76561198000000002, 1700000000);
    assert!(expired_temp.is_none());

    // Non-banned user
    let clean = banlist.is_banned(76561198000000003, 1700000000);
    assert!(clean.is_none());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_banlist_add_ban() {
    let mut banlist = BanlistManager::new();
    banlist.add_ban(BanEntry {
        steam_id: 76561198000000005,
        username: "Troll".into(),
        end_unix_timestamp: 0,
        reason: "toxic".into(),
    });

    let banned = banlist.is_banned(76561198000000005, 100);
    assert!(banned.is_some());
    assert_eq!(banned.unwrap().reason, "toxic");
}

#[test]
fn test_admin_commands_authorized() {
    let mod_steamid = 76561198000000010;
    let mut server = create_test_server(vec![mod_steamid]);

    // /help
    let res = handle_admin_command(&mut server, mod_steamid, "/help").unwrap();
    match res {
        AdminResponse::Help(commands) => {
            assert!(commands.contains(&"/warn".into()) || commands.iter().any(|c| c.contains("/warn")));
        }
        _ => panic!("expected AdminResponse::Help"),
    }

    // /warn <msg>
    let res = handle_admin_command(&mut server, mod_steamid, "/warn Watch your speed").unwrap();
    match res {
        AdminResponse::Warning(msg) => {
            assert_eq!(msg, "Watch your speed");
        }
        _ => panic!("expected AdminResponse::Warning"),
    }

    // /set_time <HH:MM>
    let res = handle_admin_command(&mut server, mod_steamid, "/set_time 14:30").unwrap();
    match res {
        AdminResponse::TimeSet { hours, minutes } => {
            assert_eq!(hours, 14);
            assert_eq!(minutes, 30);
            assert_eq!(server.world_clock(), (14, 30));
        }
        _ => panic!("expected AdminResponse::TimeSet"),
    }

    // /set_rain_factor <value>
    let res = handle_admin_command(&mut server, mod_steamid, "/set_rain_factor 0.75").unwrap();
    match res {
        AdminResponse::RainFactorSet(val) => {
            assert!((val - 0.75).abs() < 1e-4);
            assert!((server.weather().rain_factor() - 0.75).abs() < 1e-4);
        }
        _ => panic!("expected AdminResponse::RainFactorSet"),
    }
}

#[test]
fn test_admin_commands_unauthorized() {
    let mod_steamid = 76561198000000010;
    let normal_user = 76561198000000099;
    let mut server = create_test_server(vec![mod_steamid]);

    let res = handle_admin_command(&mut server, normal_user, "/warn Test");
    assert!(matches!(res, Err(AdminError::Unauthorized)));
}

#[test]
fn test_admin_commands_invalid_syntax() {
    let mod_steamid = 76561198000000010;
    let mut server = create_test_server(vec![mod_steamid]);

    // Unknown command
    let res = handle_admin_command(&mut server, mod_steamid, "/unknown");
    assert!(matches!(res, Err(AdminError::UnknownCommand(_))));

    // Invalid time
    let res = handle_admin_command(&mut server, mod_steamid, "/set_time 25:00");
    assert!(matches!(res, Err(AdminError::InvalidArguments(_))));

    // Invalid rain factor
    let res = handle_admin_command(&mut server, mod_steamid, "/set_rain_factor 2.5");
    assert!(matches!(res, Err(AdminError::InvalidArguments(_))));
}

#[test]
fn test_banned_user_connection_rejected_at_auth() {
    // When handling packet from banned user, auth should fail or be rejected
    let dir = std::env::temp_dir().join(format!("convoy-ban-reject-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let banlist_path = dir.join("banlist.txt");
    std::fs::write(&banlist_path, "76561198000000123; BadActor; 0; banned reason\n").unwrap();

    let mut config = ServerConfig::default();
    config.banlist_path = Some(banlist_path.to_str().unwrap().into());
    let mut server = ConvoyServer::new(config, Box::new(MockTicketVerifier));

    let conn = 1;
    server.handle_connection(conn);

    let mut ticket = vec![0u8; 16];
    ticket[..8].copy_from_slice(&76561198000000123u64.to_le_bytes());

    let auth_msg = convoy_proto::messages::AuthTicketMsg {
        client_id: 10,
        ticket,
    };
    let packet = auth_msg.encode();

    let res = server.handle_packet(conn, &packet);
    assert!(matches!(res, Err(ServerError::Banned(_))));

    std::fs::remove_dir_all(&dir).ok();
}
