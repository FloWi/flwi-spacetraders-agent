use anyhow::Result;
use itertools::Itertools;
use metrics::IntoF64;
use st_domain::{Cargo, Inventory, NotEnoughItemsInCargoError, ShipSymbol, TradeGoodSymbol, WaypointSymbol};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug)]
enum TransferError {
    ApiError(String),
    InsufficientCapacity,
    ShipNotFound,
    Cancelled,
}

struct CargoTransferManager {
    // Haulers waiting at each location
    waiting_haulers: Arc<Mutex<HashMap<WaypointSymbol, HashMap<ShipSymbol, Cargo>>>>,
}

#[derive(Clone, Debug, PartialEq)]
struct CargoTransferRequest {
    sending_ship: ShipSymbol,
    receiving_ship: ShipSymbol,
    trade_good_symbol: TradeGoodSymbol,
    units: u32,
}

#[derive(Clone, Debug, PartialEq)]
struct CargoTransferResponse {
    receiving_ship: ShipSymbol,
    trade_good_symbol: TradeGoodSymbol,
    units: u32,
    sending_ship_cargo: Cargo,
    receiving_ship_cargo: Cargo,
}

#[derive(Debug)]
enum CargoTransferError {
    SendingShipDoesntExist,
    ReceiveShipDoesntExist,
    NotEnoughItemsInSendingShipCargo,
    NotEnoughSpaceInReceivingShip,
}

