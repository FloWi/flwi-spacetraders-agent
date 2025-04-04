use crate::behavior_tree::behavior_args::{BehaviorArgs, DbBlackboard};
use crate::behavior_tree::behavior_tree::ActionEvent;
use crate::behavior_tree::ship_behaviors::ShipAction;
use crate::fleet::fleet::{FleetAdmiral, ShipStatusReport};
use crate::ship::ShipOperations;
use crate::st_client::StClientTrait;
use anyhow::Result;
use chrono::Utc;
use st_domain::{Ship, ShipSymbol, ShipTask};
use st_store::DbModelManager;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::event;
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

        for (ss, ship) in all_ships_map {
            Self::launch_and_register_ship(Arc::clone(&fleet_runner_mutex), &ss, ship).await?;
        }

        Self::run_message_listeners(
            Arc::clone(&fleet_runner_mutex),
            ship_updated_rx,
            ship_action_completed_rx,
            ship_status_report_rx,
        )
        .await;

        Ok(())
    }

    async fn launch_and_register_ship(runner: Arc<Mutex<FleetRunner>>, ss: &ShipSymbol, ship: Ship) -> Result<()> {
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
            let ship_task_clone = ship_task.clone();
            let ship_symbol_clone = ss.clone();

            let fiber = tokio::spawn(async move {
                Self::ship_loop(
                    ship_op_clone,
                    args_clone,
                    ship_updated_tx_clone,
                    ship_action_completed_tx_clone,
                    ship_task_clone,
                )
                .await?;
                Ok(())
            });

            guard.ship_fibers.insert(ship_symbol_clone, fiber);
        }
        guard.ship_ops.insert(ss.clone(), ship_op_mutex);
        Ok(())
    }

    pub async fn ship_loop(
        ship_op: Arc<Mutex<ShipOperations>>,
        args: BehaviorArgs,
        ship_updated_tx: Sender<ShipOperations>,
        ship_action_completed_tx: Sender<ActionEvent>,
        ship_task: ShipTask,
    ) -> Result<()> {
        use crate::behavior_tree::behavior_tree::{Actionable, Response};
        use crate::behavior_tree::ship_behaviors::ship_behaviors;
        use anyhow::Error;
        use std::time::Duration;
        use tracing::{span, Level};
        let behaviors = ship_behaviors();

        let mut ship = ship_op.lock().await;

        let maybe_behavior = match ship_task {
            ShipTask::ObserveWaypointDetails { waypoint_symbol } => {
                ship.set_permanent_observation_location(waypoint_symbol);
                println!("ship_loop: Ship {:?} is running stationary_probe_behavior", ship.symbol);
                Some(behaviors.stationary_probe_behavior)
            }
            ShipTask::ObserveAllWaypointsOnce { waypoint_symbols } => {
                ship.set_explore_locations(waypoint_symbols);
                println!("ship_loop: Ship {:?} is running explorer_behavior", ship.symbol);
                Some(behaviors.explorer_behavior)
            }
            ShipTask::MineMaterialsAtWaypoint { .. } => None,
            ShipTask::SurveyAsteroid { .. } => None,
            ShipTask::Trade { ticket } => {
                ship.set_trade_ticket(ticket);
                println!("ship_loop: Ship {:?} is running trading_behavior", ship.symbol);
                Some(behaviors.trading_behavior)
            }
        };

        match maybe_behavior {
            None => {}
            Some(ship_behavior) => {
                let mut tick: usize = 0;
                let span = span!(Level::INFO, "ship_loop", tick, ship = format!("{}", ship.symbol.0),);
                tick += 1;

                let _enter = span.enter();

                let result: std::result::Result<Response, Error> = ship_behavior
                    .run(
                        &args,
                        &mut ship,
                        Duration::from_secs(5),
                        &ship_updated_tx.clone(),
                        &ship_action_completed_tx.clone(),
                    )
                    .await;

                match &result {
                    Ok(o) => {
                        event!(
                            Level::INFO,
                            message = "Ship Tick done ",
                            result = %o,
                        );
                    }
                    Err(e) => {
                        event!(
                            Level::INFO,
                            message = "Ship Tick done with Error",
                            result = %e,
                        );
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn listen_to_ship_changes_and_persist(runner: Arc<Mutex<FleetRunner>>, mut ship_updated_rx: Receiver<ShipOperations>) -> Result<()> {
        let guard = runner.lock().await;
        let mm = guard.db_model_manager.clone();
        while let Some(updated_ship) = ship_updated_rx.recv().await {
            let mut admiral = guard.fleet_admiral.lock().await;
            let maybe_old_ship = admiral.all_ships.get(&updated_ship.symbol).cloned();

            match maybe_old_ship {
                Some(old_ship) if old_ship == updated_ship.ship => {
                    // no need to update
                    event!(Level::INFO, "No need to update ship {}. No change detected", updated_ship.symbol.0);
                }
                _ => {
                    event!(Level::INFO, "Ship {} updated", updated_ship.symbol.0);
                    let _ = st_store::upsert_ships(mm.pool(), &vec![updated_ship.ship.clone()], Utc::now()).await?;
                    admiral.all_ships.insert(updated_ship.symbol.clone(), updated_ship.ship);
                }
            }
        }

        Ok(())
    }

    pub async fn listen_to_ship_status_report_messages(runner: Arc<Mutex<FleetRunner>>, mut ship_status_report_rx: Receiver<ShipStatusReport>) -> Result<()> {
        let mm = runner.lock().await.db_model_manager.clone();

        let admiral = Arc::clone(&runner.lock().await.fleet_admiral);

        while let Some(msg) = ship_status_report_rx.recv().await {
            let mut admiral = admiral.lock().await;
            admiral.report_ship_action_completed(msg, &mm).await?;
        }

        Ok(())
    }
    pub async fn listen_to_ship_action_update_messages(runner: Arc<Mutex<FleetRunner>>, mut ship_action_completed_rx: Receiver<ActionEvent>) -> Result<()> {
        let ship_status_report_tx = runner.lock().await.ship_status_report_tx.clone();
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
        let ship_updated_listener_join_handle = tokio::spawn(Self::listen_to_ship_changes_and_persist(Arc::clone(&runner), ship_updated_rx));

        let ship_action_update_listener_join_handle = tokio::spawn(Self::listen_to_ship_action_update_messages(Arc::clone(&runner), ship_action_completed_rx));
        let ship_status_report_listener_join_handle = tokio::spawn(Self::listen_to_ship_status_report_messages(Arc::clone(&runner), ship_status_report_rx));

        // run forever
        tokio::join!(
            ship_updated_listener_join_handle,
            ship_action_update_listener_join_handle,
            ship_status_report_listener_join_handle
        );

        todo!()
    }
}
