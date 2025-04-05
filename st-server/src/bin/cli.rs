use anyhow::{anyhow, Result};
use chrono::Utc;
use clap;
use clap::{Arg, ArgAction, Parser, Subcommand};
use st_core::agent_manager;
use st_core::behavior_tree::behavior_args::{BehaviorArgs, BlackboardOps, DbBlackboard};
use st_core::behavior_tree::behavior_tree::{ActionEvent, Actionable, Behavior};
use st_core::behavior_tree::ship_behaviors::{ship_behaviors, ShipAction};
use st_core::configuration::AgentConfiguration;
use st_core::exploration::exploration::{get_exploration_tasks_for_waypoint, ExplorationTask};
use st_core::fleet::fleet::{FleetAdmiral, ShipStatusReport};
use st_core::reqwest_helpers::create_client;
use st_core::ship::ShipOperations;
use st_core::st_client::{StClient, StClientTrait};
use st_domain::{Ship, ShipSymbol, Waypoint, WaypointSymbol};
use st_server::cli_args::AppConfig;
use st_store::{db, Ctx, DbModelManager, MarketBmc, SystemBmc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::Mutex;
use tracing::{event, Level};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

/// SpaceTraders CLI utility
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Subcommand to run
    #[command(subcommand)]
    command: MyCommand,
}

