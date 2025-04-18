use crate::behavior_tree::behavior_args::{BehaviorArgs, DbBlackboard};
use crate::behavior_tree::behavior_tree::ActionEvent;
use crate::behavior_tree::ship_behaviors::ShipAction;
use crate::fleet;
use crate::fleet::fleet::{
    collect_fleet_decision_facts, compute_fleet_phase_with_tasks, compute_fleets_with_tasks, recompute_tasks_after_ship_finishing_behavior_tree, FleetAdmiral,
    NewTaskResult, ShipStatusReport,
};
use crate::fleet::ship_runner::ship_behavior_runner;
use crate::fleet::system_spawning_fleet::SystemSpawningFleet;
use crate::pagination::fetch_all_pages;
use crate::ship::ShipOperations;
use crate::st_client::StClientTrait;
use crate::test_objects::TestObjects;
use anyhow::{anyhow, Result};
use chrono::Utc;

use crate::marketplaces::marketplaces::{filter_waypoints_with_trait, find_marketplaces_to_collect_remotely, find_shipyards_to_collect_remotely};
use itertools::{all, Itertools};
use st_domain::blackboard_ops::BlackboardOps;
use st_domain::{
    Agent, Fleet, FleetDecisionFacts, FleetPhaseName, FleetsOverview, Ship, ShipFrameSymbol, ShipSymbol, ShipTask, StationaryProbeLocation, TradeTicket,
    TransactionActionEvent, WaypointTraitSymbol, WaypointType,
};
use st_store::bmc::ship_bmc::ShipBmcTrait;
use st_store::bmc::{ship_bmc, Bmc};
use st_store::{db, upsert_fleets_data, upsert_waypoints, Ctx, DbModelManager};
use std::collections::{HashMap, HashSet};
use std::ops::Not;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
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
}

impl FleetRunner {
    pub async fn run_fleets(
        fleet_admiral: Arc<Mutex<FleetAdmiral>>,
        client: Arc<dyn StClientTrait>,
        bmc: Arc<dyn Bmc>,
        blackboard: Arc<dyn BlackboardOps>,
        sleep_duration: Duration,
    ) -> Result<()> {
        event!(Level::INFO, "Running fleets");

        let (ship_updated_tx, ship_updated_rx): (Sender<ShipOperations>, Receiver<ShipOperations>) = tokio::sync::mpsc::channel(32);
        let (ship_action_completed_tx, ship_action_completed_rx): (Sender<ActionEvent>, Receiver<ActionEvent>) = tokio::sync::mpsc::channel(32);
        let (ship_status_report_tx, ship_status_report_rx): (Sender<ShipStatusReport>, Receiver<ShipStatusReport>) = tokio::sync::mpsc::channel(32);

        let args: BehaviorArgs = BehaviorArgs {
            blackboard: Arc::clone(&blackboard),
        };

        let mut ship_fibers: HashMap<ShipSymbol, JoinHandle<Result<()>>> = HashMap::new();

        let mut ship_ops: HashMap<ShipSymbol, Arc<Mutex<ShipOperations>>> = Default::default();

        // Clone fleet_admiral infos to avoid the lifetime issues
        let all_ships_map = fleet_admiral.lock().await.all_ships.clone();
        let all_ship_tasks = fleet_admiral.lock().await.ship_tasks.clone();

        let mut fleet_runner = Self {
            ship_fibers,
            ship_ops,
            ship_updated_tx,
            ship_action_completed_tx,
            ship_status_report_tx,
            client,
            args,
            fleet_admiral,
            bmc,
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
            Self::launch_and_register_ship(Arc::clone(&fleet_runner_mutex), &ss, ship, sleep_duration, &all_ship_tasks).await?;
        }

        tokio::join!(msg_listeners_join_handle);

        Ok(())
    }

