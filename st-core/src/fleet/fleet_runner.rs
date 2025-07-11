use crate::behavior_tree::behavior_args::BehaviorArgs;
use crate::behavior_tree::behavior_tree::ActionEvent;
use crate::behavior_tree::ship_behaviors::ShipAction;
use crate::fleet::fleet::{
    collect_fleet_decision_facts, compute_fleet_phase_with_tasks, compute_fleets_with_tasks, get_all_next_ship_purchases,
    recompute_tasks_after_ship_finishing_behavior_tree, FleetAdmiral, NewTaskResult, ShipStatusReport,
};
use crate::fleet::ship_runner::ship_behavior_runner;
use crate::ship::ShipOperations;
use crate::st_client::StClientTrait;
use anyhow::{anyhow, Result};
use chrono::Utc;

use crate::bmc_blackboard::BmcBlackboard;
use crate::transfer_cargo_manager::TransferCargoManager;
use itertools::Itertools;
use st_domain::blackboard_ops::BlackboardOps;
use st_domain::budgeting::treasury_redesign::ThreadSafeTreasurer;
use st_domain::{
    get_exploration_tasks_for_waypoint, FleetId, OperationExpenseEvent, Ship, ShipFrameSymbol, ShipSymbol, ShipTask, StationaryProbeLocation,
    TransactionActionEvent,
};
use st_store::bmc::Bmc;
use st_store::{upsert_fleets_data, Ctx};
use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::Not;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{event, span, Instrument};
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
    bmc: Arc<dyn Bmc>,
    pub treasurer: ThreadSafeTreasurer,
}

impl FleetRunner {
    pub async fn run_fleets(
        fleet_admiral: Arc<Mutex<FleetAdmiral>>,
        client: Arc<dyn StClientTrait>,
        bmc: Arc<dyn Bmc>,
        transfer_cargo_manager: Arc<TransferCargoManager>,
        sleep_duration: Duration,
        treasurer_archiver_join_handle: JoinHandle<()>,
    ) -> Result<()> {
        event!(Level::INFO, "Running fleets");

        let (ship_updated_tx, ship_updated_rx): (Sender<ShipOperations>, Receiver<ShipOperations>) = tokio::sync::mpsc::channel(32);
        let (ship_action_completed_tx, ship_action_completed_rx): (Sender<ActionEvent>, Receiver<ActionEvent>) = tokio::sync::mpsc::channel(32);
        let (ship_status_report_tx, ship_status_report_rx): (Sender<ShipStatusReport>, Receiver<ShipStatusReport>) = tokio::sync::mpsc::channel(32);

        let blackboard = BmcBlackboard::new(Arc::clone(&bmc));

        let blackboard: Arc<dyn BlackboardOps> = Arc::new(blackboard) as Arc<dyn BlackboardOps>;

        let thread_safe_treasurer = fleet_admiral.lock().await.treasurer.clone();
        let materialized_supply_chain_manager = fleet_admiral
            .lock()
            .await
            .materialized_supply_chain_manager
            .clone();

        let args: BehaviorArgs = BehaviorArgs {
            blackboard: Arc::clone(&blackboard),
            treasurer: thread_safe_treasurer.clone(),
            transfer_cargo_manager: Arc::clone(&transfer_cargo_manager),
            materialized_supply_chain_manager,
        };

        let ship_fibers: HashMap<ShipSymbol, JoinHandle<Result<()>>> = HashMap::new();

        let ship_ops: HashMap<ShipSymbol, Arc<Mutex<ShipOperations>>> = Default::default();
        let all_ships_map = fleet_admiral.lock().await.all_ships.clone();
        let ship_fleet_assignment = fleet_admiral.lock().await.ship_fleet_assignment.clone();

        {
            let mut admiral = fleet_admiral.lock().await;
            let system_symbol = all_ships_map
                .values()
                .next()
                .unwrap()
                .clone()
                .nav
                .system_symbol;

            let facts = collect_fleet_decision_facts(Arc::clone(&bmc), &system_symbol).await?;

            if let Some(msc) = &facts.materialized_supply_chain {
                args.materialized_supply_chain_manager
                    .register_materialized_supply_chain(system_symbol.clone(), msc.clone())?
            }

            let new_ship_tasks = FleetAdmiral::compute_ship_tasks(&mut admiral, &facts, Arc::clone(&bmc)).await?;

            FleetAdmiral::assign_ship_tasks(&mut admiral, new_ship_tasks);

            upsert_fleets_data(
                Arc::clone(&bmc),
                &Ctx::Anonymous,
                &admiral.fleets,
                &admiral.fleet_tasks,
                &admiral.ship_fleet_assignment,
                &admiral.ship_tasks,
            )
            .await?;
        }

        // Clone fleet_admiral infos to avoid the lifetime issues
        let all_ship_tasks = fleet_admiral.lock().await.ship_tasks.clone();

        if all_ship_tasks
            .contains_key(&ShipSymbol("FLWI_TEST-4".to_string()))
            .not()
        {
            println!("DEBUG")
        }
        let fleet_runner = Self {
            ship_fibers,
            ship_ops,
            ship_updated_tx: ship_updated_tx.clone(),
            ship_action_completed_tx: ship_action_completed_tx.clone(),
            ship_status_report_tx: ship_status_report_tx.clone(),
            client,
            args,
            fleet_admiral,
            bmc,
            treasurer: thread_safe_treasurer.clone(),
        };

        let fleet_runner_mutex = Arc::new(Mutex::new(fleet_runner));

        let msg_listeners_join_handle = tokio::spawn(Self::run_message_listeners(
            Arc::clone(&fleet_runner_mutex),
            ship_updated_rx,
            ship_action_completed_rx,
            ship_status_report_rx,
            sleep_duration,
        ));

        for (ss, ship) in all_ships_map {
            Self::launch_and_register_ship(
                Arc::clone(&fleet_runner_mutex),
                &ss,
                ship,
                sleep_duration,
                &all_ship_tasks,
                &ship_fleet_assignment,
            )
            .await?;
        }

        let _ = tokio::join!(msg_listeners_join_handle, treasurer_archiver_join_handle);

        Ok(())
    }

