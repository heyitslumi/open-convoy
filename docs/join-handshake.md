# Join handshake: what is known, what fails

Discovery is solved. Joining is not. This file records the evidence so the work
can resume without repeating it.

## Working

- Server appears in the in-game Convoy browser, under the configured name.
- Virtual ports `100` (join) and `101` (query) are created after Steam logon.
  Creating them before logon leaves the query socket silent.
- `ISteamNetworkingSockets::RunCallbacks` must be pumped in addition to
  `SteamGameServer_RunCallbacks`, otherwise no connection callback ever fires.
- Search connections are answered with the 76-byte session listing captured
  from the official server (`session_listing`). Answering with the player
  challenge instead makes the listing appear but never connect.
- `A2S_INFO`, `A2S_PLAYER` and `A2S_RULES` match the official replies,
  including the `D0=0` rule.

## Failing

The client reports:

```
[MP] Warning: Client - suspicious message from <id> - type 30 - 0.
[MP] Connection closed - reason: 6
```

about one second after `Game server joined`, then closes. It never sends the
139-byte mod-verification frame that the official server receives.

## Evidence gathered

Opening messages the official server sends on the join port, captured with a
probe client (`00000b0a000100` first):

| Size | Bytes |
|---|---|
| 7 | `00000b <cid> 01 00` |
| 16 | `00000c f7ff0100 02 <4 opaque> 00000000` |
| 16 | `000007 f7ff0100 02 <4 opaque> 00000000` |
| 16 | `000007 f7ff0100 03 <4 opaque> 00000000` |
| 26 | `000001 <cid> 0000 <cid> 01 4b 00×7 <8 opaque>` |

The 8-byte tail of the last message differs per server run
(`cd61c4c5b4fd5bc7`, `5e21591d55a4cfb4`, …). Rust now generates its own.

What the client sends to Rust after its 307-byte ticket:

```
00000100000a000a000a0000000000000000
```

18 bytes, subtype `0x0a` at byte 9. The official server instead receives a
139-byte subtype `0x02` frame. Rust answers the `0x0a` with the 42-byte ack
captured from the official server:

```
000001 01 000000 <cid> 04 0000000000000000 3323ffff0f7bfeff0518e8f7 3323ffff0f7bfeff0518e8f7
```

That 32-byte body is also session-specific in the captures, so the constant is
almost certainly wrong.

## Tests run (each with one real client join)

| Send on connect | Reply to client `0x0a` | Result |
|---|---|---|
| challenge only, mod request after auth | 10-byte ack | suspicious message |
| challenge + 3 setup frames + mod request | 42-byte ack | suspicious message |
| same | nothing | suspicious message |

The complaint appears in all three, so it is not caused by the reply, and the
setup frames are not required to reach the mod step.

## Gate located in the client binary (2026-11-24)

`[MP] Warning: Client - suspicious message from %hu - type %u - %u.` lives at
`0x2baa6a0`; its reporter is at `0x1f835c0`. An `LD_PRELOAD` hook that patches
that function (source kept outside the repo) showed the failing call site:

```
0x1f881fd  (check at 0x1f881cb)
```

Which decodes as:

- `1e3d880(buf, 0x12, 8)` returns true only when `buf.len - 8 >= 18`, i.e. the
  buffered server reply must be **at least 26 bytes**.
- the client then compares `buf[18..26]` with a value it computed earlier
  (`rbp-0x1c0`, filled from the "server2client" channel).

So the reply must be >=26 bytes and carry the right 8-byte token at offset 18.
An 18-byte reply is too short; that is why the official-looking 18-byte ack
never satisfied it.

### Variants tried (all with a real client)

| Opening frames | Reply to the `0x0a` frame | Result |
|---|---|---|
| directed `f7ff0100 02` | 10-byte ack | suspicious `type 30` |
| directed | 42-byte, official token | suspicious `type 30` |
| directed | 18-byte `…0d` ack | suspicious `type 30` |
| broadcast `f7ff ffff 02` | 42-byte, client's token echoed | **no warning**, silent close |
| broadcast | 18-byte `…0d` ack | two `type 1` warnings from 65535, then `type 30` |
| broadcast | 42-byte + extra `0x01/06` frame | `type 1` |
| none | 42-byte, client's token echoed | suspicious `type 30` |

Best observed state: broadcast setup frames plus the 42-byte ack with the
client's own token echoed at 18 and 30. The client accepts every message and
then closes the connection without a warning, so the next unknown is what it
waits for after the mod exchange rather than what it rejects.

The hook also crashed the client twice while reading the compared values, so
that instrumentation needs a fault guard before it is used again.

## Solved: the mod-request token (2026-11-25)

The gate rejects the **server's 26-byte mod request**, not any reply. Bytes
18..26 must be:

```
CityHash64("server2client" || UPPER_HEX(SHA256(steam_id as u64 LE)))
```

`client2server` is the mirror image, carried by the client's mod list. Verified
against a capture of a real join (SteamID `85568392937043937` gives
`cd61c4c5b4fd5bc7` and `c6c14336b4789ded`); see `crates/convoy-server/src/token.rs`.

With the correct token the client sends its 128-byte mod list instead of
aborting, and no longer logs `suspicious message`.

## Remaining: world state after the mod list

The official server answers the mod list with a stream of entity frames: 45-byte
`0x02` and 26/56/90-byte `0x06` messages for every entity in the session,
followed by the 18-byte `0x01/0x05` connect ack. A single fabricated frame is
not enough; the client closes with reason 9. Implementing entity replication is
the next milestone.

## Next steps

1. Determine what "type 30" refers to in the client. The message appears even
   when the only server message sent is the 7-byte challenge, so start by
   testing a build that sends *nothing* on connect and let the client speak
   first.
2. Derive the 8-byte token in the mod request and the 32-byte body of the ack.
   Both vary per run while the mod set stays empty, so they are probably keyed
   by something the server holds (session or server SteamID) rather than a mod
   hash.
3. Capture a real client joining the official server with
   `tools/netcap-client.c` in `LD_PRELOAD` mode and align both directions frame
   by frame, instead of comparing probes. That interposer hooks `SteamAPI_Init`
   (the dedicated-server variant hooks `SteamInternal_GameServer_Init`, which
   the game never calls) and patches the client's `ISteamNetworkingSockets`
   vtable. It compiles but has not been run against the game yet. The official real-client capture shows the client sending
   a 128-byte subtype-`0x02` mod list carrying `3323ffff0f7bfeff` at 18..26,
   while the client replies to Rust with an 18-byte subtype-`0x0a` frame full of
   zeros. Fixing that divergence is the likely root cause: the client never
   reaches its normal mod-list state against Rust.