    pub async fn launch_and_register_ship(
        runner: Arc<Mutex<FleetRunner>>,
        ss: &ShipSymbol,
        ship: Ship,
        sleep_duration: Duration,
        all_ship_tasks: &HashMap<ShipSymbol, ShipTask>,
    ) -> Result<()> {
        // if ss.0 != "FLWI-26" {
        //     return Ok(());
        // }
        let mut guard = runner.lock().await;

        let ship_op_mutex = Arc::new(Mutex::new(ShipOperations::new(ship.clone(), Arc::clone(&guard.client))));
        let maybe_ship_task = all_ship_tasks.get(&ss);

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
                    sleep_duration,
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

    pub async fn launch_ship_fibers_of_idle_or_new_ships(
        runner: Arc<Mutex<FleetRunner>>,
        all_ships: HashSet<ShipSymbol>,
        ship_tasks: HashMap<ShipSymbol, ShipTask>,
        sleep_duration: Duration,
    ) -> Result<()> {
        let not_running_ships = {
            let mut runner_guard = runner.lock().await;

            let completed_fibers =
                runner_guard.ship_fibers.iter().filter_map(|(ss, ship_fiber)| ship_fiber.is_finished().then_some(ss)).cloned().collect::<HashSet<_>>();
            let running_fibers =
                runner_guard.ship_fibers.iter().filter_map(|(ss, ship_fiber)| ship_fiber.is_finished().not().then_some(ss)).cloned().collect::<HashSet<_>>();

            let not_running_ships = all_ships.difference(&running_fibers).cloned().collect::<HashSet<_>>().union(&completed_fibers).cloned().collect_vec();

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
            Self::relaunch_ship(Arc::clone(&runner), &ss, ship_tasks.clone(), sleep_duration).await?
        }

        Ok(())
    }

    //TODO - refactor to DRY up with fn launch_and_register_ship
    pub async fn relaunch_ship(
        runner: Arc<Mutex<FleetRunner>>,
        ss: &ShipSymbol,
        ship_tasks: HashMap<ShipSymbol, ShipTask>,
        sleep_duration: Duration,
    ) -> Result<()> {
        let mut guard = runner.lock().await;

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
                    sleep_duration,
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
        sleep_duration: Duration,
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
                //println!("ship_loop: Ship {:?} is running stationary_probe_behavior", ship.symbol);
                Some((behaviors.stationary_probe_behavior, "stationary_probe_behavior"))
            }
            ShipTask::ObserveAllWaypointsOnce { waypoint_symbols } => {
                ship.set_explore_locations(waypoint_symbols);
                //println!("ship_loop: Ship {:?} is running explorer_behavior", ship.symbol);
                Some((behaviors.explorer_behavior, "explorer_behavior"))
            }
            ShipTask::MineMaterialsAtWaypoint { .. } => None,
            ShipTask::SurveyAsteroid { .. } => None,
            ShipTask::Trade { ticket_id } => {
                let ticket: TradeTicket = args.blackboard.get_ticket_by_id(ticket_id).await?;
                ship.set_trade_ticket(ticket);
                //println!("ship_loop: Ship {:?} is running trading_behavior", ship.symbol);
                Some((behaviors.trading_behavior, "trading_behavior"))
            }
        };

