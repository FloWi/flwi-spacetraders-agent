use crate::db::upsert_waypoints_of_system;
use crate::db::DbSystemCoordinateData;
use anyhow::{Context, Result};
use chrono::Local;
use clap::Parser;
use flwi_spacetraders_agent::behavior_tree::behavior_tree::Actionable;
use flwi_spacetraders_agent::db::{
    insert_market_data, select_latest_marketplace_entry_of_system, select_waypoints_of_system,
    upsert_systems_from_receiver, upsert_waypoints_from_receiver, DbWaypointEntry,
};
use futures::StreamExt;
use itertools::Itertools;
use serde_json::json;
use sqlx::types::JsonValue;
use sqlx::{ConnectOptions, Executor, Pool, Postgres};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{event, span, Level};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use utoipa::OpenApi;

use flwi_spacetraders_agent::behavior_tree::behavior_tree::Behavior;
use flwi_spacetraders_agent::behavior_tree::ship_behaviors::{
    ship_navigation_behaviors, ShipAction,
};
use flwi_spacetraders_agent::cli_args::{Cli, Commands};
use flwi_spacetraders_agent::configuration::AgentConfiguration;
use flwi_spacetraders_agent::db;
use flwi_spacetraders_agent::exploration::exploration::generate_exploration_route;
use flwi_spacetraders_agent::marketplaces::marketplaces::find_marketplaces_for_exploration;
use flwi_spacetraders_agent::pagination::{
    collect_results, fetch_all_pages, fetch_all_pages_into_queue, PaginatedResponse,
    PaginationInput,
};
use flwi_spacetraders_agent::pathfinder::pathfinder;
use flwi_spacetraders_agent::reqwest_helpers::create_client;
use flwi_spacetraders_agent::ship::{MyShip, ShipOperations};
use flwi_spacetraders_agent::st_client::StClient;
use flwi_spacetraders_agent::st_model::{
    AgentSymbol, FactionSymbol, LabelledCoordinate, MarketData, NavStatus, RegistrationRequest,
    SerializableCoordinate, Ship, SystemSymbol, Waypoint, WaypointInSystemResponseData,
    WaypointSymbol, WaypointTrait, WaypointTraitSymbol,
};
use flwi_spacetraders_agent::{cli_args, format_time_delta_hh_mm_ss};

