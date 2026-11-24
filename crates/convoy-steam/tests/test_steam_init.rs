use convoy_steam::gameserver::SteamServer;

#[test]
#[ignore = "needs a running Steam client and exclusive Steam GameServer access"]
fn test_steam_server_init_and_drop() {
    std::env::set_var("SteamAppId", "227300");

    let server = SteamServer::init(27015, 27016, "OpenConvoy", None)
        .expect("SteamServer::init should succeed with valid steam library");

    // Check sockets interface availability
    let incoming = server.sockets().poll_incoming();
    assert!(incoming.is_empty());

    // Create a listen socket P2P
    let sock = server.sockets().create_listen_socket_p2p(0);
    assert_ne!(sock, 0);

    // Callbacks pump
    server.run_callbacks();

    // Begin / End auth session
    let auth_res = server.begin_auth_session(b"dummy_ticket", 123456789);
    // Dummy ticket should fail verification but call succeeds without panic
    assert!(auth_res.is_err());
    server.end_auth_session(123456789);

    // Drop cleanly
    drop(server);
}
