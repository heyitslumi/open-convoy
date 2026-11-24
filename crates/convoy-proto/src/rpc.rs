//! Remote procedure calls the convoy server and clients exchange.
//!
//! Variant names and payloads mirror the `prism::mp_rpc<...>` signatures
//! recovered from ETS2 RTTI (Pass 3). The wire encoding of RPCs — type ids vs
//! names, and how `mp_rpc` is framed on the SteamNetworking channel — is not
//! known yet, so these are plain data types with no codec.

/// `mp_traffic_t` — server-authoritative traffic arbitration.
///
/// The brief's heading says 13 variants but enumerates 12; the 13th RTTI entry,
/// `request_transfer_approval(mp_id, u16 x3)`, is left out because the brief
/// does not define its field semantics.
#[derive(Debug, Clone, PartialEq)]
pub enum TrafficRpc {
    RequestSpawnApproval {
        road_id: u16,
        item_id: u64,
        flags: u8,
    },
    ConfirmSpawn {
        item_id: u64,
        flags: u8,
        variant: u32,
    },
    RequestTransferStart {
        from_id: u32,
        to_id: u32,
        road_id: u16,
    },
    ConfirmTransferApproval {
        id: u32,
        status: bool,
    },
    ConfirmVehicleTransfer {
        id: u32,
        road_id: u16,
    },
    EnableSpawning(bool),
    RemoveTrafficVehicle(u32),
    SetTrafficSettings {
        max_total: u16,
        max_player: u16,
        max_spawn: u16,
    },
    AddOccupantSingleTrackRoad {
        road_id: u16,
        item_id: u64,
    },
    RemoveOccupantSingleTrackRoad {
        road_id: u16,
        item_id: u64,
    },
    UpdateSingleTrackRoad {
        road_id: u16,
        state: u64,
    },
    RequestTransferStartPlaced {
        from_id: u32,
        to_id: u32,
        pos: (f32, f32, f32),
    },
}

/// `mp_weather_t` — server-driven weather cycle sync.
#[derive(Debug, Clone, PartialEq)]
pub enum WeatherRpc {
    InitRemoteCycles {
        t1: u32,
        t2: u32,
        t3: u32,
        rain_factor: f32,
        c1: u32,
        c2: u32,
    },
    ReinitRemoteCycles {
        t1: u32,
        t2: u32,
        t3: u32,
        rain_factor: f32,
    },
    RandomizeWeather {
        seed: u32,
        force: bool,
    },
    UpdateNextCycle(u32),
}