#[tokio::main]
async fn main() -> Result<()> {
    let args = cli_args::Cli::parse();

    match args {
        Cli { command } => match command.clone() {
            Commands::RunAgent {
                database_url,
                spacetraders_agent_faction,
                spacetraders_agent_symbol,
                spacetraders_registration_email,
            } => {
                tracing_subscriber::registry()
                    .with(fmt::layer().with_span_events(fmt::format::FmtSpan::CLOSE))
                    .with(EnvFilter::from_default_env())
                    .init();

                let cfg = AgentConfiguration::new(command.clone());

                let reqwest_client_with_middleware = create_client(None);

                let unauthenticated_client = StClient::new(reqwest_client_with_middleware);

                let status = unauthenticated_client.get_status().await?;

                let pool = db::prepare_database_schema(&status, cfg.clone()).await?;

                let authenticated_client = get_authenticated_client(
                    &pool,
                    unauthenticated_client,
                    spacetraders_agent_faction,
                    spacetraders_agent_symbol.clone(),
                    spacetraders_registration_email,
                )
                .await?;

                let my_agent = authenticated_client.get_agent().await?;
                dbg!(my_agent.clone());

                let headquarters_waypoint_symbol =
                    WaypointSymbol(my_agent.data.headquarters.clone());
                let headquarters_system_symbol = headquarters_waypoint_symbol.system_symbol();

                let now = Local::now().to_utc();

                let number_systems_in_db = db::select_count_of_systems(&pool).await?;

                let need_collect_systems = status.stats.systems as i64 != number_systems_in_db;

                if need_collect_systems {
                    event!(
                        Level::INFO,
                        "Not all {} systems are in database. Currently stored: {}",
                        status.stats.systems,
                        number_systems_in_db,
                    );

                    let _ = collect_all_systems(&authenticated_client, &pool).await?;
                } else {
                    event!(
                        Level::INFO,
                        "No need to collect systems - all {} systems are already in db",
                        number_systems_in_db
                    );
                }

                let systems_with_waypoint_details_to_be_loaded: Vec<DbSystemCoordinateData> =
                    db::select_systems_with_waypoint_details_to_be_loaded(&pool).await?;

                // let marketplaces_of_system = db::select_waypoints_of_system_with_trait(
                //     &pool,
                //     headquarters_system_symbol.clone(),
                //     WaypointTraitSymbol("MARKETPLACE".to_string()),
                // )
                // .await?;

                //TODO: only check far-away marketplaces once

                // let market_data: Vec<_> =
                //     collect_results(marketplaces_of_system.clone(), |waypoint_symbol| {
                //         authenticated_client.get_marketplace(waypoint_symbol)
                //     })
                //     .await?
                //     .iter()
                //     .map(|md| md.data.clone())
                //     .collect();
                //
                // let _ = insert_market_data(&pool, market_data.clone(), now).await;

                let ships = collect_all_ships(&authenticated_client).await?;
                let client = Arc::new(authenticated_client);

                let mut my_ships: Vec<_> = ships
                    .iter()
                    .map(|s| ShipOperations::new(MyShip::new(s.clone()), Arc::clone(&client)))
                    .collect();

                let command_ship_name = spacetraders_agent_symbol + "-1";

                let marketplace_entries =
                    select_latest_marketplace_entry_of_system(&pool, &headquarters_system_symbol)
                        .await?;

                let marketplaces_to_explore =
                    find_marketplaces_for_exploration(marketplace_entries.clone());

                let waypoint_entries_of_home_system =
                    select_waypoints_of_system(&pool, &headquarters_system_symbol).await?;

                let waypoints_of_home_system: Vec<_> = waypoint_entries_of_home_system
                    .into_iter()
                    .map(|db| db.entry.0.clone())
                    .collect();

                let command_ship_index = my_ships
                    .iter()
                    .position(|s| s.symbol == command_ship_name)
                    .unwrap();

                let mut command_ship = my_ships.remove(command_ship_index);
                let current_location = command_ship.nav.waypoint_symbol.clone();

                let exploration_route = generate_exploration_route(
                    &marketplaces_to_explore,
                    &waypoints_of_home_system,
                    &current_location,
                )
                .unwrap();

                let mut route_debugging_list: Vec<JsonValue> = Vec::new();

                exploration_route
                    .iter()
                    .tuple_windows()
                    .for_each(|(from, to)| {
                        let mut debug_info: HashMap<&str, JsonValue> = HashMap::new();
                        debug_info.insert("from", json!(from.symbol));
                        debug_info.insert("to", json!(to.symbol));

                        if let Some(travel_instructions) = pathfinder::compute_path(
                            from.symbol.clone(),
                            to.symbol.clone(),
                            waypoints_of_home_system.clone(),
                            marketplace_entries
                                .iter()
                                .map(|db| db.entry.0.clone())
                                .collect(),
                            command_ship.ship.ship.clone(),
                        ) {
                            debug_info.insert("actions", json!(&travel_instructions));

                            println!("Path found from {} to {}", from.symbol.0, to.symbol.0);

                            let (final_location, total_time) =
                                travel_instructions.last().unwrap().waypoint_and_time();
                            assert_eq!(final_location, &to.symbol);
                        } else {
                            println!("No path found from {} to {}", from.symbol.0, to.symbol.0);
                        };
                        route_debugging_list.push(json!(debug_info));
                    });

                let stripped_down_route: Vec<SerializableCoordinate<WaypointSymbol>> =
                    exploration_route
                        .clone()
                        .into_iter()
                        .map(|wp| wp.to_serializable())
                        .collect();

                let json_route = serde_json::to_string(&stripped_down_route)?;
                println!("Explorer Route: \n{}", json_route);
                println!(
                    "Detailed Routes with actions: \n{}",
                    serde_json::to_string(&route_debugging_list)?
                );
                let to = exploration_route.get(1).unwrap();
                let path = pathfinder::compute_path(
                    command_ship.nav.waypoint_symbol.clone(),
                    to.symbol.clone(),
                    waypoints_of_home_system.clone(),
                    marketplace_entries
                        .iter()
                        .map(|db| db.entry.0.clone())
                        .collect(),
                    command_ship.ship.ship.clone(),
                )
                .unwrap();
                command_ship.set_route(path);

                let blackboard: HashMap<String, String> = HashMap::new();
                let _ = tokio::spawn(ship_loop(command_ship)).await?;

                //let my_ships: Vec<_> = my_ships.iter().map(|so| so.get_ship()).collect();
                //dbg!(my_ships);
                Ok(())
            }
        },
    }
}

