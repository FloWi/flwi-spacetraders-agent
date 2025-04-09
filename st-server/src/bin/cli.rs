use anyhow::{anyhow, Result};
use chrono::Utc;
use clap;
use clap::{Arg, ArgAction, Parser, Subcommand};
use itertools::Itertools;
use lazy_static::lazy_static;
use leptos::html::Mark;
use serde::{Deserialize, Serialize};
use st_core::agent_manager;
use st_core::behavior_tree::behavior_args::{BehaviorArgs, BlackboardOps, DbBlackboard};
use st_core::behavior_tree::behavior_tree::{ActionEvent, Actionable, Behavior};
use st_core::behavior_tree::ship_behaviors::{ship_behaviors, ShipAction};
use st_core::configuration::AgentConfiguration;
use st_core::exploration::exploration::{get_exploration_tasks_for_waypoint, ExplorationTask};
use st_core::fleet::fleet::{collect_fleet_decision_facts, diff_waypoint_symbols, FleetAdmiral, ShipStatusReport};
use st_core::pagination::fetch_all_pages;
use st_core::reqwest_helpers::create_client;
use st_core::ship::ShipOperations;
use st_core::st_client::{StClient, StClientTrait};
use st_domain::trading::find_trading_opportunities;
use st_domain::{
    trading, EvaluatedTradingOpportunity, NavStatus, PurchaseGoodTicketDetails, SellGoodTicketDetails, Ship, ShipFrameSymbol, ShipSymbol, SystemSymbol,
    TicketId, TradeTicket, Waypoint, WaypointSymbol,
};
use st_server::cli_args::AppConfig;
use st_store::trade_bmc::TradeBmc;
use st_store::{
    db, select_latest_marketplace_entry_of_system, select_latest_shipyard_entry_of_system, Ctx, DbModelManager, FleetBmc, MarketBmc, ShipBmc, SystemBmc,
};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use time::format_description;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::Mutex;
use tracing::{event, span, Instrument, Level};
use tracing_appender::{
    non_blocking::NonBlocking,
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, registry::Registry, EnvFilter};

use tracing_subscriber::fmt::time::UtcTime;

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
    RunTradingBehavior {
        /// The ship symbol to run the behavior on
        #[arg(long, required = true)]
        ship_symbol: String,
    },

    /// Run a specific behavior
    RunExplorerBehavior {
        /// The ship symbol to run the behavior on
        #[arg(long, required = true)]
        ship_symbol: String,
    },

    MoveProbesToObservationWaypoints,
}

