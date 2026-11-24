use std::collections::HashMap;

use convoy_proto::rpc::TrafficRpc;

/// Maximum number of active vehicles per single road before overcrowding kicks in.
pub const MAX_ROAD_OCCUPANTS: usize = 16;

/// State-authoritative traffic arbitration and road occupancy manager.
pub struct TrafficManager {
    /// Road IDs mapped to active occupants (item_ids).
    road_occupants: HashMap<u16, Vec<u64>>,
    /// Active vehicles across the world: item_id -> owner client ID.
    active_vehicles: HashMap<u64, u16>,
    /// Pending transfer requests: vehicle_id (u32) -> (from_id, to_id).
    pending_transfers: HashMap<u32, (u32, u32)>,
}

impl TrafficManager {
    pub fn new() -> Self {
        Self {
            road_occupants: HashMap::new(),
            active_vehicles: HashMap::new(),
            pending_transfers: HashMap::new(),
        }
    }

    /// Owner client ID of active vehicle, if tracked.
    pub fn vehicle_owner(&self, item_id: u64) -> Option<u16> {
        self.active_vehicles.get(&item_id).copied()
    }

    /// Spawn arbitration: approve if item_id not already spawned and road not overcrowded.
    pub fn handle_spawn_request(
        &mut self,
        sender_cid: u16,
        road_id: u16,
        item_id: u64,
        flags: u8,
    ) -> Option<TrafficRpc> {
        if self.active_vehicles.contains_key(&item_id) {
            return None;
        }

        let occupants = self.road_occupants.entry(road_id).or_default();
        if occupants.len() >= MAX_ROAD_OCCUPANTS {
            return None;
        }

        occupants.push(item_id);
        self.active_vehicles.insert(item_id, sender_cid);

        Some(TrafficRpc::ConfirmSpawn {
            item_id,
            flags,
            variant: 0,
        })
    }

    /// Register occupancy on a road.
    pub fn add_occupant(&mut self, road_id: u16, item_id: u64) {
        let occupants = self.road_occupants.entry(road_id).or_default();
        if !occupants.contains(&item_id) {
            occupants.push(item_id);
        }
    }

    /// Unregister occupancy on a road.
    pub fn remove_occupant(&mut self, road_id: u16, item_id: u64) {
        if let Some(occupants) = self.road_occupants.get_mut(&road_id) {
            occupants.retain(|&id| id != item_id);
            if occupants.is_empty() {
                self.road_occupants.remove(&road_id);
            }
        }
        self.active_vehicles.remove(&item_id);
    }

    /// Check if road is occupied.
    pub fn is_occupied(&self, road_id: u16) -> bool {
        self.road_occupants
            .get(&road_id)
            .map_or(false, |occupants| !occupants.is_empty())
    }

    /// Request transfer of vehicle between players across visibility bubbles.
    pub fn request_transfer_start(
        &mut self,
        from_id: u32,
        to_id: u32,
        road_id: u16,
        vehicle_id: u32,
    ) -> Option<TrafficRpc> {
        let item_id = vehicle_id as u64;
        if !self.active_vehicles.contains_key(&item_id) {
            return None;
        }

        self.pending_transfers.insert(vehicle_id, (from_id, to_id));

        Some(TrafficRpc::RequestTransferStart {
            from_id,
            to_id,
            road_id,
        })
    }

    /// Confirm approval of vehicle transfer and update ownership.
    pub fn confirm_transfer_approval(&mut self, id: u32, status: bool) -> TrafficRpc {
        if let Some((_from, to)) = self.pending_transfers.remove(&id) {
            if status {
                self.active_vehicles.insert(id as u64, to as u16);
            }
        }
        TrafficRpc::ConfirmTransferApproval { id, status }
    }
}

impl Default for TrafficManager {
    fn default() -> Self {
        Self::new()
    }
}