        match maybe_behavior {
            None => Ok(None),
            Some((ship_behavior, behavior_label)) => {
                let ship_span = span!(Level::INFO, "ship_behavior", ship = format!("{}", ship.symbol.0), behavior = behavior_label);

                let result: Result<Response, Error> = ship_behavior_runner(
                    &mut ship,
                    sleep_duration,
                    &args,
                    ship_behavior,
                    &ship_updated_tx.clone(),
                    &ship_action_completed_tx.clone(),
                )
                .instrument(ship_span)
                .await;

                let ship_span = span!(Level::INFO, "fleet_runner", ship = format!("{}", ship.symbol.0), behavior = behavior_label);
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
        ship_bmc: Arc<dyn ShipBmcTrait>,
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
                    let _ = ship_bmc.upsert_ships(&Ctx::Anonymous, &vec![updated_ship.ship.clone()], Utc::now()).await?;
                    admiral.all_ships.insert(updated_ship.symbol.clone(), updated_ship.ship);
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
            let ship_span = span!(
                Level::INFO,
                "fleet_runner::listen_to_ship_status_report_messages",
                ship = format!("{}", msg.ship_symbol().0)
            );
            let _enter = ship_span.enter();

            let mut admiral_guard = fleet_admiral.lock().await;
            admiral_guard.report_ship_action_completed(&msg, Arc::clone(&bmc)).await?;

            match msg {
                ShipStatusReport::ShipFinishedBehaviorTree(ship, task) => {
                    admiral_guard.ship_tasks.remove(&ship.symbol);
                    let result = recompute_tasks_after_ship_finishing_behavior_tree(&admiral_guard, &ship, &task, Arc::clone(&bmc)).await?;
                    event!(
                        Level::INFO,
                        message = "ShipFinishedBehaviorTree",
                        ship = ship.symbol.0,
                        recompute_result = result.to_string()
                    );
                    match result {
                        NewTaskResult::DismantleFleets { fleets_to_dismantle } => {
                            event!(
                                Level::INFO,
                                "Dismantling fleets {}",
                                fleets_to_dismantle.iter().map(|fleet_id| fleet_id.0.to_string()).join(", ")
                            );

                            FleetAdmiral::dismantle_fleets(&mut admiral_guard, fleets_to_dismantle.clone());
                            bmc.fleet_bmc().delete_fleets(&Ctx::Anonymous, &fleets_to_dismantle).await?;

                            let system_symbol = ship.nav.system_symbol;
                            let facts = collect_fleet_decision_facts(Arc::clone(&bmc), &system_symbol).await?;

                            let (fleets, fleet_tasks, fleet_phase) = compute_fleets_with_tasks(
                                system_symbol,
                                &admiral_guard.completed_fleet_tasks,
                                &facts,
                                &admiral_guard.fleets,
                                &admiral_guard.fleet_tasks,
                            );
                            // println!("Computed new fleets after dismantling the fleets: {:?}", fleets_to_dismantle);
                            // dbg!(&fleets);
                            // dbg!(&fleet_tasks);
                            // dbg!(&fleet_phase);

                            admiral_guard.fleets = fleets.into_iter().map(|f| (f.id.clone(), f)).collect();
                            admiral_guard.fleet_tasks = fleet_tasks.into_iter().map(|(fleet_id, task)| (fleet_id, vec![task])).collect();
                            admiral_guard.fleet_phase = fleet_phase;

                            //FIXME: assuming one fleet task per fleet
                            let fleet_task_list =
                                admiral_guard.fleet_tasks.iter().map(|(fleet_id, tasks)| (fleet_id.clone(), tasks.first().cloned().unwrap())).collect_vec();
                            let ship_fleet_assignment =
                                FleetAdmiral::assign_ships(&fleet_task_list, &admiral_guard.all_ships, &admiral_guard.fleet_phase.shopping_list_in_order);
                            admiral_guard.ship_fleet_assignment = ship_fleet_assignment;

                            let new_ship_tasks = FleetAdmiral::compute_ship_tasks(&mut admiral_guard, &facts, Arc::clone(&bmc)).await?;
                            FleetAdmiral::assign_ship_tasks_and_potential_requirements(&mut admiral_guard, new_ship_tasks);

                            Self::launch_ship_fibers_of_idle_or_new_ships(
                                Arc::clone(&runner),
                                admiral_guard.all_ships.keys().cloned().collect::<HashSet<_>>(),
                                admiral_guard.ship_tasks.clone(),
                                sleep_duration,
                            )
                            .await?;

                            let _ = upsert_fleets_data(
                                Arc::clone(&bmc),
                                &Ctx::Anonymous,
                                &admiral_guard.fleets,
                                &admiral_guard.fleet_tasks,
                                &admiral_guard.ship_fleet_assignment,
                                &admiral_guard.ship_tasks,
                                &admiral_guard.active_trades,
                            )
                            .await?;
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
                            bmc.ship_bmc().insert_stationary_probe(&Ctx::Anonymous, location.clone()).await?;
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
                            Self::relaunch_ship(runner.clone(), &ship_symbol, admiral_guard.ship_tasks.clone(), sleep_duration).await?
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
                        bmc.ship_bmc().upsert_ships(&Ctx::Anonymous, &vec![new_ship.clone()], Utc::now()).await?;
                        admiral_guard.all_ships.insert(new_ship.symbol.clone(), new_ship.clone());
                        admiral_guard.ship_fleet_assignment.insert(new_ship.symbol.clone(), ticket_details.assigned_fleet_id.clone());

                        let facts = collect_fleet_decision_facts(Arc::clone(&bmc), &new_ship.nav.system_symbol).await?;
                        let new_ship_tasks = FleetAdmiral::compute_ship_tasks(&mut admiral_guard, &facts, Arc::clone(&bmc)).await?;
                        FleetAdmiral::assign_ship_tasks_and_potential_requirements(&mut admiral_guard, new_ship_tasks);
                        let _ = upsert_fleets_data(
                            Arc::clone(&bmc),
                            &Ctx::Anonymous,
                            &admiral_guard.fleets,
                            &admiral_guard.fleet_tasks,
                            &admiral_guard.ship_fleet_assignment,
                            &admiral_guard.ship_tasks,
                            &admiral_guard.active_trades,
                        )
                        .await?;
                        Self::launch_and_register_ship(
                            Arc::clone(&runner),
                            &new_ship.symbol,
                            new_ship.clone(),
                            sleep_duration,
                            &admiral_guard.ship_tasks,
                        )
                        .await?
                    }
                },
            }
            drop(_enter);
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
                "fleet_runner::listen_to_ship_status_report_messages",
                ship = format!("{}", msg.ship_symbol().0)
            );
            let _enter = ship_span.enter();

            match msg {
                ActionEvent::ShipActionCompleted(ship_op, ship_action, result) => match result {
                    Ok(_) => {
                        event!(
                            Level::DEBUG,
                            message = "ShipActionCompleted",
                            ship = ship_op.symbol.0,
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
                        event!(Level::ERROR, message = "Error completing ShipAction", error = %err,
                            ship = ship_op.symbol.0,
                            action = %ship_action,
                        );
                    }
                },
                ActionEvent::BehaviorCompleted(ship_ops, ship_action, result) => match result {
                    Ok(_) => {
                        event!(
                            Level::INFO,
                            message = "BehaviorCompleted",
                            ship = ship_ops.symbol.0,
                            action = %ship_action,
                        );
                    }
                    Err(error) => {
                        event!(
                            Level::ERROR,
                            message = "BehaviorCompleted",
                            ship = ship_ops.symbol.0,
                            action = %ship_action,
                            error
                        );
                    }
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
        sleep_duration: Duration,
    ) {
        // Extract all needed data with a single lock acquisition
        let (bmc, fleet_admiral, ship_status_report_tx) = {
            let guard = runner.lock().await;
            (guard.bmc.clone(), Arc::clone(&guard.fleet_admiral), guard.ship_status_report_tx.clone())
        };

        let ship_updated_listener_join_handle = tokio::spawn(Self::listen_to_ship_changes_and_persist(
            bmc.ship_bmc(),
            Arc::clone(&fleet_admiral),
            ship_updated_rx,
        ));

        let ship_action_update_listener_join_handle =
            tokio::spawn(Self::listen_to_ship_action_update_messages(ship_status_report_tx, ship_action_completed_rx));

        let ship_status_report_listener_join_handle = tokio::spawn(Self::listen_to_ship_status_report_messages(
            fleet_admiral,
            bmc,
            ship_status_report_rx,
            Arc::clone(&runner),
            sleep_duration,
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

    async fn load_and_store_initial_data(client: Arc<dyn StClientTrait>, bmc: Arc<dyn Bmc>) -> Result<()> {
        let ctx = &Ctx::Anonymous;
        let agent = match { bmc.agent_bmc().load_agent(ctx).await } {
            Ok(agent) => agent,
            Err(_) => {
                let response = client.get_agent().await?;
                bmc.agent_bmc().store_agent(ctx, &response.data).await?;
                response.data
            }
        };

        let headquarters_system_symbol = agent.headquarters.system_symbol();

        let waypoint_entries_of_home_system = match bmc.system_bmc().get_waypoints_of_system(ctx, &headquarters_system_symbol).await {
            Ok(waypoints) if waypoints.is_empty().not() => waypoints,
            _ => {
                let waypoints = fetch_all_pages(|p| client.list_waypoints_of_system_page(&headquarters_system_symbol, p)).await?;
                bmc.system_bmc().save_waypoints_of_system(ctx, &headquarters_system_symbol, waypoints.clone()).await?;
                waypoints
            }
        };

        let marketplaces_to_collect_remotely = filter_waypoints_with_trait(&waypoint_entries_of_home_system, WaypointTraitSymbol::MARKETPLACE)
            .into_iter()
            .map(|wp| wp.symbol.clone())
            .collect_vec();

        let shipyards_to_collect_remotely =
            filter_waypoints_with_trait(&waypoint_entries_of_home_system, WaypointTraitSymbol::SHIPYARD).into_iter().map(|wp| wp.symbol.clone()).collect_vec();

        for wps in marketplaces_to_collect_remotely {
            let market = client.get_marketplace(wps).await?;
            bmc.market_bmc().save_market_data(ctx, vec![market.data], Utc::now()).await?;
        }
        for wps in shipyards_to_collect_remotely {
            let shipyard = client.get_shipyard(wps).await?;
            bmc.shipyard_bmc().save_shipyard_data(ctx, shipyard.data, Utc::now()).await?;
        }
        let jump_gate_wp_of_home_system =
            waypoint_entries_of_home_system.iter().find(|wp| wp.r#type == WaypointType::JUMP_GATE).expect("home system should have a jump-gate");

        let construction_site = match bmc.construction_bmc().get_construction_site_for_system(ctx, headquarters_system_symbol).await {
            Ok(Some(cs)) => cs,
            _ => {
                let cs = client.get_construction_site(&jump_gate_wp_of_home_system.symbol).await?;
                bmc.construction_bmc().save_construction_site(ctx, cs.clone()).await?;
                cs
            }
        };

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::bmc_blackboard::BmcBlackboard;
    use crate::fleet::fleet::FleetAdmiral;
    use crate::fleet::fleet_runner::FleetRunner;
    use crate::st_client::StClientTrait;
    use crate::universe_server::universe_server::{InMemoryUniverse, InMemoryUniverseClient};
    use itertools::Itertools;
    use st_domain::{FleetConfig, FleetId, FleetPhaseName, FleetTask, ShipRegistrationRole};
    use st_store::bmc::jump_gate_bmc::{InMemoryJumpGateBmc, JumpGateBmcTrait};
    use st_store::bmc::ship_bmc::{InMemoryShips, InMemoryShipsBmc, ShipBmcTrait};
    use st_store::bmc::{Bmc, InMemoryBmc};
    use st_store::shipyard_bmc::{InMemoryShipyardBmc, ShipyardBmcTrait};
    use st_store::trade_bmc::{InMemoryTradeBmc, TradeBmcTrait};
    use st_store::{
        AgentBmcTrait, ConstructionBmcTrait, Ctx, FleetBmcTrait, InMemoryAgentBmc, InMemoryConstructionBmc, InMemoryFleetBmc, InMemoryMarketBmc,
        InMemorySystemsBmc, MarketBmcTrait, SystemBmcTrait,
    };
    use std::sync::Arc;
    use std::time::Duration;
    use test_log::test;
    use tokio::sync::Mutex;

    //#[test(tokio::test)]
    #[tokio::test] // for accessing runtime-infos with tokio-console
    async fn create_fleet_admiral_from_startup_ship_config() {
        // uncomment for displaying tasks with `tokio-console` in terminal
        // also don't use test-tracing-subscriber `#[test(tokio::test)]` but rather #[tokio::test]
        console_subscriber::init();

        let in_memory_universe = InMemoryUniverse::from_snapshot("tests/assets/universe_snapshot.json").expect("InMemoryUniverse::from_snapshot");
        let in_memory_client = InMemoryUniverseClient::new(in_memory_universe);

        let agent = in_memory_client.get_agent().await.expect("agent").data;
        let hq_system_symbol = agent.headquarters.system_symbol();

        let ship_bmc = InMemoryShipsBmc::new(InMemoryShips::new());
        let agent_bmc = InMemoryAgentBmc::new(agent);
        let trade_bmc = InMemoryTradeBmc::new();
        let fleet_bmc = InMemoryFleetBmc::new();
        let system_bmc = InMemorySystemsBmc::new();
        let construction_bmc = InMemoryConstructionBmc::new();

        //insert some data
        //construction_bmc.save_construction_site(&Ctx::Anonymous, in_memory_client.get_construction_site().unwrap())

        let market_bmc = InMemoryMarketBmc::new();
        let shipyard_bmc = InMemoryShipyardBmc::new();
        let jump_gate_bmc = InMemoryJumpGateBmc::new();

        let bmc = InMemoryBmc {
            ship_bmc: Arc::new(ship_bmc) as Arc<dyn ShipBmcTrait>,
            fleet_bmc: Arc::new(fleet_bmc) as Arc<dyn FleetBmcTrait>,
            trade_bmc: Arc::new(trade_bmc) as Arc<dyn TradeBmcTrait>,
            system_bmc: Arc::new(system_bmc) as Arc<dyn SystemBmcTrait>,
            agent_bmc: Arc::new(agent_bmc) as Arc<dyn AgentBmcTrait>,
            construction_bmc: Arc::new(construction_bmc) as Arc<dyn ConstructionBmcTrait>,
            market_bmc: Arc::new(market_bmc) as Arc<dyn MarketBmcTrait>,
            jump_gate_bmc: Arc::new(jump_gate_bmc) as Arc<dyn JumpGateBmcTrait>,
            shipyard_bmc: Arc::new(shipyard_bmc) as Arc<dyn ShipyardBmcTrait>,
        };

        let client = Arc::new(in_memory_client) as Arc<dyn StClientTrait>;
        let bmc = Arc::new(bmc) as Arc<dyn Bmc>;
        let blackboard = BmcBlackboard::new(Arc::clone(&bmc));

        FleetRunner::load_and_store_initial_data(Arc::clone(&client), Arc::clone(&bmc)).await.expect("FleetRunner::load_and_store_initial_data");

        println!("Creating fleet admiral");

        let mut fleet_admiral =
            FleetAdmiral::load_or_create(Arc::clone(&bmc), hq_system_symbol, Arc::clone(&client)).await.expect("FleetAdmiral::load_or_create");

        assert!(matches!(
            fleet_admiral.fleet_tasks.get(&FleetId(0)).cloned().unwrap_or_default().get(0),
            Some(FleetTask::CollectMarketInfosOnce { .. })
        ));
        assert!(matches!(
            fleet_admiral.fleet_tasks.get(&FleetId(1)).cloned().unwrap_or_default().get(0),
            Some(FleetTask::ObserveAllWaypointsOfSystemWithStationaryProbes { .. })
        ));

        let admiral_mutex = Arc::new(Mutex::new(fleet_admiral));
        let admiral_clone = Arc::clone(&admiral_mutex);

        // This task runs your fleets
        let fleet_task = async {
            println!("Running fleets");
            FleetRunner::run_fleets(
                Arc::clone(&admiral_mutex),
                Arc::clone(&client),
                Arc::clone(&bmc),
                Arc::new(blackboard),
                Duration::from_millis(1),
            )
            .await
            .unwrap();
        };

        // This task periodically checks if the condition is met
        let condition_checker = async {
            let check_interval = Duration::from_millis(1000); // Adjust as needed

            loop {
                // Sleep first to give the fleet a chance to start
                tokio::time::sleep(check_interval).await;

                let condition_met = {
                    let admiral = admiral_clone.lock().await;
                    let has_finished_initial_observation =
                        admiral.completed_fleet_tasks.iter().any(|t| matches!(t.task, FleetTask::CollectMarketInfosOnce { .. }));
                    let is_in_construction_phase = admiral.fleet_phase.name == FleetPhaseName::ConstructJumpGate;
                    let num_ships = admiral.all_ships.len();
                    let has_bought_ships = num_ships > 2;

                    println!(
                        r#"
has_finished_initial_observation: {has_finished_initial_observation}
is_in_construction_phase: {is_in_construction_phase}
num_ships: {num_ships}
"#
                    );

                    has_finished_initial_observation && is_in_construction_phase && has_bought_ships
                };

                if condition_met {
                    break;
                }
            }
        };

        // Use select to race between the fleet task and your condition checker
        // Add a timeout as a fallback
        tokio::select! {
            _ = tokio::time::timeout(Duration::from_secs(60), fleet_task) => {
                println!("Fleet task completed or timed out");
            }
            _ = condition_checker => {
                println!("Condition met, stopping early");
            }
        }

        // Your validation code remains the same
        let completed_tasks = bmc.fleet_bmc().load_completed_fleet_tasks(&Ctx::Anonymous).await.unwrap();
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

        let siphoning_fleet_ships = admiral_mutex.lock().await.get_ships_of_fleet(&siphoning_fleet.0).into_iter().cloned().collect_vec();
        let mining_fleet_ships = admiral_mutex.lock().await.get_ships_of_fleet(&mining_fleet.0).into_iter().cloned().collect_vec();
        let construct_jump_gate_fleet_ships = admiral_mutex.lock().await.get_ships_of_fleet(&construct_jump_gate_fleet.0).into_iter().cloned().collect_vec();
        let market_observation_fleet_ships = admiral_mutex.lock().await.get_ships_of_fleet(&market_observation_fleet.0).into_iter().cloned().collect_vec();

        assert!(siphoning_fleet_ships.is_empty());
        assert!(mining_fleet_ships.is_empty());

        match construct_jump_gate_fleet_ships.as_slice() {
            [ship] => assert_eq!(ship.registration.role, ShipRegistrationRole::Command),
            _ => panic!("expected one ship, but got {}", construct_jump_gate_fleet_ships.len()),
        }

        let probes = market_observation_fleet_ships.iter().filter(|ship| ship.registration.role == ShipRegistrationRole::Satellite).collect_vec();
        assert!(probes.len() > 1, "more than one probe");
        assert_eq!(probes.len(), market_observation_fleet_ships.len(), "only probes");
    }
}