#[tokio::main]
async fn main() -> Result<()> {
    setup_tracing();

    let AppConfig {
        database_url,
        spacetraders_agent_faction,
        spacetraders_agent_symbol,
        spacetraders_registration_email,
        spacetraders_account_token,
    } = AppConfig::from_env().expect("cfg");

    //tracing_subscriber::registry().with(fmt::layer().with_span_events(fmt::format::FmtSpan::CLOSE)).with(EnvFilter::from_default_env()).init();

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

    let authenticated_client: Arc<dyn StClientTrait> =
        Arc::new(agent_manager::get_authenticated_client(&cfg, mm.pool().clone(), client_with_account_token).await?);

    match args.command {
        MyCommand::CollectWaypointInfos { ship_symbol } => {
            let ship_symbol = ShipSymbol(ship_symbol);

            collect_waypoint_infos(&mm, authenticated_client, ship_symbol).await?;
        }
        MyCommand::MoveProbesToObservationWaypoints => {
            let system_symbol = authenticated_client.get_agent().await?.data.headquarters.system_symbol();

            let waypoints_of_interest = get_waypoints_of_interest(&system_symbol, &mm).await?;

            let ships: Vec<Ship> = fetch_all_pages(|p| authenticated_client.list_ships(p)).await?;
            let probe_ships: Vec<Ship> = ships.iter().filter(|s| s.frame.symbol == ShipFrameSymbol::FRAME_PROBE).cloned().collect_vec();
            let probes_by_locations: HashMap<WaypointSymbol, Vec<Ship>> = probe_ships.iter().cloned().into_group_map_by(|s| s.nav.waypoint_symbol.clone());
            // we potentially purchased multiple probes at the shipyards (which are locations of interest).
            // So, we just pick the first one each and mark it as correctly placed.
            let correctly_placed_probes: HashMap<WaypointSymbol, Ship> = probes_by_locations
                .iter()
                .filter_map(|(wps, ships_at_wps)| waypoints_of_interest.contains(wps).then_some((wps.clone(), ships_at_wps.first().cloned().unwrap().clone())))
                .collect();

            let unassigned_waypoints: Vec<WaypointSymbol> =
                waypoints_of_interest.iter().filter(|wps| correctly_placed_probes.contains_key(wps) == false).cloned().collect_vec();

            let unassigned_ships: Vec<Ship> = probe_ships
                .iter()
                .filter(|ship| correctly_placed_probes.iter().any(|(_, correct_ship)| correct_ship.symbol == ship.symbol) == false)
                .cloned()
                .collect_vec();

            let new_assignments: Vec<(Ship, WaypointSymbol)> = unassigned_ships.iter().cloned().zip(unassigned_waypoints.iter().cloned()).collect_vec();

            println!("Found {} probes", probe_ships.len());
            println!("Found {} correctly placed probes", correctly_placed_probes.len());
            println!("Found {} unassigned waypoints", unassigned_waypoints.len());
            println!("Found {} unassigned ships", unassigned_ships.len());
            println!("Assigned {} probes to waypoints", unassigned_ships.len());
            println!("Moving Ships");
            for (ship, wps) in new_assignments {
                match ship.nav.status {
                    NavStatus::InTransit => {
                        println!("Can't move probe {}, because it's already in transit", ship.symbol.0);
                    }
                    NavStatus::InOrbit => {
                        println!("Moving probe {} to {}", &ship.symbol.0, &wps.0);
                        authenticated_client.navigate(ship.symbol, &wps).await?;
                    }
                    NavStatus::Docked => {
                        println!("Undocking probe {}", &ship.symbol.0);
                        authenticated_client.orbit_ship(ship.symbol.clone()).await?;
                        println!("Moving probe {} to {}", &ship.symbol.0, &wps.0);
                        authenticated_client.navigate(ship.symbol, &wps).await?;
                    }
                }
            }
        }
        MyCommand::RunExplorerBehavior { ship_symbol } => {
            let ship_symbol = ShipSymbol(ship_symbol);
            run_behavior(mm, Arc::clone(&authenticated_client), ship_symbol, CliShipBehavior::CollectWaypointInfosOnce).await?;
        }
        MyCommand::RunTradingBehavior { ship_symbol } => {
            let ship_symbol = ShipSymbol(ship_symbol);
            run_behavior(mm, Arc::clone(&authenticated_client), ship_symbol, CliShipBehavior::Trading).await?;
        }
    }

    Ok(())
}

async fn get_waypoints_exploration_tasks(system_symbol: &SystemSymbol, mm: &DbModelManager) -> Result<Vec<(WaypointSymbol, Vec<ExplorationTask>)>> {
    let waypoints_of_interest = get_waypoints_of_interest(&system_symbol, mm).await?;

    let exploration_tasks = SystemBmc::get_waypoints_of_system(&Ctx::Anonymous, &mm, system_symbol)
        .await?
        .iter()
        .filter(|wp| waypoints_of_interest.contains(&wp.symbol))
        .map(|wp| (wp.symbol.clone(), get_exploration_tasks_for_waypoint(wp)))
        .collect_vec();

    Ok(exploration_tasks)
}

async fn get_waypoints_of_interest(system_symbol: &SystemSymbol, mm: &DbModelManager) -> Result<HashSet<WaypointSymbol>> {
    let marketplaces_of_interest = select_latest_marketplace_entry_of_system(mm.pool(), system_symbol).await?;
    let shipyards_of_interest = select_latest_shipyard_entry_of_system(mm.pool(), &system_symbol).await?;

    let waypoints_of_interest = marketplaces_of_interest
        .iter()
        .map(|db_entry| WaypointSymbol(db_entry.waypoint_symbol.clone()))
        .chain(shipyards_of_interest.iter().map(|db_entry| WaypointSymbol(db_entry.waypoint_symbol.clone())))
        .collect::<HashSet<_>>();
    Ok(waypoints_of_interest)
}

