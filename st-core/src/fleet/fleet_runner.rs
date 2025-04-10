use crate::behavior_tree::behavior_args::{BehaviorArgs, DbBlackboard};
use crate::behavior_tree::behavior_tree::ActionEvent;
use crate::behavior_tree::ship_behaviors::ShipAction;
use crate::fleet;
use crate::fleet::fleet::{collect_fleet_decision_facts, compute_fleets_with_tasks, FleetAdmiral, NewTaskResult, ShipStatusReport};
use crate::fleet::ship_runner::ship_behavior_runner;
use crate::fleet::system_spawning_fleet::SystemSpawningFleet;
use crate::ship::ShipOperations;
use crate::st_client::StClientTrait;
use crate::test_objects::TestObjects;
use anyhow::{anyhow, Result};
use chrono::Utc;
use fleet::fleet::recompute_tasks_after_ship_finishing_behavior_tree;
use st_domain::{
    Agent, Fleet, FleetDecisionFacts, FleetPhaseName, FleetsOverview, Ship, ShipFrameSymbol, ShipSymbol, ShipTask, StationaryProbeLocation, TradeTicket,
    TransactionActionEvent,
};
use st_store::{db, Ctx, DbModelManager, FleetBmc, ShipBmc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{event, Instrument};
use tracing_core::Level;

pub struct FleetRunner {
    ship_fibers: HashMap<ShipSymbol, JoinHandle<Result<()>>>,
    ship_ops: HashMap<ShipSymbol, Arc<Mutex<ShipOperations>>>,
    //ship_updated_listener_join_handle: tokio::task::JoinHandle<anyhow::Result<()>>,
    ship_updated_tx: Sender<ShipOperations>,
    ship_action_completed_tx: Sender<ActionEvent>,
    ship_status_report_tx: Sender<ShipStatusReport>,
    client: Arc<dyn StClientTrait>,
    args: BehaviorArgs,
    fleet_admiral: Arc<Mutex<FleetAdmiral>>,
    db_model_manager: DbModelManager,
}

impl FleetRunner {
    pub async fn run_fleets(fleet_admiral: Arc<Mutex<FleetAdmiral>>, client: Arc<dyn StClientTrait>, db_model_manager: DbModelManager) -> Result<()> {
        event!(Level::INFO, "Running fleets");

        let (ship_updated_tx, ship_updated_rx): (Sender<ShipOperations>, Receiver<ShipOperations>) = tokio::sync::mpsc::channel(32);
        let (ship_action_completed_tx, ship_action_completed_rx): (Sender<ActionEvent>, Receiver<ActionEvent>) = tokio::sync::mpsc::channel(32);
        let (ship_status_report_tx, ship_status_report_rx): (Sender<ShipStatusReport>, Receiver<ShipStatusReport>) = tokio::sync::mpsc::channel(32);

        let args: BehaviorArgs = BehaviorArgs {
            blackboard: Arc::new(DbBlackboard {
                model_manager: db_model_manager.clone(),
            }),
        };

        let mut ship_fibers: HashMap<ShipSymbol, JoinHandle<Result<()>>> = HashMap::new();

        let mut ship_ops: HashMap<ShipSymbol, Arc<Mutex<ShipOperations>>> = Default::default();

        // Clone fleet_admiral infos to avoid the lifetime issues
        let all_ships_map = fleet_admiral.lock().await.all_ships.clone();

        let mut fleet_runner = Self {
            ship_fibers,
            ship_ops,
            ship_updated_tx,
            ship_action_completed_tx,
            ship_status_report_tx,
            client,
            args,
            fleet_admiral,
            db_model_manager,
        };

        let fleet_runner_mutex = Arc::new(Mutex::new(fleet_runner));

        Self::run_message_listeners(
            Arc::clone(&fleet_runner_mutex),
            ship_updated_rx,
            ship_action_completed_rx,
            ship_status_report_rx,
        )
        .await;

        for (ss, ship) in all_ships_map {
            Self::launch_and_register_ship(Arc::clone(&fleet_runner_mutex), &ss, ship).await?;
        }

        Ok(())
    }

    pub async fn launch_and_register_ship(runner: Arc<Mutex<FleetRunner>>, ss: &ShipSymbol, ship: Ship) -> Result<()> {
        // if ss.0 != "FLWI-26" {
        //     return Ok(());
        // }
        let mut guard = runner.lock().await;
        let ship_tasks = guard.fleet_admiral.lock().await.ship_tasks.clone();

        let ship_op_mutex = Arc::new(Mutex::new(ShipOperations::new(ship.clone(), Arc::clone(&guard.client))));
        let maybe_ship_task = ship_tasks.get(&ss);

        if let Some(ship_task) = maybe_ship_task {
            // Clone all the values that need to be moved into the async task
            let ship_op_clone = Arc::clone(&ship_op_mutex);
            let args_clone = guard.args.clone();
            let ship_updated_tx_clone = guard.ship_updated_tx.clone();
            let ship_action_completed_tx_clone = guard.ship_action_completed_tx.clone();
            let ship_status_report_tx_clone = guard.ship_status_report_tx.clone();
            let ship_task_clone = ship_task.clone();
            let ship_symbol_clone = ss.clone();

            let fiber = tokio::spawn(async move {
                let maybe_task_finished_result = Self::behavior_runner(
                    ship_op_clone,
                    args_clone,
                    ship_updated_tx_clone,
                    ship_action_completed_tx_clone,
                    ship_task_clone,
                )
                .await?;

                if let Some((ship, ship_task)) = maybe_task_finished_result {
                    ship_status_report_tx_clone.send(ShipStatusReport::ShipFinishedBehaviorTree(ship, ship_task)).await?;
                }

                Ok(())
            });

            guard.ship_fibers.insert(ship_symbol_clone, fiber);
        }
        guard.ship_ops.insert(ss.clone(), ship_op_mutex);
        Ok(())
    }

    //TODO - refactor to DRY up with fn launch_and_register_ship
    pub async fn relaunch_ship(runner: Arc<Mutex<FleetRunner>>, ss: &ShipSymbol) -> Result<()> {
        let mut guard = runner.lock().await;
        let ship_tasks = guard.fleet_admiral.lock().await.ship_tasks.clone();

        let ship_op_mutex = guard.ship_ops.get(ss).unwrap();
        let maybe_ship_task = ship_tasks.get(&ss);

        if let Some(ship_task) = maybe_ship_task {
            // Clone all the values that need to be moved into the async task
            let ship_op_clone = Arc::clone(&ship_op_mutex);
            let args_clone = guard.args.clone();
            let ship_updated_tx_clone = guard.ship_updated_tx.clone();
            let ship_action_completed_tx_clone = guard.ship_action_completed_tx.clone();
            let ship_status_report_tx_clone = guard.ship_status_report_tx.clone();
            let ship_task_clone = ship_task.clone();
            let ship_symbol_clone = ss.clone();

            let fiber = tokio::spawn(async move {
                let maybe_task_finished_result = Self::behavior_runner(
                    ship_op_clone,
                    args_clone,
                    ship_updated_tx_clone,
                    ship_action_completed_tx_clone,
                    ship_task_clone,
                )
                .await?;

                if let Some((ship, ship_task)) = maybe_task_finished_result {
                    ship_status_report_tx_clone.send(ShipStatusReport::ShipFinishedBehaviorTree(ship, ship_task)).await?;
                }

                Ok(())
            });

            guard.ship_fibers.insert(ship_symbol_clone, fiber);
        }
        Ok(())
    }

    pub async fn behavior_runner(
        ship_op: Arc<Mutex<ShipOperations>>,
        args: BehaviorArgs,
        ship_updated_tx: Sender<ShipOperations>,
        ship_action_completed_tx: Sender<ActionEvent>,
        ship_task: ShipTask,
    ) -> Result<Option<(Ship, ShipTask)>> {
        use crate::behavior_tree::behavior_tree::{Actionable, Response};
        use crate::behavior_tree::ship_behaviors::ship_behaviors;
        use anyhow::Error;
        use std::time::Duration;
        use tracing::{span, Level};
        let behaviors = ship_behaviors();

        let mut ship = ship_op.lock().await;

        let maybe_behavior = match ship_task.clone() {
            ShipTask::ObserveWaypointDetails { waypoint_symbol } => {
                ship.set_permanent_observation_location(waypoint_symbol);
                println!("ship_loop: Ship {:?} is running stationary_probe_behavior", ship.symbol);
                Some((behaviors.stationary_probe_behavior, "stationary_probe_behavior"))
            }
            ShipTask::ObserveAllWaypointsOnce { waypoint_symbols } => {
                ship.set_explore_locations(waypoint_symbols);
                println!("ship_loop: Ship {:?} is running explorer_behavior", ship.symbol);
                Some((behaviors.explorer_behavior, "explorer_behavior"))
            }
            ShipTask::MineMaterialsAtWaypoint { .. } => None,
            ShipTask::SurveyAsteroid { .. } => None,
            ShipTask::Trade { ticket_id } => {
                let ticket: TradeTicket = args.blackboard.get_ticket_by_id(ticket_id).await?;
                ship.set_trade_ticket(ticket);
                println!("ship_loop: Ship {:?} is running trading_behavior", ship.symbol);
                Some((behaviors.trading_behavior, "trading_behavior"))
            }
        };

        match maybe_behavior {
            None => Ok(None),
            Some((ship_behavior, behavior_label)) => {
                let ship_span = span!(Level::INFO, "ship_behavior", ship = format!("{}", ship.symbol.0), behavior = behavior_label);

                let result: Result<Response, Error> = ship_behavior_runner(
                    &mut ship,
                    Duration::from_secs(5),
                    &args,
                    ship_behavior,
                    &ship_updated_tx.clone(),
                    &ship_action_completed_tx.clone(),
                )
                .instrument(ship_span)
                .await;

                match &result {
                    Ok(o) => {
                        event!(
                            Level::INFO,
                            message = "behavior_runner done",
                            result = %o,
                        );
                        let ship_clone = ship.ship.clone();
                        Ok(Some((ship_clone, ship_task)))
                    }
                    Err(e) => {
                        event!(
                            Level::INFO,
                            message = "behavior_runner done with Error",
                            result = %e,
                        );
                        Err(anyhow!("behavior_runner done with Error: {}", e))
                    }
                }
            }
        }
    }

    pub async fn listen_to_ship_changes_and_persist(
        db_model_manager: DbModelManager,
        fleet_admiral: Arc<Mutex<FleetAdmiral>>,
        mut ship_updated_rx: Receiver<ShipOperations>,
    ) -> Result<()> {
        while let Some(updated_ship) = ship_updated_rx.recv().await {
            let mut admiral = fleet_admiral.lock().await;
            let maybe_old_ship = admiral.all_ships.get(&updated_ship.symbol).cloned();

            match maybe_old_ship {
                Some(old_ship) if old_ship == updated_ship.ship => {
                    // no need to update
                    //event!(Level::DEBUG, "No need to update ship {}. No change detected", updated_ship.symbol.0);
                }
                _ => {
                    //event!(Level::DEBUG, "Ship {} updated", updated_ship.symbol.0);
                    let _ = st_store::upsert_ships(db_model_manager.pool(), &vec![updated_ship.ship.clone()], Utc::now()).await?;
                    admiral.all_ships.insert(updated_ship.symbol.clone(), updated_ship.ship);
                }
            }
        }

        Ok(())
    }
    pub async fn listen_to_ship_status_report_messages(
        fleet_admiral: Arc<Mutex<FleetAdmiral>>,
        db_model_manager: DbModelManager,
        mut ship_status_report_rx: Receiver<ShipStatusReport>,
        runner: Arc<Mutex<FleetRunner>>,
    ) -> Result<()> {
        while let Some(msg) = ship_status_report_rx.recv().await {
            let mut admiral_guard = fleet_admiral.lock().await;
            admiral_guard.report_ship_action_completed(&msg, &db_model_manager).await?;

            match msg {
                ShipStatusReport::ShipFinishedBehaviorTree(ship, task) => {
                    let mut admiral_guard = fleet_admiral.lock().await;
                    admiral_guard.ship_tasks.remove(&ship.symbol);
                    let result = recompute_tasks_after_ship_finishing_behavior_tree(&admiral_guard, &ship, &task, &db_model_manager).await?;
                    event!(
                        Level::INFO,
                        message = "ShipFinishedBehaviorTree",
                        ship = ship.symbol.0,
                        recompute_result = result.to_string()
                    );
                    match result {
                        NewTaskResult::DismantleFleets { fleets_to_dismantle } => {
                            FleetAdmiral::dismantle_fleets(&mut admiral_guard, fleets_to_dismantle);
                        }
                        NewTaskResult::RegisterWaypointForPermanentObservation {
                            ship_symbol,
                            waypoint_symbol,
                            exploration_tasks,
                        } => {
                            let location = StationaryProbeLocation {
                                waypoint_symbol,
                                probe_ship_symbol: ship_symbol.clone(),
                                exploration_tasks,
                            };
                            ShipBmc::insert_stationary_probe(&Ctx::Anonymous, &db_model_manager, location.clone()).await?;
                            FleetAdmiral::add_stationary_probe_location(&mut admiral_guard, location);
                            FleetAdmiral::remove_ship_from_fleet(&mut admiral_guard, &ship_symbol);
                            FleetAdmiral::remove_ship_task(&mut admiral_guard, &ship_symbol);
                            Self::stop_ship(Arc::clone(&runner), &ship_symbol).await?;
                        }
                        NewTaskResult::AssignNewTaskToShip {
                            ship_symbol,
                            task,
                            ship_task_requirement,
                        } => {
                            FleetAdmiral::assign_ship_task_and_potential_requirement(&mut admiral_guard, ship_symbol.clone(), task, ship_task_requirement);
                            Self::relaunch_ship(runner.clone(), &ship_symbol).await?
                        }
                    }
                }

                ShipStatusReport::ShipActionCompleted(_, _) => {}
                ShipStatusReport::TransactionCompleted(_, transaction_event, _) => match &transaction_event {
                    TransactionActionEvent::PurchasedTradeGoods { .. } => {}
                    TransactionActionEvent::SoldTradeGoods { .. } => {}
                    TransactionActionEvent::SuppliedConstructionSite { .. } => {}
                    TransactionActionEvent::ShipPurchased { ticket_details, response } => {
                        let new_ship = response.data.ship.clone();
                        db::upsert_ships(db_model_manager.pool(), &vec![new_ship.clone()], Utc::now()).await?;
                        admiral_guard.all_ships.insert(new_ship.symbol.clone(), new_ship.clone());
                        admiral_guard.ship_fleet_assignment.insert(new_ship.symbol.clone(), ticket_details.assigned_fleet_id.clone());

                        let facts = collect_fleet_decision_facts(&db_model_manager, &new_ship.nav.system_symbol).await?;
                        let new_ship_tasks = FleetAdmiral::compute_ship_tasks(&mut admiral_guard, &facts, &db_model_manager).await?;
                        FleetAdmiral::assign_ship_tasks_and_potential_requirements(&mut admiral_guard, new_ship_tasks);
                        Self::launch_and_register_ship(Arc::clone(&runner), &new_ship.symbol, new_ship.clone()).await?
                    }
                },
            }
        }

        Ok(())
    }

    pub async fn listen_to_ship_action_update_messages(
        ship_status_report_tx: Sender<ShipStatusReport>,
        mut ship_action_completed_rx: Receiver<ActionEvent>,
    ) -> Result<()> {
        while let Some(msg) = ship_action_completed_rx.recv().await {
            match msg {
                ActionEvent::ShipActionCompleted(result) => match result {
                    Ok((ship_op, ship_action)) => {
                        let ss = ship_op.symbol.0.clone();
                        event!(
                            Level::INFO,
                            message = "ShipActionCompleted",
                            ship = ss,
                            action = %ship_action,
                        );
                        match ship_action {
                            ShipAction::CollectWaypointInfos => {
                                ship_status_report_tx.send(ShipStatusReport::ShipActionCompleted(ship_op.ship.clone(), ship_action)).await?;
                            }
                            _ => {}
                        }
                    }
                    Err(err) => {
                        event!(Level::ERROR, message = "Error completing ShipAction", error = %err,);
                    }
                },
                ActionEvent::BehaviorCompleted(result) => match result {
                    Ok(_) => {}
                    Err(_) => {}
                },
                ActionEvent::TransactionCompleted(ship, transaction, ticket) => {
                    ship_status_report_tx.send(ShipStatusReport::TransactionCompleted(ship.ship, transaction, ticket)).await?;
                }
            }
        }

        Ok(())
    }

    async fn run_message_listeners(
        runner: Arc<Mutex<FleetRunner>>,
        ship_updated_rx: Receiver<ShipOperations>,
        ship_action_completed_rx: Receiver<ActionEvent>,
        ship_status_report_rx: Receiver<ShipStatusReport>,
    ) {
        // Extract all needed data with a single lock acquisition
        let (db_model_manager, fleet_admiral, ship_status_report_tx) = {
            let guard = runner.lock().await;
            (
                guard.db_model_manager.clone(),
                Arc::clone(&guard.fleet_admiral),
                guard.ship_status_report_tx.clone(),
            )
        };

        let ship_updated_listener_join_handle = tokio::spawn(Self::listen_to_ship_changes_and_persist(
            db_model_manager.clone(),
            Arc::clone(&fleet_admiral),
            ship_updated_rx,
        ));

        let ship_action_update_listener_join_handle =
            tokio::spawn(Self::listen_to_ship_action_update_messages(ship_status_report_tx, ship_action_completed_rx));

        let ship_status_report_listener_join_handle = tokio::spawn(Self::listen_to_ship_status_report_messages(
            fleet_admiral,
            db_model_manager,
            ship_status_report_rx,
            Arc::clone(&runner),
        ));

        // run forever
        tokio::join!(
            ship_updated_listener_join_handle,
            ship_action_update_listener_join_handle,
            ship_status_report_listener_join_handle
        );

        unreachable!()
    }

    async fn stop_ship(fleet_runner: Arc<Mutex<FleetRunner>>, ship_symbol: &ShipSymbol) -> Result<()> {
        let mut guard = fleet_runner.lock().await;
        if let Some(join_handle) = guard.ship_fibers.get(ship_symbol) {
            join_handle.abort();
        };
        guard.ship_fibers.remove(ship_symbol);
        guard.ship_ops.remove(ship_symbol);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;
    use st_domain::{FleetDecisionFacts, FleetPhaseName, FleetsOverview, ShipFrameSymbol, ShipSymbol, WaypointTraitSymbol, WaypointType};
    use std::collections::HashMap;

    use crate::fleet::fleet::{compute_fleets_with_tasks, FleetAdmiral};
    use crate::test_objects::TestObjects;
    use test_log::test;

    #[test(tokio::test)]
    async fn create_fleet_admiral_from_startup_ship_config() {
        let mut command_ship = TestObjects::test_ship(600);
        command_ship.frame.symbol = ShipFrameSymbol::FRAME_FRIGATE;
        command_ship.symbol = ShipSymbol("FLWI-1".to_string());

        let mut probe = TestObjects::test_ship(0);
        probe.frame.symbol = ShipFrameSymbol::FRAME_PROBE;
        probe.fuel.capacity = 0;
        probe.symbol = ShipSymbol("FLWI-2".to_string());

        let ship_map = HashMap::from([(command_ship.symbol.clone(), command_ship), (probe.symbol.clone(), probe)]);

        let overview = FleetsOverview {
            completed_fleet_tasks: vec![],
            fleets: Default::default(),
            all_ships: ship_map.clone(),
            fleet_task_assignments: Default::default(),
            ship_fleet_assignment: Default::default(),
            ship_tasks: Default::default(),
            open_trade_tickets: Default::default(),
            stationary_probe_locations: vec![],
        };
        let market_1 = TestObjects::create_waypoint(
            &TestObjects::system_symbol().with_waypoint_suffix("MARKET1"),
            100,
            100,
            vec![WaypointTraitSymbol::MARKETPLACE],
        );
        let mut jump_gate_1 = TestObjects::create_waypoint(
            &TestObjects::system_symbol().with_waypoint_suffix("JUMP_GATE"),
            100,
            100,
            vec![WaypointTraitSymbol::MARKETPLACE],
        );
        jump_gate_1.r#type == WaypointType::JUMP_GATE;
        let shipyard_1 = TestObjects::create_waypoint(
            &TestObjects::system_symbol().with_waypoint_suffix("SHIPYARD1"),
            200,
            200,
            vec![WaypointTraitSymbol::MARKETPLACE, WaypointTraitSymbol::SHIPYARD],
        );

        let waypoints = vec![market_1.clone(), jump_gate_1.clone(), shipyard_1.clone()];

        let facts = FleetDecisionFacts {
            marketplaces_of_interest: vec![market_1.symbol.clone(), shipyard_1.symbol.clone()],
            marketplaces_with_up_to_date_infos: vec![],
            shipyards_of_interest: vec![shipyard_1.symbol.clone()],
            shipyards_with_up_to_date_infos: vec![],
            construction_site: Some(TestObjects::startup_construction(&jump_gate_1.symbol)),
            ships: ship_map.values().cloned().collect_vec(),
            materialized_supply_chain: None,
            agent_info: TestObjects::agent(),
        };

        let (fleets, fleet_tasks, fleet_phase) = compute_fleets_with_tasks(TestObjects::system_symbol(), &overview.completed_fleet_tasks, &facts);

        let fleets_map = fleets.iter().map(|f| (f.id.clone(), f.clone())).collect();

        assert_eq!(2, fleets.len());
        assert_eq!(2, fleet_tasks.len());
        assert_eq!(FleetPhaseName::InitialExploration, fleet_phase.name);

        let ship_fleet_assignment = FleetAdmiral::assign_ships(&fleet_tasks, &ship_map, &fleet_phase.shopping_list_in_order);

        let mut fleet_admiral = FleetAdmiral {
            completed_fleet_tasks: overview.completed_fleet_tasks.clone(),
            fleets: fleets_map,
            all_ships: ship_map,
            ship_tasks: Default::default(),
            fleet_tasks: fleet_tasks.into_iter().map(|(fleet_id, fleet_task)| (fleet_id, vec![fleet_task])).collect(),
            ship_fleet_assignment,
            agent_info: TestObjects::agent(),
            fleet_phase,
            active_trades: Default::default(),
            stationary_probe_locations: vec![],
        };

        let new_ship_tasks = FleetAdmiral::pure_compute_ship_tasks(
            &fleet_admiral,
            &facts,
            TestObjects::latest_market_data(&waypoints),
            TestObjects::ship_prices(&waypoints),
            waypoints,
        )
        .unwrap();

        FleetAdmiral::assign_ship_tasks_and_potential_requirements(&mut fleet_admiral, new_ship_tasks);
    }
}