/// Available commands
#[derive(Subcommand, Debug, Clone)]
enum MyCommand {
    /// Collect waypoint information
    CollectWaypointInfos {
        /// The ship symbol that is located at the waypoint of interest
        #[arg(long, required = true)]
        ship_symbol: String,
    },
    /// Run a specific behavior
    RunBehavior {
        /// The ship symbol to run the behavior on
        #[arg(long, required = true)]
        ship_symbol: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let AppConfig {
        database_url,
        spacetraders_agent_faction,
        spacetraders_agent_symbol,
        spacetraders_registration_email,
        spacetraders_account_token,
    } = AppConfig::from_env().expect("cfg");

    tracing_subscriber::registry().with(fmt::layer().with_span_events(fmt::format::FmtSpan::CLOSE)).with(EnvFilter::from_default_env()).init();

    let cfg: AgentConfiguration = AgentConfiguration {
        database_url,
        spacetraders_agent_faction,
        spacetraders_agent_symbol,
        spacetraders_registration_email,
        spacetraders_account_token,
    };

    let args = Args::parse();

    let pool = db::get_pg_connection_pool(cfg.pg_connection_string()).await.expect("should be able to get pool");

    let mm = DbModelManager::new(pool);
    let client_with_account_token = create_client(Some(cfg.spacetraders_account_token.clone()), None);
    let client_with_account_token = StClient::new(client_with_account_token);

    let authenticated_client = agent_manager::get_authenticated_client(&cfg, mm.pool().clone(), client_with_account_token).await?;

    match args.command {
        MyCommand::CollectWaypointInfos { ship_symbol } => {
            let ship_symbol = ShipSymbol(ship_symbol);

            collect_waypoint_infos(&mm, &authenticated_client, ship_symbol).await;
        }
        MyCommand::RunBehavior { ship_symbol } => {
            let ship_symbol = ShipSymbol(ship_symbol);
            let behaviors = ship_behaviors();
            let behavior = behaviors.trading_behavior;

            run_behavior(mm, authenticated_client, ship_symbol, behavior).await?;
        }
    }

    Ok(())
}

async fn collect_waypoint_infos(mm: &DbModelManager, authenticated_client: &StClient, ship_symbol: ShipSymbol) -> Result<()> {
    let ship = authenticated_client.get_ship(ship_symbol).await?.data;

    let waypoint_symbol = ship.nav.waypoint_symbol.clone();

    let waypoints = SystemBmc::get_waypoints_of_system(&Ctx::Anonymous, &mm, &waypoint_symbol.system_symbol()).await?;
    let waypoint = waypoints.iter().find(|wp| wp.symbol == waypoint_symbol).unwrap();

    let exploration_tasks = get_exploration_tasks_for_waypoint(&waypoint);

    for task in exploration_tasks {
        match task {
            ExplorationTask::CreateChart => return Err(anyhow!("Waypoint should have been charted by now")),
            ExplorationTask::GetMarket => {
                println!("Getting marketplace data...");
                let market = authenticated_client.get_marketplace(waypoint_symbol.clone()).await?;
                db::insert_market_data(mm.pool(), vec![market.data], Utc::now()).await?;
                println!("Inserted marketplace data successfully.");
            }
            ExplorationTask::GetJumpGate => {
                println!("Getting jump_gate data...");
                let jump_gate = authenticated_client.get_jump_gate(waypoint_symbol.clone()).await?;
                db::insert_jump_gates(mm.pool(), vec![jump_gate.data], Utc::now()).await?;
                println!("Inserted marketplace data successfully.");
            }
            ExplorationTask::GetShipyard => {
                println!("Getting shipyard data...");
                let shipyard = authenticated_client.get_shipyard(waypoint_symbol.clone()).await?;
                db::insert_shipyards(mm.pool(), vec![shipyard.data], Utc::now()).await?;
                println!("Inserted marketplace data successfully.");
            }
        }
    }

    Ok(())
}

async fn run_behavior(mm: DbModelManager, authenticated_client: StClient, ship_symbol: ShipSymbol, behavior: Behavior<ShipAction>) -> Result<()> {
    let client: Arc<dyn StClientTrait> = Arc::new(authenticated_client);
    let ship = client.get_ship(ship_symbol).await?.data;
    let (ship_updated_tx, ship_updated_rx): (Sender<ShipOperations>, Receiver<ShipOperations>) = tokio::sync::mpsc::channel(32);
    let (ship_action_completed_tx, ship_action_completed_rx): (Sender<ActionEvent>, Receiver<ActionEvent>) = tokio::sync::mpsc::channel(32);
    let (ship_status_report_tx, ship_status_report_rx): (Sender<ShipStatusReport>, Receiver<ShipStatusReport>) = tokio::sync::mpsc::channel(32);

    let db_blackboard: Arc<dyn BlackboardOps> = Arc::new(DbBlackboard { model_manager: mm.clone() });

    let message_listeners_join_handle = tokio::spawn({
        let mm = mm.clone();
        run_message_listeners(mm, ship_updated_rx, ship_action_completed_rx, ship_status_report_rx, ship_status_report_tx)
    });

    let ship_behavior_join_handle = tokio::spawn({
        let mut ship_op = ShipOperations::new(ship, Arc::clone(&client));
        let behavior = behavior.clone();
        let behavior_args = BehaviorArgs {
            blackboard: Arc::clone(&db_blackboard),
        };

        async move {
            behavior
                .run(
                    &behavior_args,
                    &mut ship_op,
                    Duration::from_secs(10),
                    &ship_updated_tx,
                    &ship_action_completed_tx,
                )
                .await
        }
    });

    tokio::join!(message_listeners_join_handle, ship_behavior_join_handle);
    Ok(())
}

async fn run_message_listeners(
    db_model_manager: DbModelManager,
    ship_updated_rx: Receiver<ShipOperations>,
    ship_action_completed_rx: Receiver<ActionEvent>,
    ship_status_report_rx: Receiver<ShipStatusReport>,
    ship_status_report_tx: Sender<ShipStatusReport>,
) {
    let ship_updated_listener_join_handle = tokio::spawn(listen_to_ship_changes_and_persist(db_model_manager.clone(), ship_updated_rx));

    let ship_action_update_listener_join_handle = tokio::spawn(listen_to_ship_action_update_messages(ship_status_report_tx, ship_action_completed_rx));

    let ship_status_report_listener_join_handle = tokio::spawn({
        let mm = db_model_manager.clone();
        listen_to_ship_status_report_messages(mm, ship_status_report_rx)
    });

    // run forever
    tokio::join!(
        ship_updated_listener_join_handle,
        ship_action_update_listener_join_handle,
        ship_status_report_listener_join_handle
    );
    unreachable!()
}

async fn listen_to_ship_changes_and_persist(mm: DbModelManager, mut ship_updated_rx: Receiver<ShipOperations>) -> Result<()> {
    while let Some(updated_ship) = ship_updated_rx.recv().await {
        event!(Level::INFO, "Got Ship Change Message For Ship {}", updated_ship.symbol.0);
        let _ = st_store::upsert_ships(mm.pool(), &vec![updated_ship.ship.clone()], Utc::now()).await?;
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
                std::prelude::rust_2015::Ok((ship_op, ship_action)) => {
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

pub async fn listen_to_ship_status_report_messages(db_model_manager: DbModelManager, mut ship_status_report_rx: Receiver<ShipStatusReport>) -> Result<()> {
    event!(Level::INFO, "Fleet_runner::listen_to_ship_status_report_messages - starting");

    while let Some(msg) = ship_status_report_rx.recv().await {
        event!(
            Level::INFO,
            message = "Fleet_runner::listen_to_ship_status_report_messages - got message",
            msg = serde_json::to_string(&msg)?
        );

        event!(
            Level::INFO,
            "Fleet_runner::listen_to_ship_status_report_messages - successfully processed message"
        );
    }

    Ok(())
}