async fn collect_waypoint_infos(mm: &DbModelManager, authenticated_client: Arc<dyn StClientTrait>, ship_symbol: ShipSymbol) -> Result<()> {
    let ship = authenticated_client.get_ship(ship_symbol).await?.data;

    let waypoint_symbol = ship.nav.waypoint_symbol.clone();

    let waypoints = SystemBmc::get_waypoints_of_system(&Ctx::Anonymous, &mm, &waypoint_symbol.system_symbol()).await?;
    let waypoint = waypoints.iter().find(|wp| wp.symbol == waypoint_symbol).unwrap();

    let exploration_tasks = get_exploration_tasks_for_waypoint(&waypoint);

    collect_waypoints_infos_for_waypoint(mm, authenticated_client, waypoint_symbol, exploration_tasks).await?;

    Ok(())
}

async fn collect_waypoints_infos_for_waypoint(
    mm: &DbModelManager,
    authenticated_client: Arc<dyn StClientTrait>,
    waypoint_symbol: WaypointSymbol,
    exploration_tasks: Vec<ExplorationTask>,
) -> Result<()> {
    for task in exploration_tasks {
        match task {
            ExplorationTask::CreateChart => return Err(anyhow!("Waypoint should have been charted by now")),
            ExplorationTask::GetMarket => {
                event!(Level::INFO, message = "loading marketplace data", waypoint_symbol = waypoint_symbol.0);
                let market = authenticated_client.get_marketplace(waypoint_symbol.clone()).await?;
                db::insert_market_data(mm.pool(), vec![market.data], Utc::now()).await?;
                event!(
                    Level::INFO,
                    message = "inserted marketplace data successfully.",
                    waypoint_symbol = waypoint_symbol.0
                );
            }
            ExplorationTask::GetJumpGate => {
                event!(Level::INFO, message = "loading jump_gate data", waypoint_symbol = waypoint_symbol.0);
                let jump_gate = authenticated_client.get_jump_gate(waypoint_symbol.clone()).await?;
                db::insert_jump_gates(mm.pool(), vec![jump_gate.data], Utc::now()).await?;
                event!(
                    Level::INFO,
                    message = "inserted jump_gate data successfully.",
                    waypoint_symbol = waypoint_symbol.0
                );
            }
            ExplorationTask::GetShipyard => {
                event!(Level::INFO, message = "loading shipyard data", waypoint_symbol = waypoint_symbol.0);
                let shipyard = authenticated_client.get_shipyard(waypoint_symbol.clone()).await?;
                db::insert_shipyards(mm.pool(), vec![shipyard.data], Utc::now()).await?;
                event!(
                    Level::INFO,
                    message = "inserted shipyard data successfully.",
                    waypoint_symbol = waypoint_symbol.0
                );
            }
        }
    }

    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
enum CliShipBehavior {
    CollectWaypointInfosOnce,
    Trading,
}

async fn run_behavior(
    mm: DbModelManager,
    authenticated_client: Arc<dyn StClientTrait>,
    ship_symbol: ShipSymbol,
    cli_ship_behavior: CliShipBehavior,
) -> Result<()> {
    let behavior_label = match cli_ship_behavior.clone() {
        CliShipBehavior::CollectWaypointInfosOnce => "explorer_behavior",
        CliShipBehavior::Trading => "trading_behavior",
    };

    let ship = authenticated_client.get_ship(ship_symbol.clone()).await?.data;
    let (ship_updated_tx, ship_updated_rx): (Sender<ShipOperations>, Receiver<ShipOperations>) = tokio::sync::mpsc::channel(32);
    let (ship_action_completed_tx, ship_action_completed_rx): (Sender<ActionEvent>, Receiver<ActionEvent>) = tokio::sync::mpsc::channel(32);
    let (ship_status_report_tx, ship_status_report_rx): (Sender<ShipStatusReport>, Receiver<ShipStatusReport>) = tokio::sync::mpsc::channel(32);

    let db_blackboard: Arc<dyn BlackboardOps> = Arc::new(DbBlackboard { model_manager: mm.clone() });

    let message_listeners_join_handle = tokio::spawn({
        let mm = mm.clone();
        let message_listener_span = span!(Level::INFO, "message_listener", ship = ship_symbol.0, behavior = behavior_label);
        run_message_listeners(mm, ship_updated_rx, ship_action_completed_rx, ship_status_report_rx, ship_status_report_tx).instrument(message_listener_span)
    });

    let ships: Vec<Ship> = fetch_all_pages(|p| authenticated_client.list_ships(p)).await?;
    db::upsert_ships(mm.pool(), &ships, Utc::now()).await?;

    let system_symbol = authenticated_client.get_agent().await?.data.headquarters.system_symbol();
    let observation_tasks: Vec<(WaypointSymbol, Vec<ExplorationTask>)> = get_waypoints_exploration_tasks(&system_symbol, &mm).await?;

    let waypoint_observation_join_handle = tokio::spawn({
        let client = Arc::clone(&authenticated_client);
        let mm = mm.clone();
        let waypoint_span = span!(Level::INFO, "waypoint_observation");

        async move {
            let tick_duration = Duration::from_secs(5 * 60);
            let mut interval = tokio::time::interval(tick_duration);

            loop {
                let observation_tasks = observation_tasks.clone();
                interval.tick().await; // First tick finishes immediately.
                let ships = ShipBmc::get_ships(&Ctx::Anonymous, &mm, None).await.expect("Ships");
                let waypoints_with_ships: HashSet<WaypointSymbol> =
                    ships.iter().filter(|s| s.nav.status != NavStatus::InTransit).map(|s| s.nav.waypoint_symbol.clone()).collect();
                let relevant_tasks = observation_tasks.into_iter().filter(|(wps, _)| waypoints_with_ships.contains(wps)).collect_vec();
                event!(Level::INFO, "Collecting infos for {} waypoints", &relevant_tasks.len());
                for (wps, tasks) in relevant_tasks.iter().cloned() {
                    collect_waypoints_infos_for_waypoint(&mm, Arc::clone(&client), wps, tasks).await.expect("collect_waypoints_infos_for_waypoint")
                }
                event!(
                    Level::INFO,
                    "Done collecting infos for {} waypoints. Next tick in {:?}",
                    relevant_tasks.len(),
                    tick_duration
                );
            }
        }
        .instrument(waypoint_span)
    });

    let ship_span = span!(Level::INFO, "ship_behavior", ship = ship_symbol.0, behavior = behavior_label);

    let ship_behavior_join_handle = tokio::spawn({
        let mm = mm.clone();
        let client = Arc::clone(&authenticated_client);

        async move {
            let behaviors = ship_behaviors();

            match cli_ship_behavior {
                CliShipBehavior::CollectWaypointInfosOnce => {
                    explore_waypoints_once(
                        client,
                        ship,
                        ship_updated_tx,
                        ship_action_completed_tx,
                        db_blackboard,
                        mm,
                        behaviors.explorer_behavior,
                    )
                    .instrument(ship_span)
                    .await
                }
                CliShipBehavior::Trading => {
                    trade_forever(
                        client,
                        ship,
                        ship_updated_tx,
                        ship_action_completed_tx,
                        db_blackboard,
                        mm,
                        behaviors.trading_behavior,
                    )
                    .instrument(ship_span)
                    .await
                }
            }
        }
    });

    tokio::join!(message_listeners_join_handle, ship_behavior_join_handle, waypoint_observation_join_handle);
    Ok(())
}

async fn trade_forever(
    authenticated_client: Arc<dyn StClientTrait>,
    ship: Ship,
    ship_updated_tx: Sender<ShipOperations>,
    ship_action_completed_tx: Sender<ActionEvent>,
    db_blackboard: Arc<dyn BlackboardOps>,
    mm: DbModelManager,
    trading_behavior: Behavior<ShipAction>,
) -> Result<()> {
    let ship_symbol = ship.symbol.clone();
    let mut ship_op = ShipOperations::new(ship, Arc::clone(&authenticated_client));
    let behavior = trading_behavior.clone();
    let behavior_args = BehaviorArgs {
        blackboard: Arc::clone(&db_blackboard),
    };

    while let Some(trading_ticket) =
        find_best_trade(&mm, &ship_op.nav.system_symbol, &ship_op.ship, Arc::clone(&authenticated_client)).await.expect("ticket").map(create_ticket)
    {
        TradeBmc::upsert_ticket(
            &Ctx::Anonymous,
            &mm,
            &ship_symbol,
            &trading_ticket.ticket_id(),
            &trading_ticket,
            trading_ticket.is_complete(),
        )
        .await?;

        event!(Level::INFO, "Found best trade: {:?}", &trading_ticket);
        ship_op.set_trade_ticket(trading_ticket);
        let _ = behavior
            .run(
                &behavior_args,
                &mut ship_op,
                Duration::from_secs(10),
                &ship_updated_tx,
                &ship_action_completed_tx,
            )
            .await
            .expect("behavior");
    }

    Ok(())
}

async fn explore_waypoints_once(
    authenticated_client: Arc<dyn StClientTrait>,
    ship: Ship,
    ship_updated_tx: Sender<ShipOperations>,
    ship_action_completed_tx: Sender<ActionEvent>,
    db_blackboard: Arc<dyn BlackboardOps>,
    mm: DbModelManager,
    explorer_behavior: Behavior<ShipAction>,
) -> Result<()> {
    let mut ship_op = ShipOperations::new(ship.clone(), Arc::clone(&authenticated_client));
    let behavior_args = BehaviorArgs {
        blackboard: Arc::clone(&db_blackboard),
    };

    let facts = collect_fleet_decision_facts(&mm, &ship.nav.system_symbol.clone()).await?;
    let marketplaces_to_explore = diff_waypoint_symbols(&facts.marketplaces_of_interest, &facts.marketplaces_with_up_to_date_infos);
    let shipyards_to_explore = diff_waypoint_symbols(&facts.shipyards_of_interest, &facts.shipyards_with_up_to_date_infos);
    let all_locations_of_interest = marketplaces_to_explore.iter().chain(shipyards_to_explore.iter()).unique().cloned().collect_vec();

    ship_op.set_explore_locations(all_locations_of_interest);
    let _ = explorer_behavior
        .run(
            &behavior_args,
            &mut ship_op,
            Duration::from_secs(10),
            &ship_updated_tx,
            &ship_action_completed_tx,
        )
        .await
        .expect("behavior");

    Ok(())
}

async fn find_best_trade(
    mm: &DbModelManager,
    system_symbol: &SystemSymbol,
    ship: &Ship,
    client: Arc<dyn StClientTrait>,
) -> Result<Option<EvaluatedTradingOpportunity>> {
    let waypoints = SystemBmc::get_waypoints_of_system(&Ctx::Anonymous, mm, system_symbol).await?;
    let waypoint_map = waypoints.iter().map(|wp| (wp.symbol.clone(), wp)).collect::<HashMap<_, _>>();

    let latest_market_data = MarketBmc::get_latest_market_data_for_system(&Ctx::Anonymous, mm, system_symbol).await?;
    let market_data = trading::to_trade_goods_with_locations(&latest_market_data);
    let trading_opportunities = find_trading_opportunities(&market_data, &waypoint_map);
    let trading_budget = client.get_agent().await?.data.credits;
    let evaluated_trading_opportunities: Vec<EvaluatedTradingOpportunity> =
        trading::evaluate_trading_opportunities(&vec![ship], &waypoint_map, trading_opportunities, trading_budget);

    let maybe_best_opp = evaluated_trading_opportunities.iter().sorted_by_key(|e| e.profit_per_distance_unit).last();
    Ok(maybe_best_opp.cloned())
}

fn create_ticket(opp: EvaluatedTradingOpportunity) -> TradeTicket {
    TradeTicket::TradeCargo {
        ticket_id: TicketId::new(),
        purchase_completion_status: vec![(PurchaseGoodTicketDetails::from_trading_opportunity(&opp), false)],
        sale_completion_status: vec![(SellGoodTicketDetails::from_trading_opportunity(&opp), false)],
        evaluation_result: vec![opp.clone()],
    }
}

async fn run_message_listeners(
    db_model_manager: DbModelManager,
    ship_updated_rx: Receiver<ShipOperations>,
    ship_action_completed_rx: Receiver<ActionEvent>,
    ship_status_report_rx: Receiver<ShipStatusReport>,
    ship_status_report_tx: Sender<ShipStatusReport>,
) {
    let ship_updated_listener_join_handle =
        tokio::spawn(listen_to_ship_changes_and_persist(db_model_manager.clone(), ship_updated_rx).instrument(tracing::Span::current()));

    let ship_action_update_listener_join_handle =
        tokio::spawn(listen_to_ship_action_update_messages(ship_status_report_tx, ship_action_completed_rx).instrument(tracing::Span::current()));

    let ship_status_report_listener_join_handle = tokio::spawn({
        let mm = db_model_manager.clone();
        listen_to_ship_status_report_messages(mm, ship_status_report_rx).instrument(tracing::Span::current())
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
            ActionEvent::TransactionCompleted(ship, transaction_action_event, ticket) => {
                ship_status_report_tx.send(ShipStatusReport::TransactionCompleted(ship.ship, transaction_action_event, ticket)).await?;
            }
        }
    }

    Ok(())
}

pub async fn listen_to_ship_status_report_messages(db_model_manager: DbModelManager, mut ship_status_report_rx: Receiver<ShipStatusReport>) -> Result<()> {
    event!(Level::INFO, "listen_to_ship_status_report_messages - starting");

    while let Some(msg) = ship_status_report_rx.recv().await {
        match msg {
            ShipStatusReport::ShipActionCompleted(_, _) => {}
            ShipStatusReport::TransactionCompleted(ship, transaction_action_event, trading_ticket) => {
                event!(
                    Level::INFO,
                    message = "Transaction completed",
                    ticket_id = trading_ticket.ticket_id().0.to_string(),
                    transaction_ticket_id = transaction_action_event.transaction_ticket_id().0.to_string(),
                );
                st_core::fleet::trading_manager::TradingManager::log_transaction_completed(
                    Ctx::Anonymous,
                    &db_model_manager,
                    &ship,
                    &transaction_action_event,
                    &trading_ticket,
                )
                .await?;
            }
        }
    }

    Ok(())
}

lazy_static! {
    static ref GUARD: std::sync::Mutex<Option<tracing_appender::non_blocking::WorkerGuard>> = std::sync::Mutex::new(None);
}

fn setup_tracing() {
    // Create a file appender with daily rotation
    let file_appender = RollingFileAppender::new(Rotation::DAILY, "./logs/cli", "spaceTraders-cli.log.ndjson");

    // Create a non-blocking writer for the file appender
    let (non_blocking_appender, guard) = tracing_appender::non_blocking(file_appender);

    // Format for timestamps
    let time_format = format_description::parse("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond]Z").expect("Invalid time format");

    let timer = UtcTime::new(time_format);

    // Create the console layer with colored output
    let console_layer = fmt::layer().with_timer(timer.clone()).with_ansi(true).with_target(true).pretty();

    // Create the JSON file layer
    let file_layer = fmt::layer()
        .with_span_events(fmt::format::FmtSpan::CLOSE) // Only log spans when they close
        .with_timer(timer)
        .with_ansi(false)
        .json()
        .with_current_span(true) // Keep just one current span
        .with_span_list(false) // Don't include the full span list
        .with_writer(non_blocking_appender);

    // Create the filter
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // Register all layers
    Registry::default().with(filter).with(console_layer).with(file_layer).init();

    // Store guard in a static to keep it alive for the program duration
    if let Ok(mut g) = GUARD.lock() {
        *g = Some(guard);
    }
}