    pub async fn launch_and_register_ship(
        runner: Arc<Mutex<FleetRunner>>,
        ss: &ShipSymbol,
        ship: Ship,
        sleep_duration: Duration,
        all_ship_tasks: &HashMap<ShipSymbol, ShipTask>,
        ship_fleet_assignment: &HashMap<ShipSymbol, FleetId>,
    ) -> Result<()> {
        // if ss.0 != "FLWI-26" {
        //     return Ok(());
        // }
        let mut guard = runner.lock().await;
        let fleet_id = ship_fleet_assignment.get(ss).unwrap();

        println!("DEBUG: Creating new ship_op_mutex for ship: {}", ss.0);

        let ship_op_mutex = Arc::new(Mutex::new(ShipOperations::new(ship.clone(), Arc::clone(&guard.client), fleet_id.clone())));
        let maybe_ship_task = all_ship_tasks.get(ss);

        if let Some(ship_task) = maybe_ship_task {
            // Clone all the values that need to be moved into the async task
            let ship_op_clone = Arc::clone(&ship_op_mutex);
            let fleet_id_clone = fleet_id.clone();
            let args_clone = guard.args.clone();
            let ship_updated_tx_clone = guard.ship_updated_tx.clone();
            let ship_action_completed_tx_clone = guard.ship_action_completed_tx.clone();
            let ship_status_report_tx_clone = guard.ship_status_report_tx.clone();
            let ship_task_clone = ship_task.clone();
            let ship_symbol_clone = ss.clone();
            let ship_symbol_clone_2 = ship_symbol_clone.clone();

            let fiber = tokio::spawn(async move {
                match Self::behavior_runner(
                    ship_op_clone,
                    args_clone,
                    ship_updated_tx_clone,
                    ship_action_completed_tx_clone,
                    ship_task_clone,
                    sleep_duration,
                    fleet_id_clone,
                )
                .await
                {
                    Ok(maybe_task_finished_result) => {
                        if let Some((ship, ship_task)) = maybe_task_finished_result {
                            if ship.frame.symbol == ShipFrameSymbol::FRAME_PROBE {
                                event!(
                                    Level::ERROR,
                                    message = "A probe should never finish their behavior tree",
                                    ship = ship_symbol_clone_2.clone().0
                                );
                            }

                            ship_status_report_tx_clone
                                .send(ShipStatusReport::ShipFinishedBehaviorTree(ship, ship_task))
                                .await?;
                        }
                    }
                    Err(err) => {
                        event!(
                            Level::ERROR,
                            message = "Error in behavior_runner",
                            error = err.to_string(),
                            ship = ship_symbol_clone_2.clone().0
                        )
                    }
                }

                Ok(())
            });

            guard.ship_fibers.insert(ship_symbol_clone, fiber);
        }
        guard.ship_ops.insert(ss.clone(), ship_op_mutex);
        Ok(())
    }

    pub async fn launch_ship_fibers_of_idle_or_new_ships(
        runner: Arc<Mutex<FleetRunner>>,
        all_ships: HashSet<ShipSymbol>,
        ship_tasks: HashMap<ShipSymbol, ShipTask>,
        sleep_duration: Duration,
        ship_fleet_assignment: &HashMap<ShipSymbol, FleetId>,
    ) -> Result<()> {
        let not_running_ships = {
            let runner_guard = runner.lock().await;

            let completed_fibers = runner_guard
                .ship_fibers
                .iter()
                .filter_map(|(ss, ship_fiber)| ship_fiber.is_finished().then_some(ss))
                .cloned()
                .collect::<HashSet<_>>();

            let running_fibers = runner_guard
                .ship_fibers
                .iter()
                .filter_map(|(ss, ship_fiber)| ship_fiber.is_finished().not().then_some(ss))
                .cloned()
                .collect::<HashSet<_>>();

            let not_running_ships = all_ships
                .difference(&running_fibers)
                .cloned()
                .collect::<HashSet<_>>()
                .union(&completed_fibers)
                .cloned()
                .collect_vec();

            event!(
                Level::INFO,
                "{} out of {} ships have running fibers. (Re-)Starting fibers for {} ships ({})",
                running_fibers.len(),
                all_ships.len(),
                not_running_ships.len(),
                not_running_ships.iter().map(|ss| ss.0.clone()).join(", ")
            );
            not_running_ships
        };

        for ss in not_running_ships {
            let fleet_id = ship_fleet_assignment.get(&ss).unwrap();
            Self::relaunch_ship(Arc::clone(&runner), &ss, ship_tasks.clone(), sleep_duration, fleet_id.clone()).await?
        }

        Ok(())
    }

    //TODO - refactor to DRY up with fn launch_and_register_ship
    pub async fn relaunch_ship(
        runner: Arc<Mutex<FleetRunner>>,
        ss: &ShipSymbol,
        ship_tasks: HashMap<ShipSymbol, ShipTask>,
        sleep_duration: Duration,
        fleet_id: FleetId,
    ) -> Result<()> {
        let mut runner_guard = runner.lock().await;

        let ship_op_mutex = match runner_guard.ship_ops.get(ss) {
            None => {
                println!("DEBUG: Reusing existing ship_op_mutex for ship: {}", ss.0);
                event!(Level::INFO, "relaunch_ship called for {}, but it has no ship_ops entry. This is probably a probe that has been taken off the behavior-trees is just passively sitting at the observation waypoint.", ss.0.clone());
                return Ok(());
            }
            Some(ship_op) => ship_op,
        };
        let maybe_ship_task = ship_tasks.get(ss);

        if let Some(ship_task) = maybe_ship_task {
            // Clone all the values that need to be moved into the async task
            let ship_op_clone = Arc::clone(ship_op_mutex);
            let args_clone = runner_guard.args.clone();
            let ship_updated_tx_clone = runner_guard.ship_updated_tx.clone();
            let ship_action_completed_tx_clone = runner_guard.ship_action_completed_tx.clone();
            let ship_status_report_tx_clone = runner_guard.ship_status_report_tx.clone();
            let ship_task_clone = ship_task.clone();
            let ship_symbol_clone = ss.clone();
            let fleet_id_clone = fleet_id.clone();
            let ship_symbol_clone2 = ship_symbol_clone.clone();

            let fiber = tokio::spawn(async move {
                let ship_task_clone_2 = ship_task_clone.clone();
                match Self::behavior_runner(
                    ship_op_clone,
                    args_clone,
                    ship_updated_tx_clone,
                    ship_action_completed_tx_clone,
                    ship_task_clone,
                    sleep_duration,
                    fleet_id_clone,
                )
                .await
                {
                    Ok(maybe_task_finished_result) => {
                        if let Some((ship, ship_task)) = maybe_task_finished_result {
                            ship_status_report_tx_clone
                                .send(ShipStatusReport::ShipFinishedBehaviorTree(ship, ship_task))
                                .await?;
                        }
                    }
                    Err(err) => {
                        event!(
                            Level::ERROR,
                            message = "Error in behavior_runner",
                            error = err.to_string(),
                            ship = ship_symbol_clone2.0.clone(),
                            task = ship_task_clone_2.to_string()
                        )
                    }
                }
                Ok(())
            });

            runner_guard.ship_fibers.insert(ship_symbol_clone, fiber);
            event!(Level::DEBUG, message = "Ship fiber spawned")
        } else {
            event!(Level::ERROR, message = "Failed to spawn ship fiber - no task found")
        }
        Ok(())
    }

