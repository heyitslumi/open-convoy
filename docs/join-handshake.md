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

## Next steps

1. Determine what "type 30" refers to in the client. The message appears even
   when the only server message sent is the 7-byte challenge, so start by
   testing a build that sends *nothing* on connect and let the client speak
   first.
2. Derive the 8-byte token in the mod request and the 32-byte body of the ack.
   Both vary per run while the mod set stays empty, so they are probably keyed
   by something the server holds (session or server SteamID) rather than a mod
   hash.
3. Capture a real client joining the official server with the interposer in
   `LD_PRELOAD` mode and align both directions frame by frame, instead of
   comparing probes.