pub async fn ship_loop(mut ship: ShipOperations) -> Result<()> {
    let behaviors = ship_navigation_behaviors();
    let ship_behavior = behaviors.ship_behavior;

    println!(
        "Running behavior tree. \n<mermaid>\n{}\n</mermaid>",
        ship_behavior.to_mermaid()
    );

    // let mut ship_behavior_tree = BT::new(behaviors.travel_behavior, blackboard);
    //
    // let mut timer = Timer::init_time();
    //
    loop {
        let span = span!(Level::INFO, "ship_loop", ship = format!("{}", ship.symbol),);

        let _enter = span.enter();

        let result = ship_behavior.run(&(), &mut ship).await;

        event!(Level::INFO, "Ship tick done - result: {:?}", result);

        sleep(Duration::from_secs(1)).await;
    }
    Ok(())
}

async fn collect_all_systems(client: &StClient, pool: &Pool<Postgres>) -> Result<()> {
    let (tx, rx) = mpsc::channel(100); // Buffer up to 100 pages

    let producer = fetch_all_pages_into_queue(
        |page| client.list_systems_page(page),
        PaginationInput { page: 1, limit: 20 },
        tx,
    );

    tokio::try_join!(producer, upsert_systems_from_receiver(pool, rx))?;
    Ok(())
}

async fn collect_waypoints_for_systems(
    client: &StClient,
    systems: &[DbSystemCoordinateData],
    home_system: &SystemSymbol,
    pool: &Pool<Postgres>,
) -> Result<()> {
    let home_system = db::select_system_with_coordinate(pool, home_system)
        .await?
        .unwrap();

    // sort systems by the distance from our system
    event!(
        Level::INFO,
        "Collecting missing waypoints for {} systems in order of distance from home-system {}",
        systems.len(),
        home_system.system_symbol
    );

    let start_timestamp = Local::now();

    let sorted = systems.iter().sorted_by_key(|s| home_system.distance_to(s));

    for (idx, system) in sorted.enumerate() {
        let now = Local::now();

        let duration = now - start_timestamp;
        let download_speed = idx as f32 / duration.num_seconds() as f32; // systems per second
        let number_elements_left = systems.len() - idx;
        let estimated_rest_duration =
            chrono::Duration::seconds((number_elements_left as f32 / download_speed) as i64);

        let estimated_finish_ts = now + estimated_rest_duration;

        let download_speed_info = format!(
            "avg {:.1} systems/s; estimated duration: {}; estimated completion: {}",
            download_speed,
            format_time_delta_hh_mm_ss(estimated_rest_duration),
            estimated_finish_ts
        );

        event!(
            Level::INFO,
            "Downloading waypoints for system {} ({} of {} systems) {}",
            system.system_symbol,
            idx + 1,
            systems.len(),
            download_speed_info
        );

        collect_waypoints_of_system(client, pool, SystemSymbol(system.system_symbol.clone()))
            .await?
    }

    Ok(())
}

async fn collect_waypoints_of_system(
    client: &StClient,
    pool: &Pool<Postgres>,
    system_symbol: SystemSymbol,
) -> Result<()> {
    let (tx, rx) = mpsc::channel(100); // Buffer up to 100 pages

    let producer = fetch_all_pages_into_queue(
        |page| client.list_waypoints_of_system_page(&system_symbol, page),
        PaginationInput { page: 1, limit: 20 },
        tx,
    );

    tokio::try_join!(producer, upsert_waypoints_from_receiver(pool, rx))?;
    Ok(())
}

async fn collect_all_ships(client: &StClient) -> Result<Vec<Ship>> {
    let ships: Vec<Ship> = fetch_all_pages(|p| client.list_ships(p)).await?;

    Ok(ships)
}

async fn get_authenticated_client(
    pool: &Pool<Postgres>,
    unauthenticated_client: StClient,
    spacetraders_agent_faction: String,
    spacetraders_agent_symbol: String,
    spacetraders_registration_email: String,
) -> Result<StClient> {
    event!(Level::INFO, "Trying to load registration from database",);

    let maybe_existing_reqistration = db::load_registration(pool).await?;

    match maybe_existing_reqistration {
        Some(db_entry) => {
            event!(
                Level::INFO,
                "Found registration infos in database. Creating authenticated client",
            );

            Ok(StClient::new(create_client(Some(db_entry.token))))
        }
        None => {
            event!(
                Level::INFO,
                "No registration infos found in database. Registering new agent",
            );

            let registration_response = unauthenticated_client
                .register(RegistrationRequest {
                    faction: FactionSymbol(spacetraders_agent_faction),
                    symbol: spacetraders_agent_symbol,
                    email: spacetraders_registration_email,
                })
                .await
                .expect("Error during registration");

            event!(
                Level::INFO,
                "Registration complete: {:?}",
                registration_response
            );

            let _ = db::save_registration(pool, registration_response.clone()).await;

            Ok(StClient::new(create_client(Some(
                registration_response.clone().data.token,
            ))))
        }
    }
}