    pub async fn behavior_runner(
        ship_op: Arc<Mutex<ShipOperations>>,
        args: BehaviorArgs,
        ship_updated_tx: Sender<ShipOperations>,
        ship_action_completed_tx: Sender<ActionEvent>,
        ship_task: ShipTask,
        sleep_duration: Duration,
        fleet_id: FleetId,
    ) -> Result<Option<(Ship, ShipTask)>> {
        use crate::behavior_tree::behavior_tree::Response;
        use crate::behavior_tree::ship_behaviors::ship_behaviors;
        use anyhow::Error;

        use tracing::{span, Level};
        let behaviors = ship_behaviors();

        let mut ship = ship_op.lock().await;

        ship.my_fleet = fleet_id;
        let ship_updated_tx_clone = ship_updated_tx.clone();
        let ship_action_completed_tx_clone = ship_action_completed_tx.clone();

        let maybe_behavior = match ship_task.clone() {
            ShipTask::ObserveWaypointDetails { waypoint_symbol } => {
                ship.set_permanent_observation_location(waypoint_symbol);
                //println!("ship_loop: Ship {:?} is running stationary_probe_behavior", ship.symbol);
                Some((behaviors.stationary_probe_behavior, "stationary_probe_behavior"))
            }
            ShipTask::ObserveAllWaypointsOnce { waypoint_symbols } => {
                ship.set_explore_locations(waypoint_symbols);
                //println!("ship_loop: Ship {:?} is running explorer_behavior", ship.symbol);
                Some((behaviors.explorer_behavior, "explorer_behavior"))
            }
            ShipTask::Trade => {
                let tickets = args
                    .treasurer
                    .get_active_tickets_for_ship(&ship.symbol)
                    .await?;
                // println!("running trading behavior for ship, successfully started ticket execution");
                event!(
                    Level::INFO,
                    message = "Ship is executing trades",
                    ship = ship.symbol.0.clone(),
                    ids = tickets.iter().map(|t| t.ticket_id.to_string()).join(", "),
                    r#types = tickets
                        .iter()
                        .map(|t| t.details.get_description())
                        .join(", "),
                );
                //println!("ship_loop: Ship {:?} is running trading_behavior", ship.symbol);
                Some((behaviors.trading_behavior, "trading_behavior"))
            }
            ShipTask::PrepositionShipForTrade { first_purchase_location } => {
                ship.set_destination(first_purchase_location);
                Some((behaviors.navigate_to_destination, "navigate_to_destination"))
            }
            ShipTask::SiphonCarboHydratesAtWaypoint { siphoning_waypoint } => {
                ship.set_siphoning_waypoint(siphoning_waypoint);
                Some((behaviors.siphoning_behavior, "siphoning_behavior"))
            }
            ShipTask::SurveyMiningSite { mining_waypoint } => {
                ship.set_mining_waypoint(mining_waypoint.clone());
                Some((behaviors.surveyor_behavior, "surveyor_behavior"))
            }
            ShipTask::HaulMiningGoods { mining_waypoint } => {
                ship.set_mining_waypoint(mining_waypoint);
                Some((behaviors.mining_hauler_behavior, "mining_hauler_behavior"))
            }
            ShipTask::MineMaterialsAtWaypoint { mining_waypoint } => {
                ship.set_mining_waypoint(mining_waypoint);
                Some((behaviors.miner_behavior, "miner_behavior"))
            }
            ShipTask::ExecuteContracts => {
                if let Some(contract) = args
                    .blackboard
                    .get_youngest_contract(&ship.nav.system_symbol)
                    .await?
                {
                    ship.set_contract(contract);
                }
                Some((behaviors.contractor_behavior, "contractor_behavior"))
            }
        };

        event!(
            Level::DEBUG,
            message = "behavior_runner - successfully computed behavior for ship",
            ship = ship.symbol.0
        );

        match maybe_behavior {
            None => {
                event!(
                    Level::WARN,
                    message = "No behavior to run found for ship with task",
                    ship = ship.symbol.0.clone(),
                    task = ship_task.to_string()
                );

                Ok(None)
            }
            Some((ship_behavior, behavior_label)) => {
                event!(
                    Level::DEBUG,
                    message = "behavior runner started",
                    ship = format!("{}", ship.symbol.0),
                    behavior = behavior_label
                );
                let ship_span = span!(Level::INFO, "ship_behavior", ship = format!("{}", ship.symbol.0), behavior = behavior_label);

                let result: Result<Response, Error> = ship_behavior_runner(
                    &mut ship,
                    sleep_duration,
                    &args,
                    ship_behavior,
                    ship_updated_tx_clone,
                    ship_action_completed_tx_clone,
                )
                .instrument(ship_span)
                .await;

                let ship_span = span!(Level::DEBUG, "fleet_runner", ship = format!("{}", ship.symbol.0), behavior = behavior_label);
                let _enter = ship_span.enter();

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
                            Level::WARN,
                            message = "behavior_runner done with Error",
                            result = %e,
                        );
                        Err(anyhow!("behavior_runner done with Error: {}", e))
                    }
                }
            }
        }
    }

    pub async fn listen_to_ship_changes_and_persist(fleet_admiral: Arc<Mutex<FleetAdmiral>>, mut ship_updated_rx: Receiver<ShipOperations>) -> Result<()> {
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
                    admiral
                        .all_ships
                        .insert(updated_ship.symbol.clone(), updated_ship.ship);
                }
            }
        }

        Ok(())
    }

    pub async fn listen_to_ship_status_report_messages(
        fleet_admiral: Arc<Mutex<FleetAdmiral>>,
        bmc: Arc<dyn Bmc>,
        mut ship_status_report_rx: Receiver<ShipStatusReport>,
        runner: Arc<Mutex<FleetRunner>>,
        sleep_duration: Duration,
    ) -> Result<()> {
        while let Some(msg) = ship_status_report_rx.recv().await {
            let messages_in_queue = ship_status_report_rx.len();

            let ship_span = span!(
                Level::INFO,
                "fleet_runner::listen_to_ship_status_report_messages",
                ship = format!("{}", msg.ship_symbol().0),
            );

            // Process the message with error handling that doesn't return from the function
            if let Err(e) = Self::process_ship_status_report(
                &msg,
                Arc::clone(&fleet_admiral),
                Arc::clone(&bmc),
                Arc::clone(&runner),
                sleep_duration,
                messages_in_queue,
            )
            .instrument(ship_span)
            .await
            {
                // Log the error but continue the loop
                let maybe_fleet = {
                    let guard = fleet_admiral.lock().await;
                    guard
                        .ship_fleet_assignment
                        .get(&msg.ship_symbol())
                        .cloned()
                        .and_then(|id| guard.fleets.get(&id))
                        .cloned()
                };
                event!(
                    Level::ERROR,
                    message = format!("Error processing ship status report: {}", e),
                    fleet = maybe_fleet.clone().map(|f| f.id.0),
                    fleet_cfg = maybe_fleet.map(|f| f.cfg.to_string()),
                );
                // Optionally add a small delay to prevent CPU spinning on persistent errors
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        }

        // This should only be reached if the channel is closed
        event!(Level::WARN, "Ship status report channel closed, exiting listener");
        Ok(())
    }

    pub async fn process_ship_status_report(
        msg: &ShipStatusReport,
        fleet_admiral: Arc<Mutex<FleetAdmiral>>,
        bmc: Arc<dyn Bmc>,
        runner: Arc<Mutex<FleetRunner>>,
        sleep_duration: Duration,
        messages_in_queue: usize,
    ) -> Result<()> {
        let mut admiral_guard = fleet_admiral.lock().await;
        admiral_guard
            .report_ship_action_completed(msg, Arc::clone(&bmc), messages_in_queue)
            .await?;

        let treasurer_credits = admiral_guard.agent_info_credits().await.0;

        match msg {
            ShipStatusReport::ShipFinishedBehaviorTree(ship, task) => {
                admiral_guard.ship_tasks.remove(&ship.symbol);
                let result = recompute_tasks_after_ship_finishing_behavior_tree(&mut admiral_guard, ship, task, Arc::clone(&bmc)).await?;
                event!(
                    Level::INFO,
                    message = "ShipFinishedBehaviorTree",
                    ship = ship.symbol.0,
                    completed_task = task.to_string(),
                    recompute_result = result.to_string()
                );
                bmc.ship_bmc()
                    .save_ship_tasks(&Ctx::Anonymous, &admiral_guard.ship_tasks)
                    .await?;
                match result {
                    NewTaskResult::DismantleFleets { fleets_to_dismantle } => {
                        event!(
                            Level::INFO,
                            "Dismantling fleets {}",
                            fleets_to_dismantle
                                .iter()
                                .map(|fleet_id| format!("#{}", fleet_id.0))
                                .join(", ")
                        );

                        let system_symbol = ship.nav.system_symbol.clone();

                        FleetAdmiral::dismantle_fleets(&mut admiral_guard, fleets_to_dismantle.clone()).await?;

                        bmc.fleet_bmc()
                            .delete_fleets(&Ctx::Anonymous, &fleets_to_dismantle)
                            .await?;

                        let facts = collect_fleet_decision_facts(Arc::clone(&bmc), &system_symbol).await?;
                        admiral_guard.update_materialized_supply_chain(&facts.materialized_supply_chain)?;

                        let fleet_phase = compute_fleet_phase_with_tasks(system_symbol.clone(), &facts, &admiral_guard.completed_fleet_tasks);
                        let (fleets, fleet_tasks) = compute_fleets_with_tasks(&facts, &admiral_guard.fleets, &admiral_guard.fleet_tasks, &fleet_phase);
                        // println!("Computed new fleets after dismantling the fleets: {:?}", fleets_to_dismantle);
                        // dbg!(&fleets);
                        // dbg!(&fleet_tasks);
                        // dbg!(&fleet_phase);
                        let current_ship_demands = get_all_next_ship_purchases(&admiral_guard.all_ships, &fleet_phase);
                        admiral_guard.ship_purchase_demand = VecDeque::from(current_ship_demands);

                        admiral_guard.fleets = fleets.into_iter().map(|f| (f.id.clone(), f)).collect();
                        admiral_guard.fleet_tasks = fleet_tasks
                            .into_iter()
                            .map(|(fleet_id, task)| (fleet_id, vec![task]))
                            .collect();
                        admiral_guard.fleet_phase = fleet_phase;

                        let ship_price_info = bmc
                            .shipyard_bmc()
                            .get_latest_ship_prices(&Ctx::Anonymous, &system_symbol)
                            .await?;

                        //FIXME: assuming one fleet task per fleet
                        let fleet_task_list = admiral_guard
                            .fleet_tasks
                            .iter()
                            .map(|(fleet_id, tasks)| (fleet_id.clone(), tasks.first().cloned().unwrap()))
                            .collect_vec();

                        let ship_fleet_assignment =
                            FleetAdmiral::assign_ships(&fleet_task_list, &admiral_guard.all_ships, &admiral_guard.fleet_phase.shopping_list_in_order);
                        admiral_guard.ship_fleet_assignment = ship_fleet_assignment.clone();

                        admiral_guard
                            .redistribute_distribute_fleet_budgets(&ship_price_info, &system_symbol)
                            .await?;

                        let new_ship_tasks = FleetAdmiral::compute_ship_tasks(&mut admiral_guard, &facts, Arc::clone(&bmc)).await?;
                        FleetAdmiral::assign_ship_tasks(&mut admiral_guard, new_ship_tasks);

                        Self::launch_ship_fibers_of_idle_or_new_ships(
                            Arc::clone(&runner),
                            admiral_guard
                                .all_ships
                                .keys()
                                .cloned()
                                .collect::<HashSet<_>>(),
                            admiral_guard.ship_tasks.clone(),
                            sleep_duration,
                            &ship_fleet_assignment,
                        )
                        .await?;

                        upsert_fleets_data(
                            Arc::clone(&bmc),
                            &Ctx::Anonymous,
                            &admiral_guard.fleets,
                            &admiral_guard.fleet_tasks,
                            &admiral_guard.ship_fleet_assignment,
                            &admiral_guard.ship_tasks,
                        )
                        .await?;
                    }
                    NewTaskResult::AssignNewTaskToShip { ship_symbol, task } => {
                        FleetAdmiral::assign_ship_tasks(&mut admiral_guard, vec![(ship_symbol.clone(), task)]);
                        assert!(
                            admiral_guard.get_task_of_ship(&ship_symbol).is_some(),
                            "After AssignNewTaskToShip, the ship is supposed to have a new task, but it was None"
                        );
                        let fleet_id = admiral_guard
                            .ship_fleet_assignment
                            .get(&ship_symbol)
                            .unwrap();

                        upsert_fleets_data(
                            Arc::clone(&bmc),
                            &Ctx::Anonymous,
                            &admiral_guard.fleets,
                            &admiral_guard.fleet_tasks,
                            &admiral_guard.ship_fleet_assignment,
                            &admiral_guard.ship_tasks,
                        )
                        .await?;
                        Self::relaunch_ship(runner.clone(), &ship_symbol, admiral_guard.ship_tasks.clone(), sleep_duration, fleet_id.clone()).await?;
                        event!(Level::DEBUG, message = "Ship relaunched successfully")
                    }
                }
            }

            ShipStatusReport::ShipActionCompleted(ship, ship_action) => {
                if ship_action == &ShipAction::RegisterProbeForPermanentObservation {
                    let current_waypoint_symbol = ship.nav.waypoint_symbol.clone();
                    let exploration_tasks = bmc
                        .system_bmc()
                        .get_waypoints_of_system(&Ctx::Anonymous, &ship.nav.system_symbol)
                        .await?
                        .into_iter()
                        .find(|wp| wp.symbol == current_waypoint_symbol)
                        .map(|wp| get_exploration_tasks_for_waypoint(&wp))
                        .unwrap_or_default();

                    let stationary_probe_location = StationaryProbeLocation {
                        waypoint_symbol: ship.nav.waypoint_symbol.clone(),
                        probe_ship_symbol: ship.symbol.clone(),
                        exploration_tasks,
                    };

                    // TODO: might be redundant - don't store if already stored

                    FleetAdmiral::add_stationary_probe_location(&mut admiral_guard, stationary_probe_location.clone());

                    bmc.ship_bmc()
                        .insert_stationary_probe(&Ctx::Anonymous, stationary_probe_location)
                        .await?;
                } else if ship_action == &ShipAction::CollectWaypointInfos {
                    let facts = collect_fleet_decision_facts(Arc::clone(&bmc), &ship.nav.system_symbol).await?;
                    admiral_guard.update_materialized_supply_chain(&facts.materialized_supply_chain)?;
                }
            }
            ShipStatusReport::Expense(_, operation_expense) => match operation_expense {
                OperationExpenseEvent::RefueledShip { response } => {
                    event!(
                        Level::DEBUG,
                        message = "ShipStatusReport",
                        report_type = "OperationExpenseEvent::RefueledShip",
                        units = response.data.transaction.units,
                        price_per_unit = response.data.transaction.price_per_unit,
                        total_price = response.data.transaction.total_price,
                        waypoint_symbol = response.data.transaction.waypoint_symbol.0,
                        agent_credits = response.data.agent.credits,
                        treasurer_credits
                    );
                }
            },
            ShipStatusReport::TransactionCompleted(_, transaction_event, _) => match &transaction_event {
                TransactionActionEvent::PurchasedTradeGoods { response, .. } => {
                    event!(
                        Level::INFO,
                        message = "ShipStatusReport",
                        report_type = "TransactionActionEvent::PurchasedTradeGoods",
                        trade_symbol = response.data.transaction.trade_symbol.to_string(),
                        units = response.data.transaction.units,
                        price_per_unit = response.data.transaction.price_per_unit,
                        total_price = response.data.transaction.total_price,
                        waypoint_symbol = response.data.transaction.waypoint_symbol.0,
                        agent_credits = response.data.agent.credits,
                        treasurer_credits
                    );
                }
                TransactionActionEvent::SoldTradeGoods { response, .. } => {
                    event!(
                        Level::INFO,
                        message = "ShipStatusReport",
                        report_type = "TransactionActionEvent::SoldTradeGoods",
                        trade_symbol = response.data.transaction.trade_symbol.to_string(),
                        units = response.data.transaction.units,
                        price_per_unit = response.data.transaction.price_per_unit,
                        total_price = response.data.transaction.total_price,
                        waypoint_symbol = response.data.transaction.waypoint_symbol.0,
                        agent_credits = response.data.agent.credits,
                        treasurer_credits
                    );
                }
                TransactionActionEvent::SuppliedConstructionSite { ticket_details, response, .. } => {
                    let overview_str = response
                        .data
                        .construction
                        .materials
                        .iter()
                        .map(|cm| format!("{}: {} of {}", cm.trade_symbol, cm.fulfilled, cm.required))
                        .join(", ");
                    event!(
                        Level::INFO,
                        message = "ShipStatusReport",
                        report_type = "TransactionActionEvent::SuppliedConstructionSite",
                        trade_symbol = ticket_details.trade_good.to_string(),
                        units = ticket_details.quantity,
                        waypoint_symbol = ticket_details.waypoint_symbol.to_string(),
                        material_overview = overview_str,
                        treasurer_credits
                    );
                }
                TransactionActionEvent::PurchasedShip { ticket_details, response, .. } => {
                    event!(
                        Level::INFO,
                        message = "ShipStatusReport",
                        report_type = "TransactionActionEvent::ShipPurchased",
                        new_ship = response.data.ship.symbol.0,
                        new_ship_type = response.data.ship.frame.symbol.to_string(),
                        assigned_fleet_id = ticket_details.assigned_fleet_id.0,
                        agent_credits = response.data.agent.credits,
                        treasurer_credits
                    );

                    let new_ship = response.data.ship.clone();
                    bmc.ship_bmc()
                        .upsert_ships(&Ctx::Anonymous, &vec![new_ship.clone()], Utc::now())
                        .await?;

                    admiral_guard
                        .all_ships
                        .insert(new_ship.symbol.clone(), new_ship.clone());

                    admiral_guard
                        .ship_fleet_assignment
                        .insert(new_ship.symbol.clone(), ticket_details.assigned_fleet_id.clone());

                    FleetAdmiral::adjust_fleet_budget_after_ship_purchase(&admiral_guard, &new_ship, &ticket_details.assigned_fleet_id).await?;

                    let facts = collect_fleet_decision_facts(Arc::clone(&bmc), &new_ship.nav.system_symbol).await?;
                    let new_ship_tasks = FleetAdmiral::compute_ship_tasks(&mut admiral_guard, &facts, Arc::clone(&bmc)).await?;
                    FleetAdmiral::assign_ship_tasks(&mut admiral_guard, new_ship_tasks);
                    upsert_fleets_data(
                        Arc::clone(&bmc),
                        &Ctx::Anonymous,
                        &admiral_guard.fleets,
                        &admiral_guard.fleet_tasks,
                        &admiral_guard.ship_fleet_assignment,
                        &admiral_guard.ship_tasks,
                    )
                    .await?;

                    if let Some(fleet_of_new_ship) = admiral_guard.get_fleet_of_ship(&new_ship.symbol) {
                        if fleet_of_new_ship.id != ticket_details.assigned_fleet_id {
                            eprintln!("newly purchased ship got assigned to the wrong fleet");
                        }
                    }

                    Self::launch_and_register_ship(
                        Arc::clone(&runner),
                        &new_ship.symbol,
                        new_ship.clone(),
                        sleep_duration,
                        &admiral_guard.ship_tasks,
                        &admiral_guard.ship_fleet_assignment,
                    )
                    .await?
                }
            },
        }
        Ok(())
    }

    pub async fn listen_to_ship_action_update_messages(
        ship_status_report_tx: Sender<ShipStatusReport>,
        mut ship_action_completed_rx: Receiver<ActionEvent>,
    ) -> Result<()> {
        while let Some(msg) = ship_action_completed_rx.recv().await {
            let ship_span = span!(
                Level::DEBUG,
                "fleet_runner::listen_to_ship_action_update_messages",
                ship = format!("{}", msg.ship_symbol().0)
            );
            let _enter = ship_span.enter();

            match msg {
                ActionEvent::ShipActionCompleted(ship_op, ship_action, result) => {
                    if &ship_op.symbol.0 == "FLWI_TEST-4" {
                        event!(
                            Level::INFO,
                            message = "ShipActionCompleted - CHECKING FOR FLWI-TEST-4",
                            ship = ship_op.symbol.0,
                            action = %ship_action,
                        );
                    }

                    match result {
                        Ok(_) => {
                            event!(
                                Level::DEBUG,
                                message = "ShipActionCompleted",
                                ship = ship_op.symbol.0,
                                action = %ship_action,
                            );
                            if ship_action == ShipAction::CollectWaypointInfos || ship_action == ShipAction::RegisterProbeForPermanentObservation {
                                ship_status_report_tx
                                    .send(ShipStatusReport::ShipActionCompleted(ship_op.ship.clone(), ship_action))
                                    .await?;
                            }
                        }
                        Err(err) => {
                            event!(Level::ERROR, message = "Error completing ShipAction", error = %err,
                                ship = ship_op.symbol.0,
                                action = %ship_action,
                            );
                        }
                    }
                }
                ActionEvent::BehaviorCompleted(ship_ops, ship_action, result) => match result {
                    Ok(_) => {
                        event!(
                            Level::DEBUG,
                            message = "BehaviorCompleted",
                            ship = ship_ops.symbol.0,
                            action = %ship_action,
                            index = ship_action.index(),
                        );
                    }
                    Err(error) => {
                        event!(
                            Level::ERROR,
                            message = "BehaviorCompleted with error",
                            ship = ship_ops.symbol.0,
                            action = %ship_action,
                            index = ship_action.index(),
                            error,
                            debug_state = ship_ops.to_debug_string(),
                        );
                    }
                },
                ActionEvent::TransactionCompleted(ship, transaction, ticket) => {
                    event!(
                        Level::INFO,
                        message = "TransactionCompleted",
                        ship = ship.symbol.0,
                        transaction = %transaction.to_string(),
                        description = ticket.details.get_description(),

                    );
                    ship_status_report_tx
                        .send(ShipStatusReport::TransactionCompleted(ship.ship, transaction, ticket))
                        .await?;
                }

                ActionEvent::Expense(ship, operation_expense) => {
                    ship_status_report_tx
                        .send(ShipStatusReport::Expense(ship.ship, operation_expense))
                        .await?;
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
        sleep_duration: Duration,
    ) {
        // Extract all needed data with a single lock acquisition
        let (bmc, fleet_admiral, ship_status_report_tx) = {
            let guard = runner.lock().await;
            (Arc::clone(&guard.bmc), Arc::clone(&guard.fleet_admiral), guard.ship_status_report_tx.clone())
        };

        // Create a cancellation token for coordinated shutdown
        let cancel_token = CancellationToken::new();

        // Clone tokens and resources for each task
        let ship_updated_token = cancel_token.clone();
        let ship_action_token = cancel_token.clone();
        let ship_status_token = cancel_token.clone();

        let bmc_for_status = Arc::clone(&bmc);
        let fleet_admiral_for_updated = Arc::clone(&fleet_admiral);
        let fleet_admiral_for_status = Arc::clone(&fleet_admiral);
        let runner_for_status = Arc::clone(&runner);

        let fleet_admiral_for_restart_ships = Arc::clone(&fleet_admiral);
        let runner_for_restart_ships = Arc::clone(&runner);

        // Spawn tasks with error handling
        let ship_updated_listener_join_handle = tokio::spawn(async move {
            let result = tokio::select! {
                r = Self::listen_to_ship_changes_and_persist(
                    fleet_admiral_for_updated,
                    ship_updated_rx,
                ) => r,
                _ = ship_updated_token.cancelled() => {
                    event!(Level::INFO, "Ship updated listener cancelled");
                    Ok(())
                }
            };

            if let Err(e) = &result {
                event!(Level::ERROR, "Ship updated listener failed: {}", e);
                // Cancel other tasks when one fails
                ship_updated_token.cancel();
            }
            result
        });

        let ship_action_update_listener_join_handle = tokio::spawn(async move {
            let result = tokio::select! {
                r = Self::listen_to_ship_action_update_messages(
                    ship_status_report_tx,
                    ship_action_completed_rx
                ) => r,
                _ = ship_action_token.cancelled() => {
                    event!(Level::INFO, "Ship action update listener cancelled");
                    Ok(())
                }
            };

            if let Err(e) = &result {
                event!(Level::ERROR, "Ship action update listener failed: {}", e);
                ship_action_token.cancel();
            }
            result
        });

        // These listener-functions must be very robust.
        // If we throw an error in here (e.g. early return by `?` on a Result)
        // the passed in receivers (e.g. Receiver<ShipStatusReport>) get dropped and then everything grinds to a halt due to channel closed

        let ship_status_report_listener_join_handle = tokio::spawn(async move {
            let result = tokio::select! {
                r = Self::listen_to_ship_status_report_messages(
                    fleet_admiral_for_status,
                    bmc_for_status,
                    ship_status_report_rx,
                    runner_for_status,
                    sleep_duration,
                ) => r,
                _ = ship_status_token.cancelled() => {
                    event!(Level::INFO, "Ship status report listener cancelled");
                    Ok(())
                }
            };

            if let Err(e) = &result {
                event!(Level::ERROR, "Ship status report listener failed: {}", e);
                ship_status_token.cancel();
            }
            result
        });

        let restart_idle_ships_join_handle = tokio::spawn(async move {
            let res = loop {
                let admiral = fleet_admiral_for_restart_ships.clone();
                let runner = runner_for_restart_ships.clone();
                let (all_ships, ship_tasks, sleep_duration, ship_fleet_assignment) = {
                    let admiral_guard = admiral.lock().await;
                    let all_ships = admiral_guard.all_ships.keys().cloned().collect();
                    let ship_tasks = admiral_guard.ship_tasks.clone();
                    let sleep_duration = sleep_duration;
                    let ship_fleet_assignment = admiral_guard.ship_fleet_assignment.clone();
                    (all_ships, ship_tasks, sleep_duration, ship_fleet_assignment)
                };
                let res = Self::launch_ship_fibers_of_idle_or_new_ships(runner, all_ships, ship_tasks, sleep_duration, &ship_fleet_assignment).await;
                if let Err(e) = res {
                    break e;
                }
                tokio::time::sleep(Duration::from_secs(30)).await;
            };
            res
        });

        // Wait for all tasks and handle errors
        let (updated_result, action_result, status_result, restart_idle_ships_result) = tokio::join!(
            ship_updated_listener_join_handle,
            ship_action_update_listener_join_handle,
            ship_status_report_listener_join_handle,
            restart_idle_ships_join_handle
        );

        // Log any join errors
        if let Err(e) = updated_result {
            event!(Level::ERROR, "Ship updated listener join error: {}", e);
        }
        if let Err(e) = action_result {
            event!(Level::ERROR, "Ship action update listener join error: {}", e);
        }
        if let Err(e) = status_result {
            event!(Level::ERROR, "Ship status report listener join error: {}", e);
        }
        if let Err(e) = restart_idle_ships_result {
            event!(Level::ERROR, "restart_idle_ships_result join error: {}", e);
        }

        event!(Level::WARN, "All listeners have exited, fleet runner will no longer process messages");
    }
}

#[cfg(test)]
mod tests {
    use crate::bmc_blackboard::BmcBlackboard;
    use crate::fleet::fleet::FleetAdmiral;
    use crate::fleet::fleet_runner::FleetRunner;
    use crate::fleet::initial_data_collector::load_and_store_initial_data_in_bmcs;
    use crate::format_and_sort_collection;
    use crate::st_client::StClientTrait;
    use crate::transfer_cargo_manager::TransferCargoManager;
    use crate::universe_server::universe_server::{InMemoryUniverse, InMemoryUniverseClient};
    use chrono::Utc;
    use itertools::Itertools;
    use metrics::IntoF64;
    use st_domain::{
        FleetConfig, FleetId, FleetPhaseName, FleetTask, ShipFrameSymbol, ShipRegistrationRole, ShipSymbol, ShipTask, TradeGoodSymbol, WaypointSymbol,
    };
    use st_store::bmc::contract_bmc::InMemoryContractBmc;
    use st_store::bmc::jump_gate_bmc::InMemoryJumpGateBmc;
    use st_store::bmc::ship_bmc::{InMemoryShips, InMemoryShipsBmc};
    use st_store::bmc::{Bmc, InMemoryBmc};
    use st_store::ledger_bmc::InMemoryLedgerBmc;
    use st_store::shipyard_bmc::InMemoryShipyardBmc;
    use st_store::survey_bmc::InMemorySurveyBmc;
    use st_store::trade_bmc::InMemoryTradeBmc;
    use st_store::{
        Ctx, FleetBmcTrait, InMemoryAgentBmc, InMemoryConstructionBmc, InMemoryFleetBmc, InMemoryMarketBmc, InMemoryStatusBmc, InMemorySupplyChainBmc,
        InMemorySystemsBmc,
    };
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;
    use std::time::Duration;
    use test_log::test;
    use tokio::sync::Mutex;

    #[test(tokio::test)]
    //#[tokio::test] // for accessing runtime-infos with tokio-console
    async fn create_fleet_admiral_from_startup_ship_config() {
        // uncomment for displaying tasks with `tokio-console` in terminal
        // also don't use test-tracing-subscriber `#[test(tokio::test)]` but rather #[tokio::test]
        // console_subscriber::init();

        let manifest_dir = env!("CARGO_MANIFEST_DIR");

        let json_path = std::path::Path::new(manifest_dir)
            .parent()
            .unwrap()
            .join("resources")
            .join("universe_snapshot.json");

        let in_memory_universe = InMemoryUniverse::from_snapshot(json_path).expect("InMemoryUniverse::from_snapshot");

        let shipyard_waypoints = in_memory_universe
            .shipyards
            .keys()
            .cloned()
            .collect::<HashSet<_>>();

        let marketplace_waypoints = in_memory_universe
            .marketplaces
            .keys()
            .cloned()
            .collect::<HashSet<_>>();

        let in_memory_client = InMemoryUniverseClient::new(in_memory_universe);

        let agent = in_memory_client.get_agent().await.expect("agent").data;
        let hq_system_symbol = agent.headquarters.system_symbol();

        let ship_bmc = InMemoryShipsBmc::new(InMemoryShips::new());
        let agent_bmc = InMemoryAgentBmc::new(agent);
        let trade_bmc = InMemoryTradeBmc::new();
        let fleet_bmc = InMemoryFleetBmc::new();
        let system_bmc = InMemorySystemsBmc::new();
        let construction_bmc = InMemoryConstructionBmc::new();
        let survey_bmc = InMemorySurveyBmc::new();

        //insert some data
        //construction_bmc.save_construction_site(&Ctx::Anonymous, in_memory_client.get_construction_site().unwrap())

        let market_bmc = InMemoryMarketBmc::new();
        let shipyard_bmc = InMemoryShipyardBmc::new();
        let jump_gate_bmc = InMemoryJumpGateBmc::new();
        let supply_chain_bmc = InMemorySupplyChainBmc::new();
        let status_bmc = InMemoryStatusBmc::new();
        let ledger_bmc = InMemoryLedgerBmc::new();
        let contract_bmc = InMemoryContractBmc::new();

        let trade_bmc = Arc::new(trade_bmc);
        let market_bmc = Arc::new(market_bmc);
        let bmc = InMemoryBmc {
            in_mem_ship_bmc: Arc::new(ship_bmc),
            in_mem_fleet_bmc: Arc::new(fleet_bmc),
            in_mem_trade_bmc: Arc::clone(&trade_bmc),
            in_mem_system_bmc: Arc::new(system_bmc),
            in_mem_agent_bmc: Arc::new(agent_bmc),
            in_mem_construction_bmc: Arc::new(construction_bmc),
            in_mem_survey_bmc: Arc::new(survey_bmc),
            in_mem_market_bmc: Arc::clone(&market_bmc),
            in_mem_jump_gate_bmc: Arc::new(jump_gate_bmc),
            in_mem_shipyard_bmc: Arc::new(shipyard_bmc),
            in_mem_supply_chain_bmc: Arc::new(supply_chain_bmc),
            in_mem_status_bmc: Arc::new(status_bmc),
            in_mem_ledger_bmc: Arc::new(ledger_bmc),
            in_mem_contract_bmc: Arc::new(contract_bmc),
        };

        let client = Arc::new(in_memory_client) as Arc<dyn StClientTrait>;
        let bmc = Arc::new(bmc) as Arc<dyn Bmc>;
        let blackboard = BmcBlackboard::new(Arc::clone(&bmc));
        let transfer_cargo_manager = Arc::new(TransferCargoManager::new());

        load_and_store_initial_data_in_bmcs(Arc::clone(&client), Arc::clone(&bmc))
            .await
            .expect("FleetRunner::load_and_store_initial_data");

        println!("Creating fleet admiral");

        let (fleet_admiral, treasurer_archiver_join_handle) = FleetAdmiral::load_or_create(Arc::clone(&bmc), hq_system_symbol, Arc::clone(&client))
            .await
            .expect("FleetAdmiral::load_or_create");

        assert!(matches!(
            fleet_admiral
                .fleet_tasks
                .get(&FleetId(0))
                .cloned()
                .unwrap_or_default()
                .first(),
            Some(FleetTask::InitialExploration { .. })
        ));
        assert!(matches!(
            fleet_admiral
                .fleet_tasks
                .get(&FleetId(1))
                .cloned()
                .unwrap_or_default()
                .first(),
            Some(FleetTask::ObserveAllWaypointsOfSystemWithStationaryProbes { .. })
        ));

        let actual_agent_credits = fleet_admiral.agent_info_credits().await;
        let expected_agent_credits = client.get_agent().await.expect("agent").data.credits;
        assert_eq!(expected_agent_credits, actual_agent_credits.0);
        let admiral_mutex = Arc::new(Mutex::new(fleet_admiral));
        let admiral_clone = Arc::clone(&admiral_mutex);

        // This task runs your fleets
        let fleet_future = async {
            println!("Running fleets");
            FleetRunner::run_fleets(
                Arc::clone(&admiral_mutex),
                Arc::clone(&client),
                Arc::clone(&bmc),
                Arc::clone(&transfer_cargo_manager),
                Duration::from_millis(1),
                treasurer_archiver_join_handle,
            )
            .await
            .unwrap();
        };

        // This task periodically checks if the condition is met
        let condition_checker = async {
            let check_interval = Duration::from_millis(1000); // Adjust as needed
            let startup_time = Utc::now();

            let completion_duration = loop {
                // Sleep first to give the fleet a chance to start
                tokio::time::sleep(check_interval).await;

                let condition_met = {
                    let admiral = admiral_clone.lock().await;
                    let has_finished_initial_observation = admiral
                        .completed_fleet_tasks
                        .iter()
                        .any(|t| matches!(t.task, FleetTask::InitialExploration { .. }));

                    let fleet_budgets = admiral.treasurer.get_fleet_budgets().await.unwrap();
                    let has_all_fleets_registered_in_treasurer = admiral
                        .fleets
                        .keys()
                        .all(|id| fleet_budgets.contains_key(id));

                    let ship_type_overview: HashMap<(ShipRegistrationRole, ShipFrameSymbol), usize> = admiral
                        .all_ships
                        .values()
                        .map(|ship| (ship.registration.role.clone(), ship.frame.symbol.clone()))
                        .counts();

                    let is_in_construction_phase = admiral.fleet_phase.name == FleetPhaseName::ConstructJumpGate;
                    let num_ships = admiral.all_ships.len();
                    let has_bought_ships = num_ships > 2;
                    let num_stationary_probes = admiral.stationary_probe_locations.len();
                    let stationary_probe_locations: HashSet<WaypointSymbol> = admiral
                        .stationary_probe_locations
                        .iter()
                        .map(|spl| spl.waypoint_symbol.clone())
                        .collect::<HashSet<_>>();

                    let has_probes_at_every_shipyard = shipyard_waypoints
                        .difference(&stationary_probe_locations)
                        .count()
                        == 0;
                    let has_probes_at_every_marketplace = marketplace_waypoints
                        .difference(&stationary_probe_locations)
                        .count()
                        == 0;

                    let probe_task_waypoints = admiral
                        .ship_tasks
                        .iter()
                        .filter_map(|(ss, t)| match t {
                            ShipTask::ObserveWaypointDetails { waypoint_symbol } => Some(waypoint_symbol.clone()),
                            _ => None,
                        })
                        .collect::<HashSet<_>>();

                    let has_difference_in_probe_tasks_and_stationary_probe_locations = stationary_probe_locations != probe_task_waypoints;

                    let num_haulers = admiral
                        .all_ships
                        .iter()
                        .filter(|(_, s)| s.frame.symbol == ShipFrameSymbol::FRAME_LIGHT_FREIGHTER)
                        .count();

                    let num_mining_drones = admiral
                        .all_ships
                        .iter()
                        .filter(|(_, s)| s.frame.symbol == ShipFrameSymbol::FRAME_LIGHT_FREIGHTER)
                        .count();

                    let has_bought_all_ships = admiral.ship_purchase_demand.is_empty();

                    let home_system = bmc
                        .agent_bmc()
                        .load_agent(&Ctx::Anonymous)
                        .await
                        .expect("agent")
                        .headquarters
                        .system_symbol();

                    let maybe_construction_site = bmc
                        .construction_bmc()
                        .get_construction_site_for_system(&Ctx::Anonymous, home_system)
                        .await
                        .expect("construction_site");

                    let has_completed_construction = maybe_construction_site
                        .clone()
                        .map(|cs| cs.is_complete)
                        .unwrap_or(false);

                    if has_difference_in_probe_tasks_and_stationary_probe_locations {
                        println!("Hello, breakpoint");
                    }

                    let materials_summary = maybe_construction_site
                        .clone()
                        .map(|cs| {
                            cs.materials
                                .iter()
                                .map(|cm| {
                                    format!(
                                        "  {}: {}/{} ({:1}%)",
                                        cm.trade_symbol,
                                        cm.fulfilled,
                                        cm.required,
                                        (cm.fulfilled.into_f64() / cm.required.into_f64()) * 100.0
                                    )
                                })
                                .join("\n")
                        })
                        .unwrap_or_default();

                    let construction_summary = format!("Material Summary:\n{}", materials_summary);
                    let ship_type_summary = format!(
                        "Ship Types:\n{}",
                        ship_type_overview
                            .into_iter()
                            .map(|((role, frame_symbol), quantity)| format!("{frame_symbol} ({role}): {quantity}x"))
                            .join("\n")
                    );

                    let has_started_construction = maybe_construction_site
                        .map(|cs| {
                            cs.materials
                                .iter()
                                .any(|cm| &cm.trade_symbol != &TradeGoodSymbol::QUANTUM_STABILIZERS && cm.fulfilled > 0)
                        })
                        .unwrap_or(false);

                    let evaluation_result = has_finished_initial_observation
                        && has_all_fleets_registered_in_treasurer
                        && is_in_construction_phase
                        && has_bought_ships
                        && has_probes_at_every_shipyard
                        && has_probes_at_every_marketplace
                        && has_bought_all_ships
                        && has_started_construction
                        && has_completed_construction;

                    println!(
                        r#"
has_finished_initial_observation: {has_finished_initial_observation}
has_all_fleets_registered_in_treasurer: {has_all_fleets_registered_in_treasurer}
is_in_construction_phase: {is_in_construction_phase}
num_ships: {num_ships}
num_stationary_probes: {num_stationary_probes}
num_haulers: {num_haulers}
num_mining_drones: {num_mining_drones}
stationary_probe_locations: {}
shipyard_waypoints: {}
has_probes_at_every_shipyard: {has_probes_at_every_shipyard}
marketplace_waypoints: {}
has_probes_at_every_marketplace: {has_probes_at_every_marketplace}
num_stationary_probe_locations: {}
num_probe_task_waypoints: {}
has_difference_in_probe_tasks_and_stationary_probe_locations: {has_difference_in_probe_tasks_and_stationary_probe_locations}
has_bought_all_ships: {has_bought_all_ships}
has_started_construction: {has_started_construction}
has_completed_construction: {has_completed_construction}
{construction_summary}

{ship_type_summary}

evaluation_result: {evaluation_result}
"#,
                        format_and_sort_collection(&stationary_probe_locations),
                        format_and_sort_collection(&shipyard_waypoints),
                        format_and_sort_collection(&marketplace_waypoints),
                        stationary_probe_locations.len(),
                        probe_task_waypoints.len(),
                    );

                    evaluation_result
                };

                if condition_met {
                    let finish_time = Utc::now();

                    break finish_time - startup_time;
                }
            };
            completion_duration
        };

        // Use select to race between the fleet task and your condition checker
        // Add a timeout as a fallback
        tokio::select! {
            _ = tokio::time::timeout(Duration::from_secs(1800), fleet_future) => {
                println!("Fleet task completed or timed out");
            }
            completion_duration = condition_checker => {
                println!("Condition met, stopping early after {}s", completion_duration.num_seconds());
            }
        }

        // Your validation code remains the same
        let completed_tasks = bmc
            .fleet_bmc()
            .load_completed_fleet_tasks(&Ctx::Anonymous)
            .await
            .unwrap();
        let fleets = bmc.fleet_bmc().load_fleets(&Ctx::Anonymous).await.unwrap();

        assert_eq!(1, completed_tasks.len());
        assert_eq!(FleetPhaseName::ConstructJumpGate, admiral_mutex.lock().await.fleet_phase.name);
        assert_eq!(4, fleets.len());

        let siphoning_fleet = fleets
            .iter()
            .find_map(|f| match &f.cfg {
                FleetConfig::SiphoningCfg(cfg) => Some((f.clone(), cfg.clone())),
                _ => None,
            })
            .expect("One Siphoning Fleet");

        let mining_fleet = fleets
            .iter()
            .find_map(|f| match &f.cfg {
                FleetConfig::MiningCfg(cfg) => Some((f.clone(), cfg.clone())),
                _ => None,
            })
            .expect("One Mining Fleet");

        let market_observation_fleet = fleets
            .iter()
            .find_map(|f| match &f.cfg {
                FleetConfig::MarketObservationCfg(cfg) => Some((f.clone(), cfg.clone())),
                _ => None,
            })
            .expect("One MarketObservation Fleet");

        let construct_jump_gate_fleet = fleets
            .iter()
            .find_map(|f| match &f.cfg {
                FleetConfig::ConstructJumpGateCfg(cfg) => Some((f.clone(), cfg.clone())),
                _ => None,
            })
            .expect("One ConstructJumpGate Fleet");

        let siphoning_fleet_ships = admiral_mutex
            .lock()
            .await
            .get_ships_of_fleet(&siphoning_fleet.0)
            .into_iter()
            .cloned()
            .collect_vec();
        let mining_fleet_ships = admiral_mutex
            .lock()
            .await
            .get_ships_of_fleet(&mining_fleet.0)
            .into_iter()
            .cloned()
            .collect_vec();
        let construct_jump_gate_fleet_ships = admiral_mutex
            .lock()
            .await
            .get_ships_of_fleet(&construct_jump_gate_fleet.0)
            .into_iter()
            .cloned()
            .collect_vec();
        let market_observation_fleet_ships = admiral_mutex
            .lock()
            .await
            .get_ships_of_fleet(&market_observation_fleet.0)
            .into_iter()
            .cloned()
            .collect_vec();

        assert!(siphoning_fleet_ships.len() > 4);
        assert!(mining_fleet_ships.len() > 4);

        match construct_jump_gate_fleet_ships.as_slice() {
            [ship] => assert_eq!(ship.registration.role, ShipRegistrationRole::Command),
            [] => {
                panic!("expected one ship, but got 0")
            }
            ships => {
                assert_eq!(
                    1,
                    ships
                        .iter()
                        .filter(|s| s.frame.symbol == ShipFrameSymbol::FRAME_FRIGATE)
                        .count()
                );
                assert!(
                    ships
                        .iter()
                        .filter(|s| s.frame.symbol == ShipFrameSymbol::FRAME_LIGHT_FREIGHTER)
                        .count()
                        <= 4
                );
            }
        }

        {
            let guard = admiral_mutex.lock().await;

            println!("Test done, put breakpoint here for visual inspection of state");
        }
    }
}
