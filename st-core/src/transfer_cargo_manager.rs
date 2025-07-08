use anyhow::Result;
use itertools::Itertools;
use metrics::IntoF64;
use st_domain::cargo_transfer::TransferCargoError::{ReceiveShipDoesntExist, SendingUpdateMessageFailed};
use st_domain::cargo_transfer::{
    HaulerTransferSummary, InternalTransferCargoRequest, InternalTransferCargoResponse, InternalTransferCargoToHaulerResult, TransferCargoError,
};
use st_domain::{Cargo, Inventory, ShipSymbol, WaypointSymbol};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio::sync::Mutex;

pub struct TransferCargoManager {
    // Haulers waiting at each location
    waiting_haulers: Arc<Mutex<HashMap<WaypointSymbol, HashMap<ShipSymbol, (HaulerTransferSummary, Sender<(ShipSymbol, Cargo)>)>>>>,
}

impl Default for TransferCargoManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TransferCargoManager {
    pub fn new() -> Self {
        Self {
            waiting_haulers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn register_hauler_for_pickup_and_wait_until_full(
        &self,
        waypoint_symbol: WaypointSymbol,
        hauler_ship_symbol: ShipSymbol,
        hauler_cargo: Cargo,
        hauler_cargo_updated_channel: Sender<(ShipSymbol, Cargo)>,
    ) -> Result<HaulerTransferSummary> {
        // we wait and semantically block for transfers until we're full enough (80%)
        // then we yield the updated cargo of the hauler
        {
            let mut guard = self.waiting_haulers.lock().await;
            guard
                .entry(waypoint_symbol.clone())
                .or_default()
                .insert(hauler_ship_symbol.clone(), (hauler_cargo.into(), hauler_cargo_updated_channel.clone()));
        }

        let summary = loop {
            let mut guard = self.waiting_haulers.lock().await;

            if let Some(ships_at_waypoint) = guard.get(&waypoint_symbol).cloned() {
                if let Some((summary, _)) = ships_at_waypoint.get(&hauler_ship_symbol) {
                    let cargo = &summary.cargo;
                    let fill_amount: f64 = cargo.units.into_f64() / cargo.capacity.into_f64();

                    if fill_amount > 0.8 {
                        guard
                            .get_mut(&waypoint_symbol)
                            .unwrap()
                            .remove(&hauler_ship_symbol);
                        break summary.clone();
                    }
                }
            }

            // drop the guard immediately to prevent unnecessary waiting for other ships
            drop(guard);

            // now sleep for checking in later
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        };

        // poll regularly until cargo is full enough and remove ourselves from the list again

        Ok(summary)
    }

    pub async fn try_to_transfer_cargo_until_available_space<F, Fut>(
        &self,
        sending_ship: ShipSymbol,
        waypoint_symbol: WaypointSymbol,
        miner_cargo: Cargo,
        execute_cargo_transfer_fn: F,
    ) -> Result<InternalTransferCargoToHaulerResult, TransferCargoError>
    where
        F: Fn(InternalTransferCargoRequest) -> Fut,
        Fut: Future<Output = Result<InternalTransferCargoResponse, TransferCargoError>>,
    {
        {
            let mut guard = self.waiting_haulers.lock().await;
            let ships_at_this_waypoint = guard.entry(waypoint_symbol).or_default();
            let transfer_tasks = find_transfer_tasks(sending_ship, miner_cargo.clone(), ships_at_this_waypoint);

            let mut successful_tasks = vec![];
            let mut updated_miner_cargo = miner_cargo.clone();
            for transfer_task in transfer_tasks {
                let result = execute_cargo_transfer_fn(transfer_task.clone()).await?;

                updated_miner_cargo = result.sending_ship_cargo.clone();

                if let Some((summary, cargo_updated_tx)) = ships_at_this_waypoint.get_mut(&result.receiving_ship) {
                    summary.update_from_event(&result, &transfer_task);
                    cargo_updated_tx
                        .send((result.receiving_ship.clone(), result.receiving_ship_cargo))
                        .await
                        .map_err(|_| SendingUpdateMessageFailed)?
                } else {
                    return Err(ReceiveShipDoesntExist);
                }

                successful_tasks.push(transfer_task);
            }
            if successful_tasks.is_empty() {
                Ok(InternalTransferCargoToHaulerResult::NoMatchingShipFound)
            } else {
                Ok(InternalTransferCargoToHaulerResult::Success {
                    updated_miner_cargo,
                    transfer_tasks: successful_tasks,
                })
            }
        }
    }
}

fn find_transfer_task(
    sending_ship: ShipSymbol,
    cargo_item_to_transfer: Inventory,
    waiting_haulers: &HashMap<ShipSymbol, (HaulerTransferSummary, Sender<(ShipSymbol, Cargo)>)>,
) -> Option<InternalTransferCargoRequest> {
    let scored_haulers = waiting_haulers
        .iter()
        .map(|(ss, (summary, _sender))| {
            let hauler_cargo = &summary.cargo;
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

    maybe_best_hauler.map(|(ss, _, _, _)| InternalTransferCargoRequest {
        sending_ship,
        receiving_ship: ss.clone(),
        trade_good_symbol: cargo_item_to_transfer.symbol.clone(),
        units: cargo_item_to_transfer.units,
    })
}

fn find_transfer_tasks(
    miner_ship_symbol: ShipSymbol,
    miner_cargo: Cargo,
    waiting_haulers: &HashMap<ShipSymbol, (HaulerTransferSummary, Sender<(ShipSymbol, Cargo)>)>,
) -> Vec<InternalTransferCargoRequest> {
    let mut hauler_cargos = waiting_haulers.clone();

    let mut tasks = vec![];
    for inventory in miner_cargo.inventory {
        if let Some(task) = find_transfer_task(miner_ship_symbol.clone(), inventory, &hauler_cargos) {
            if let Some((summary, _sender)) = hauler_cargos.get_mut(&task.receiving_ship) {
                if let Ok(()) = summary
                    .cargo
                    .with_item_added_mut(task.trade_good_symbol.clone(), task.units)
                {
                    tasks.push(task);
                }
            }
        }
    }

    tasks
}

#[cfg(test)]
mod tests {
    use crate::transfer_cargo_manager::{find_transfer_tasks, TransferCargoManager};
    use itertools::Itertools;
    use st_domain::cargo_transfer::{
        HaulerTransferSummary, InternalTransferCargoRequest, InternalTransferCargoResponse, InternalTransferCargoToHaulerResult, TransferCargoError,
    };
    use st_domain::{Cargo, Inventory, ShipSymbol, TradeGoodSymbol, WaypointSymbol};
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::mpsc::Sender;
    use tokio::sync::{mpsc, oneshot, Mutex};
    use tokio::time::timeout;

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

        let (tx, mut rx) = tokio::sync::mpsc::channel::<(ShipSymbol, Cargo)>(32);

        let waiting_haulers: HashMap<ShipSymbol, (HaulerTransferSummary, Sender<(ShipSymbol, Cargo)>)> = HashMap::from([
            (ShipSymbol("HAULER_1".to_string()), (create_test_cargo(&vec![], 80).into(), tx.clone())),
            (
                ShipSymbol("HAULER_2_WITH_IRON_ORE_AND_COPPER_ORE".to_string()),
                (mixed_cargo_iron_and_copper.into(), tx.clone()),
            ),
            (
                ShipSymbol("HAULER_2_WITH_IRON_ORE".to_string()),
                (cargo_20_out_of_80_iron_ore.into(), tx.clone()),
            ),
        ]);

        let iron_ore_entry_40_units = Inventory {
            symbol: TradeGoodSymbol::IRON_ORE,
            units: 40,
        };
        let miner_cargo = create_test_cargo(&vec![iron_ore_entry_40_units.clone()], 40);

        let tasks = find_transfer_tasks(ShipSymbol("MINER".to_string()), miner_cargo, &waiting_haulers);

        assert_eq!(
            tasks,
            vec![InternalTransferCargoRequest {
                sending_ship: ShipSymbol("MINER".to_string()),
                receiving_ship: ShipSymbol("HAULER_2_WITH_IRON_ORE".to_string()),
                trade_good_symbol: TradeGoodSymbol::IRON_ORE,
                units: 40
            }]
        );
    }

    #[tokio::test]
    async fn test_transfer_cargo() -> Result<(), TransferCargoError> {
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

        let transfer_manager = Arc::new(TransferCargoManager::new());

        let waypoint = WaypointSymbol("WP1".to_string());

        let ships = vec![
            (ShipSymbol("HAULER_1".to_string()), create_test_cargo(&vec![], 80)),
            (ShipSymbol("HAULER_2_WITH_IRON_ORE_AND_COPPER_ORE".to_string()), mixed_cargo_iron_and_copper),
            (ShipSymbol("HAULER_2_WITH_IRON_ORE".to_string()), cargo_40_out_of_80_iron_ore),
        ];

        let (tx, mut rx) = tokio::sync::mpsc::channel::<(ShipSymbol, Cargo)>(32);

        // Create cancellation channel
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();

        // Spawn the message collector
        let message_collector = tokio::spawn(collect_messages_with_cancellation(rx, cancel_rx));

        let handles = ships
            .iter()
            .map(|(ss, cargo)| {
                let manager = transfer_manager.clone();
                let wp = waypoint.clone();
                let ship_symbol = ss.clone();
                let cargo = cargo.clone();

                tokio::spawn({
                    let tx_clone = tx.clone();
                    async move { wait_until_cargo_is_full(manager, wp, ship_symbol, cargo, tx_clone.clone()).await }
                })
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
                    InternalTransferCargoToHaulerResult::Success { .. } => {
                        break result;
                    }
                    InternalTransferCargoToHaulerResult::NoMatchingShipFound => {
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
            InternalTransferCargoToHaulerResult::Success {
                updated_miner_cargo: create_test_cargo(&vec![], 40),
                transfer_tasks: vec![InternalTransferCargoRequest {
                    sending_ship: ShipSymbol("MINER".to_string()),
                    receiving_ship: ShipSymbol("HAULER_2_WITH_IRON_ORE".to_string()),
                    trade_good_symbol: TradeGoodSymbol::IRON_ORE,
                    units: 40
                }]
            }
        );

        let (completed, _index, _remaining) = futures::future::select_all(handles).await;
        let (winner_name, final_cargo) = completed.unwrap().unwrap();

        println!("Got transfer result {transfer_result:?} and a winner {winner_name}");
        println!("Sending cancellation");
        cancel_tx.send(()).unwrap(); // Send cancellation signal
        let collected_messages = message_collector.await.unwrap();

        println!("Collected {} messages", collected_messages.len());

        assert_eq!(winner_name, ShipSymbol("HAULER_2_WITH_IRON_ORE".to_string()));
        assert_eq!(final_cargo.transfers.len(), 1);
        assert_eq!(final_cargo.cargo.units, 80);
        assert_eq!(final_cargo.cargo.inventory, vec![Inventory::new(TradeGoodSymbol::IRON_ORE, 80)]);

        Ok(())
    }

    async fn wait_until_cargo_is_full(
        transfer_manager: Arc<TransferCargoManager>,
        waypoint_symbol: WaypointSymbol,
        ship_symbol: ShipSymbol,
        cargo: Cargo,
        cargo_updated_tx: Sender<(ShipSymbol, Cargo)>,
    ) -> anyhow::Result<(ShipSymbol, HaulerTransferSummary)> {
        let updated_summary = transfer_manager
            .register_hauler_for_pickup_and_wait_until_full(waypoint_symbol.clone(), ship_symbol.clone(), cargo.clone(), cargo_updated_tx.clone())
            .await?;

        Ok((ship_symbol.clone(), updated_summary))
    }

    struct TestCargoTransferFoo {
        cargo_entries: Arc<Mutex<HashMap<ShipSymbol, Cargo>>>,
    }

    impl TestCargoTransferFoo {
        async fn transfer(&self, request: InternalTransferCargoRequest) -> anyhow::Result<InternalTransferCargoResponse, TransferCargoError> {
            let mut guard = self.cargo_entries.lock().await;

            let from_cargo = guard
                .get(&request.sending_ship)
                .ok_or(TransferCargoError::SendingShipDoesntExist)?;

            let to_cargo = guard
                .get(&request.receiving_ship)
                .ok_or(TransferCargoError::ReceiveShipDoesntExist)?;

            let new_from = from_cargo
                .clone()
                .with_units_removed(request.trade_good_symbol.clone(), request.units)
                .map_err(|_| TransferCargoError::NotEnoughItemsInSendingShipCargo)?;

            let new_to = to_cargo
                .clone()
                .with_item_added(request.trade_good_symbol.clone(), request.units)
                .map_err(|_| TransferCargoError::NotEnoughSpaceInReceivingShip)?;

            guard.insert(request.sending_ship.clone(), new_from.clone());
            guard.insert(request.receiving_ship.clone(), new_to.clone());

            let response = InternalTransferCargoResponse {
                receiving_ship: request.receiving_ship.clone(),
                trade_good_symbol: request.trade_good_symbol.clone(),
                units: request.units,
                sending_ship_cargo: new_from,
                receiving_ship_cargo: new_to,
            };
            Ok(response)
        }
    }

    async fn collect_messages_with_cancellation<T>(mut rx: mpsc::Receiver<T>, mut cancel_rx: oneshot::Receiver<()>) -> Vec<T> {
        let mut messages = Vec::new();

        loop {
            tokio::select! {
                // Try to receive a message
                msg = rx.recv() => {
                    match msg {
                        Some(message) => messages.push(message),
                        None => break, // Channel closed
                    }
                }
                // Check for cancellation signal
                _ = &mut cancel_rx => {
                    break; // Cancellation received
                }
            }
        }

        messages
    }
}
