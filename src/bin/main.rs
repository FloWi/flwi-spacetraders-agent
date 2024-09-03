use crate::db::upsert_waypoints_of_system;
use anyhow::{Context, Result};
use chrono::Local;
use clap::Parser;
use flwi_spacetraders_agent::db::{
    insert_market_data, select_latest_marketplace_entry_of_system, select_waypoints_of_system,
    upsert_systems_from_receiver, upsert_waypoints_from_receiver, DbWaypointEntry,
};
use sqlx::{ConnectOptions, Executor, Pool, Postgres};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{event, Level};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use utoipa::OpenApi;

use flwi_spacetraders_agent::api_client::api_model::{
    NavStatus, RegistrationRequest, Ship, Waypoint,
};
use flwi_spacetraders_agent::cli_args;
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
    AgentSymbol, FactionSymbol, LabelledCoordinate, MarketData, SerializableCoordinate,
    SystemSymbol, WaypointInSystemResponseData, WaypointSymbol, WaypointTrait, WaypointTraitSymbol,
};

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

                let _ = collect_all_waypoints_of_home_system(
                    &authenticated_client,
                    &pool,
                    headquarters_system_symbol.clone(),
                )
                .await?;

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

                let marketplace_entries = select_latest_marketplace_entry_of_system(
                    &pool,
                    headquarters_system_symbol.clone(),
                )
                .await?;

                let marketplaces_to_explore =
                    find_marketplaces_for_exploration(marketplace_entries.clone());

                let waypoint_entries_of_home_system =
                    select_waypoints_of_system(&pool, headquarters_system_symbol).await?;

                let waypoints_of_home_system: Vec<_> = waypoint_entries_of_home_system
                    .into_iter()
                    .map(|db| db.entry.0.clone())
                    .collect();

                let command_ship: &mut ShipOperations = my_ships
                    .iter_mut()
                    .find(|s| s.symbol == command_ship_name)
                    .unwrap();

                let current_location = command_ship.nav.waypoint_symbol.clone();

                let exploration_route = generate_exploration_route(
                    &marketplaces_to_explore,
                    &waypoints_of_home_system,
                    &current_location,
                )
                .unwrap();

                let stripped_down_route: Vec<SerializableCoordinate<WaypointSymbol>> =
                    exploration_route
                        .clone()
                        .into_iter()
                        .map(|wp| wp.to_serializable())
                        .collect();

                let json_route = serde_json::to_string(&stripped_down_route)?;
                println!("Explorer Route: \n{}", json_route);

                // compute first hop
                let start = exploration_route.get(0).unwrap();
                let first_stop = &exploration_route.get(1).unwrap();

                if let Some(travel_instructions) = pathfinder::compute_path(
                    start.symbol.clone(),
                    first_stop.symbol.clone(),
                    waypoints_of_home_system.clone(),
                    marketplace_entries
                        .iter()
                        .map(|db| db.entry.0.clone())
                        .collect(),
                    command_ship.ship.clone(),
                ) {
                    println!("Path found");
                    dbg!(travel_instructions);
                } else {
                    println!("No path found");
                };

                match command_ship.nav.status {
                    NavStatus::InTransit => {
                        println!("Ship is in transit")
                    }
                    NavStatus::InOrbit => {
                        println!("Ship is in orbit - docking");
                        command_ship.dock().await?;
                        assert_eq!(command_ship.nav.status, NavStatus::Docked);
                    }
                    NavStatus::Docked => {
                        println!("Ship is docket - orbiting");
                        command_ship.orbit().await?;
                        assert_eq!(command_ship.nav.status, NavStatus::InOrbit);
                    }
                }

                //let my_ships: Vec<_> = my_ships.iter().map(|so| so.get_ship()).collect();
                //dbg!(my_ships);
                Ok(())
            }
        },
    }
}

async fn collect_all_systems(client: &StClient, pool: &Pool<Postgres>) -> Result<()> {
    let (tx, rx) = mpsc::channel(100); // Buffer up to 100 pages

    let producer = fetch_all_pages_into_queue(
        |page| client.list_systems_page(page),
        PaginationInput { page: 1, limit: 20 },
        tx,
    );

    tokio::join!(producer, upsert_systems_from_receiver(pool, rx));
    Ok(())
}

async fn collect_all_waypoints_of_home_system(
    client: &StClient,
    pool: &Pool<Postgres>,
    headquarters_system_symbol: SystemSymbol,
) -> Result<()> {
    let (tx, rx) = mpsc::channel(100); // Buffer up to 100 pages

    let producer = fetch_all_pages_into_queue(
        |page| client.list_waypoints_of_system_page(&headquarters_system_symbol, page),
        PaginationInput { page: 1, limit: 20 },
        tx,
    );

    tokio::join!(producer, upsert_waypoints_from_receiver(pool, rx));
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
