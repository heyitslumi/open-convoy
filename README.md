# OpenConvoy

An open-source Rust server for Euro Truck Simulator 2 Convoy sessions.

Players join from the stock game; no patched client and no dependency on the
proprietary SCS dedicated server. Steam provides authentication, P2P transport
and server browsing.

## Status

Discovery works: the session appears in the in-game Convoy browser and the
server answers Steam's `A2S_INFO`, `A2S_PLAYER` and `A2S_RULES` queries the same
way the official server does.

Joining does **not** work yet. The client is rejected during the mod-verification
step of the handshake. Traffic, weather and clock synchronisation exist in the
codebase but are unverified against a real client.

## Build

Requires a Rust toolchain and Steam's `libsteam_api.so`.

```sh
cargo build --release -p convoy-server
```

## Run

```sh
cp config.example.toml config.toml
# put your Steam game server logon token in config.toml
STEAM_API_PATH=/path/to/libsteam_api.so SteamAppId=227300 \
  ./target/release/convoy-server config.toml
```

`config.toml` is git-ignored; keep the token out of version control. A game
server logon token is issued at
<https://steamcommunity.com/dev/managegameservers> for App ID `227300`.

## Tests

```sh
cargo test --workspace
```

The Steam integration test is `#[ignore]`d: it needs a running Steam client and
exclusive access to the Steam GameServer interface.

## Layout

- `crates/convoy-proto` — wire frames and message payloads
- `crates/convoy-steam` — FFI wrapper around the Steamworks GameServer and
  networking sockets
- `crates/convoy-server` — session state machine, relay, traffic, weather,
  moderation

## Legal

Not affiliated with or endorsed by SCS Software or Valve. Protocol details were
derived from observing network traffic for interoperability.

No license file is present yet; the code is not licensed for reuse until one is
added.