impl CargoTransferManager {
    fn new() -> Self {
        Self {
            waiting_haulers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn register_hauler_for_pickup_and_wait_until_full(
        &self,
        waypoint_symbol: WaypointSymbol,
        hauler_ship_symbol: ShipSymbol,
        hauler_cargo: Cargo,
    ) -> Result<Cargo> {
        // we wait and semantically block for transfers until we're full enough (90%)
        // then we yield the new cargo
        {
            let mut guard = self.waiting_haulers.lock().await;
            guard
                .entry(waypoint_symbol.clone())
                .or_default()
                .insert(hauler_ship_symbol.clone(), hauler_cargo.clone());
        }

        let cargo = loop {
            let mut guard = self.waiting_haulers.lock().await;

            if let Some(ships_at_waypoint) = guard.get(&waypoint_symbol) {
                if let Some(cargo) = ships_at_waypoint.get(&hauler_ship_symbol) {
                    let fill_amount: f64 = cargo.units.into_f64() / cargo.capacity.into_f64();

                    if fill_amount > 0.8 {
                        break cargo.clone();
                    }
                }
            }
            drop(guard);
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        };

        // poll regularly until cargo is full enough and remove ourselves from the list again

        Ok(cargo)
    }

    async fn try_to_transfer_cargo_until_available_space<F, Fut>(
        &self,
        sending_ship: ShipSymbol,
        waypoint_symbol: WaypointSymbol,
        miner_cargo: Cargo,
        execute_cargo_transfer_fn: F,
    ) -> Result<TransferCargoResult, CargoTransferError>
    where
        F: Fn(CargoTransferRequest) -> Fut,
        Fut: Future<Output = Result<CargoTransferResponse, CargoTransferError>>,
    {
        {
            let mut guard = self.waiting_haulers.lock().await;
            let ships_at_this_waypoint = guard.entry(waypoint_symbol).or_default();
            let transfer_tasks = find_transfer_tasks(sending_ship, miner_cargo.clone(), ships_at_this_waypoint);

            let mut successful_tasks = vec![];
            let mut updated_miner_cargo = miner_cargo.clone();
            for transfer_task in transfer_tasks {
                let result = execute_cargo_transfer_fn(transfer_task.clone()).await?;

                updated_miner_cargo = result.sending_ship_cargo;
                ships_at_this_waypoint.insert(result.receiving_ship.clone(), result.receiving_ship_cargo.clone());
                successful_tasks.push(transfer_task);
            }
            if successful_tasks.is_empty() {
                Ok(TransferCargoResult::NoMatchingShipFound)
            } else {
                Ok(TransferCargoResult::Success {
                    updated_miner_cargo,
                    transfer_tasks: successful_tasks,
                })
            }
        }
    }
}

#[derive(PartialEq, Debug)]
enum TransferCargoResult {
    NoMatchingShipFound,
    Success {
        updated_miner_cargo: Cargo,
        transfer_tasks: Vec<CargoTransferRequest>,
    },
}

fn find_transfer_task(
    sending_ship: ShipSymbol,
    cargo_item_to_transfer: Inventory,
    waiting_haulers: &HashMap<ShipSymbol, Cargo>,
) -> Option<CargoTransferRequest> {
    let scored_haulers = waiting_haulers
        .iter()
        .map(|(ss, hauler_cargo)| {
            let space_left = (hauler_cargo.capacity - hauler_cargo.units) as u32;
            let has_space_left = space_left >= cargo_item_to_transfer.units;
            let hauler_cargo_items = hauler_cargo
                .inventory
                .iter()
                .map(|inv| inv.symbol.clone())
                .collect::<HashSet<_>>();
            let has_trade_symbol_in_cargo = hauler_cargo_items.contains(&cargo_item_to_transfer.symbol);
            let has_only_this_item_in_cargo = hauler_cargo_items == HashSet::from([cargo_item_to_transfer.symbol.clone()]);

            let score_matching_cargo = if has_trade_symbol_in_cargo { 1 } else { 0 };
            let score_only_matching_cargo = if has_only_this_item_in_cargo { 3 } else { 0 };
            let total_score = score_only_matching_cargo + score_matching_cargo;

            (ss, hauler_cargo, has_space_left, total_score)
        })
        .collect_vec();

    let maybe_best_hauler: Option<(&ShipSymbol, &Cargo, bool, i32)> = scored_haulers
        .iter()
        .filter(|(_, _, has_space_left, _)| *has_space_left)
        .max_by_key(|(_, _, _, total_score)| total_score)
        .cloned();

    maybe_best_hauler.map(|(ss, cargo, _, _)| CargoTransferRequest {
        sending_ship,
        receiving_ship: ss.clone(),
        trade_good_symbol: cargo_item_to_transfer.symbol.clone(),
        units: cargo_item_to_transfer.units,
    })
}

fn find_transfer_tasks(miner_ship_symbol: ShipSymbol, miner_cargo: Cargo, waiting_haulers: &HashMap<ShipSymbol, Cargo>) -> Vec<CargoTransferRequest> {
    let mut hauler_cargos = waiting_haulers.clone();

    let mut tasks = vec![];
    for inventory in miner_cargo.inventory {
        if let Some(task) = find_transfer_task(miner_ship_symbol.clone(), inventory, &hauler_cargos) {
            if let Some(hauler_cargo) = hauler_cargos.get_mut(&task.receiving_ship) {
                match hauler_cargo.with_item_added_mut(task.trade_good_symbol.clone(), task.units) {
                    Ok(()) => {
                        tasks.push(task);
                    }
                    Err(_) => {}
                }
            }
        }
    }

    tasks
}

#[cfg(test)]
mod tests {
    use crate::cargo_transfer_manager::{
        find_transfer_tasks, CargoTransferError, CargoTransferManager, CargoTransferRequest, CargoTransferResponse, TestCargoTransferFoo, TransferCargoResult,
    };
    use itertools::Itertools;
    use st_domain::{Cargo, Inventory, ShipSymbol, TradeGoodSymbol, WaypointSymbol};
    use std::collections::HashMap;
    use std::future::Future;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn create_test_cargo(items: &[Inventory], capacity: u32) -> Cargo {
        let mut cargo = Cargo {
            capacity: capacity as i32,
            units: 0,
            inventory: vec![],
        };

        for item in items {
            cargo.units += item.units as i32;
            cargo.inventory.push(item.clone());
        }

        cargo
    }

    #[test]
    //#[tokio::test] // for accessing runtime-infos with tokio-console
    fn test_find_transfer_tasks() {
        let cargo_20_out_of_80_iron_ore = create_test_cargo(
            &vec![Inventory {
                symbol: TradeGoodSymbol::IRON_ORE,
                units: 20,
            }],
            80,
        );

        let mixed_cargo_iron_and_copper = cargo_20_out_of_80_iron_ore
            .clone()
            .with_item_added(TradeGoodSymbol::COPPER_ORE, 20)
            .unwrap();

        let waiting_haulers = HashMap::from([
            (ShipSymbol("HAULER_1".to_string()), create_test_cargo(&vec![], 80)),
            (
                ShipSymbol("HAULER_2_WITH_IRON_ORE_AND_COPPER_ORE".to_string()),
                mixed_cargo_iron_and_copper.clone(),
            ),
            (ShipSymbol("HAULER_2_WITH_IRON_ORE".to_string()), cargo_20_out_of_80_iron_ore.clone()),
        ]);

        let iron_ore_entry_40_units = Inventory {
            symbol: TradeGoodSymbol::IRON_ORE,
            units: 40,
        };
        let miner_cargo = create_test_cargo(&vec![iron_ore_entry_40_units.clone()], 40);

        let tasks = find_transfer_tasks(ShipSymbol("MINER".to_string()), miner_cargo, &waiting_haulers);

        assert_eq!(
            tasks,
            vec![CargoTransferRequest {
                sending_ship: ShipSymbol("MINER".to_string()),
                receiving_ship: ShipSymbol("HAULER_2_WITH_IRON_ORE".to_string()),
                trade_good_symbol: TradeGoodSymbol::IRON_ORE,
                units: 40
            }]
        );
    }

    #[tokio::test]
    async fn test_transfer_cargo() -> Result<(), CargoTransferError> {
        let cargo_40_out_of_80_iron_ore = create_test_cargo(
            &vec![Inventory {
                symbol: TradeGoodSymbol::IRON_ORE,
                units: 40,
            }],
            80,
        );

        let mixed_cargo_iron_and_copper = cargo_40_out_of_80_iron_ore
            .clone()
            .with_item_added(TradeGoodSymbol::COPPER_ORE, 20)
            .unwrap();

        let iron_ore_entry_40_units = Inventory {
            symbol: TradeGoodSymbol::IRON_ORE,
            units: 40,
        };

        let miner_cargo = create_test_cargo(&vec![iron_ore_entry_40_units.clone()], 40);

        let transfer_manager = Arc::new(CargoTransferManager::new());

        let waypoint = WaypointSymbol("WP1".to_string());

        let ships = vec![
            (ShipSymbol("HAULER_1".to_string()), create_test_cargo(&vec![], 80)),
            (ShipSymbol("HAULER_2_WITH_IRON_ORE_AND_COPPER_ORE".to_string()), mixed_cargo_iron_and_copper),
            (ShipSymbol("HAULER_2_WITH_IRON_ORE".to_string()), cargo_40_out_of_80_iron_ore),
        ];

        let handles = ships
            .iter()
            .map(|(ss, cargo)| {
                let manager = transfer_manager.clone();
                let wp = waypoint.clone();
                let ship_symbol = ss.clone();
                let cargo = cargo.clone();

                tokio::spawn(async move { wait_until_cargo_is_full(manager, wp, ship_symbol, cargo).await })
            })
            .collect_vec();

        let miner_ship = vec![(ShipSymbol("MINER".to_string()), miner_cargo.clone())];

        let local_state: HashMap<ShipSymbol, Cargo> = ships.iter().chain(miner_ship.iter()).cloned().collect();

        let local_state_manager = TestCargoTransferFoo {
            cargo_entries: Arc::new(Mutex::new(local_state)),
        };

        let transfer_result = loop {
            let sleep_duration = tokio::time::Duration::from_millis(5);

            match transfer_manager
                .try_to_transfer_cargo_until_available_space(ShipSymbol("MINER".to_string()), waypoint.clone(), miner_cargo.clone(), |req| {
                    local_state_manager.transfer(req)
                })
                .await
            {
                Ok(result) => match result {
                    TransferCargoResult::Success { .. } => {
                        break result;
                    }
                    TransferCargoResult::NoMatchingShipFound => {
                        println!("NoMatchingShipFound yet - trying again in {sleep_duration:?}")
                    }
                },
                Err(e) => {
                    panic!("error: {e:?}");
                }
            }
            tokio::time::sleep(sleep_duration).await;
        };

        assert_eq!(
            transfer_result,
            TransferCargoResult::Success {
                updated_miner_cargo: create_test_cargo(&vec![], 40),
                transfer_tasks: vec![CargoTransferRequest {
                    sending_ship: ShipSymbol("MINER".to_string()),
                    receiving_ship: ShipSymbol("HAULER_2_WITH_IRON_ORE".to_string()),
                    trade_good_symbol: TradeGoodSymbol::IRON_ORE,
                    units: 40
                }]
            }
        );

        let (completed, _index, _remaining) = futures::future::select_all(handles).await;
        let (winner_name, final_cargo) = completed.unwrap().unwrap();

        assert_eq!(winner_name, ShipSymbol("HAULER_2_WITH_IRON_ORE".to_string()));
        assert_eq!(final_cargo.units, 80);
        assert_eq!(final_cargo.inventory, vec![Inventory::new(TradeGoodSymbol::IRON_ORE, 80)]);

        Ok(())
    }

    async fn wait_until_cargo_is_full(
        transfer_manager: Arc<CargoTransferManager>,
        waypoint_symbol: WaypointSymbol,
        ship_symbol: ShipSymbol,
        cargo: Cargo,
    ) -> anyhow::Result<(ShipSymbol, Cargo)> {
        let new_cargo = transfer_manager
            .register_hauler_for_pickup_and_wait_until_full(waypoint_symbol.clone(), ship_symbol.clone(), cargo.clone())
            .await?;

        Ok((ship_symbol.clone(), new_cargo))
    }
}

struct TestCargoTransferFoo {
    cargo_entries: Arc<Mutex<HashMap<ShipSymbol, Cargo>>>,
}

impl TestCargoTransferFoo {
    async fn transfer(&self, request: CargoTransferRequest) -> Result<CargoTransferResponse, CargoTransferError> {
        let mut guard = self.cargo_entries.lock().await;

        let from_cargo = guard
            .get(&request.sending_ship)
            .ok_or(CargoTransferError::SendingShipDoesntExist)?;

        let to_cargo = guard
            .get(&request.receiving_ship)
            .ok_or(CargoTransferError::ReceiveShipDoesntExist)?;

        let new_from = from_cargo
            .clone()
            .with_units_removed(request.trade_good_symbol.clone(), request.units)
            .map_err(|_| CargoTransferError::NotEnoughItemsInSendingShipCargo)?;

        let new_to = to_cargo
            .clone()
            .with_item_added(request.trade_good_symbol.clone(), request.units)
            .map_err(|_| CargoTransferError::NotEnoughSpaceInReceivingShip)?;

        guard.insert(request.sending_ship.clone(), new_from.clone());
        guard.insert(request.receiving_ship.clone(), new_to.clone());

        let response = CargoTransferResponse {
            receiving_ship: request.receiving_ship.clone(),
            trade_good_symbol: request.trade_good_symbol.clone(),
            units: request.units,
            sending_ship_cargo: new_from,
            receiving_ship_cargo: new_to,
        };
        Ok(response)
    }
}
