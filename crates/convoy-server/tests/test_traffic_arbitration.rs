use convoy_proto::rpc::{TrafficRpc, WeatherRpc};
use convoy_server::traffic::{TrafficManager, MAX_ROAD_OCCUPANTS};
use convoy_server::weather::WeatherManager;

#[test]
fn test_spawn_approval_first_approved_duplicate_rejected() {
    let mut tm = TrafficManager::new();
    let sender_cid = 10;
    let road_id = 42;
    let item_id = 1001;
    let flags = 1;

    let res1 = tm.handle_spawn_request(sender_cid, road_id, item_id, flags);
    assert_eq!(
        res1,
        Some(TrafficRpc::ConfirmSpawn {
            item_id,
            flags,
            variant: 0,
        })
    );

    // Duplicate spawn request with same item_id rejected
    let res2 = tm.handle_spawn_request(sender_cid, road_id, item_id, flags);
    assert_eq!(res2, None);
}

#[test]
fn test_single_track_road_occupancy() {
    let mut tm = TrafficManager::new();
    let road_id = 100;
    let item_1 = 5001;
    let item_2 = 5002;

    assert!(!tm.is_occupied(road_id));

    tm.add_occupant(road_id, item_1);
    assert!(tm.is_occupied(road_id));

    tm.add_occupant(road_id, item_2);
    assert!(tm.is_occupied(road_id));

    tm.remove_occupant(road_id, item_1);
    assert!(tm.is_occupied(road_id));

    tm.remove_occupant(road_id, item_2);
    assert!(!tm.is_occupied(road_id));
}

#[test]
fn test_road_occupancy_removal_frees_up_spawn_capacity() {
    let mut tm = TrafficManager::new();
    let road_id = 55;

    // Fill road to capacity
    for i in 0..MAX_ROAD_OCCUPANTS {
        let res = tm.handle_spawn_request(10, road_id, (i + 1) as u64, 0);
        assert!(res.is_some());
    }

    // Overcrowded: next spawn request fails
    let overcrowded_id = (MAX_ROAD_OCCUPANTS + 1) as u64;
    let fail_res = tm.handle_spawn_request(10, road_id, overcrowded_id, 0);
    assert_eq!(fail_res, None);

    // Free up space by removing an occupant
    tm.remove_occupant(road_id, 1);

    // Now spawn succeeds
    let retry_res = tm.handle_spawn_request(10, road_id, overcrowded_id, 0);
    assert_eq!(
        retry_res,
        Some(TrafficRpc::ConfirmSpawn {
            item_id: overcrowded_id,
            flags: 0,
            variant: 0,
        })
    );
}

#[test]
fn test_vehicle_transfer_approval_updates_ownership() {
    let mut tm = TrafficManager::new();
    let from_id = 10;
    let to_id = 11;
    let road_id = 7;
    let vehicle_id = 99;

    // Register active vehicle first via spawn
    tm.handle_spawn_request(from_id as u16, road_id, vehicle_id as u64, 0);
    assert_eq!(tm.vehicle_owner(vehicle_id as u64), Some(from_id as u16));

    let start_rpc = tm.request_transfer_start(from_id, to_id, road_id, vehicle_id);
    assert_eq!(
        start_rpc,
        Some(TrafficRpc::RequestTransferStart {
            from_id,
            to_id,
            road_id,
        })
    );

    // Confirm transfer
    let confirm_rpc = tm.confirm_transfer_approval(vehicle_id, true);
    assert_eq!(
        confirm_rpc,
        TrafficRpc::ConfirmTransferApproval {
            id: vehicle_id,
            status: true,
        }
    );

    // Verify ownership transitioned to recipient player
    assert_eq!(tm.vehicle_owner(vehicle_id as u64), Some(to_id as u16));
}

#[test]
fn test_weather_cycle_step_and_rain_factor() {
    let mut wm = WeatherManager::new();

    let rain_rpc = wm.set_rain_factor(0.75);
    match rain_rpc {
        WeatherRpc::ReinitRemoteCycles { rain_factor, .. } => {
            assert!((rain_factor - 0.75).abs() < 1e-4);
        }
        other => panic!("expected ReinitRemoteCycles, got {other:?}"),
    }

    // Step weather timer
    let step_rpc = wm.step(3600.0);
    assert!(step_rpc.is_some());
    match step_rpc.unwrap() {
        WeatherRpc::UpdateNextCycle(_) => {}
        other => panic!("expected UpdateNextCycle, got {other:?}"),
    }
}
