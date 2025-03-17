use crate::behavior_tree::behavior_tree::Actionable;
use st_store::{
    db, select_latest_marketplace_entry_of_system, select_waypoints_of_system,
    upsert_systems_from_receiver, upsert_waypoints_from_receiver, DbModelManager,
    DbSystemCoordinateData,
};

use anyhow::Result;
use chrono::{Local, Utc};
use futures::StreamExt;
use itertools::Itertools;
use serde_json::json;
use sqlx::types::JsonValue;
use sqlx::{Pool, Postgres};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{event, span, Level};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::behavior_tree::behavior_args::{BehaviorArgs, DbBlackboard};
use crate::behavior_tree::ship_behaviors::ship_navigation_behaviors;
use crate::configuration::AgentConfiguration;
use crate::exploration::exploration::generate_exploration_route;
use crate::format_time_delta_hh_mm_ss;
use crate::marketplaces::marketplaces::{
    find_marketplaces_for_exploration, find_marketplaces_to_collect_remotely,
};
use crate::pagination::{fetch_all_pages, fetch_all_pages_into_queue, PaginationInput};
use crate::pathfinder::pathfinder;
use crate::reqwest_helpers::create_client;
use crate::ship::ShipOperations;
use crate::st_client::{StClient, StClientTrait};
use st_domain::{
    FactionSymbol, LabelledCoordinate, RegistrationRequest, SerializableCoordinate, Ship,
    ShipSymbol, StStatusResponse, SystemSymbol, WaypointSymbol, WaypointType,
};

pub async fn run_agent(
    cfg: AgentConfiguration,
    status: StStatusResponse,
    authenticated_client: StClient,
    pool: Pool<Postgres>,
) -> Result<()> {
    let my_agent = authenticated_client.get_agent().await?;
    dbg!(my_agent.clone());

    let headquarters_waypoint_symbol = WaypointSymbol(my_agent.data.headquarters.clone());
    let headquarters_system_symbol = headquarters_waypoint_symbol.system_symbol();

    let now = Local::now().to_utc();

    let ships = collect_all_ships(&authenticated_client).await?;
    let _ = db::upsert_ships(&pool, &ships, now).await?;

    load_home_system_and_waypoints_if_necessary(
        &authenticated_client,
        &pool,
        &headquarters_system_symbol,
    )
    .await?;

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

    let marketplace_entries =
        select_latest_marketplace_entry_of_system(&pool, &headquarters_system_symbol).await?;

    let waypoint_entries_of_home_system =
        select_waypoints_of_system(&pool, &headquarters_system_symbol).await?;

    let marketplaces_to_collect_remotely = find_marketplaces_to_collect_remotely(
        marketplace_entries.clone(),
        &waypoint_entries_of_home_system,
    );

    let _ = collect_marketplaces(
        &authenticated_client,
        &marketplaces_to_collect_remotely,
        &pool,
    )
    .await?;

    let client: Arc<dyn StClientTrait> = Arc::new(authenticated_client);

    let mut my_ships: Vec<_> = ships
        .iter()
        .map(|s| ShipOperations::new(s.clone(), Arc::clone(&client)))
        .collect();

    let command_ship_name = ShipSymbol(cfg.spacetraders_agent_symbol + "-1");

    let marketplace_entries =
        select_latest_marketplace_entry_of_system(&pool, &headquarters_system_symbol).await?;

    let marketplaces_to_explore = find_marketplaces_for_exploration(marketplace_entries.clone());

    let waypoints_of_home_system = waypoint_entries_of_home_system
        .into_iter()
        .map(|db| db.entry.0.clone())
        .collect_vec();

    let jump_gate_wp_of_home_system = waypoints_of_home_system
        .iter()
        .find(|wp| wp.r#type == WaypointType::JUMP_GATE)
        .expect("home system should have a jump-gate");
    let construction_site = client
        .get_construction_site(&jump_gate_wp_of_home_system.symbol)
        .await?;

    let _ = db::upsert_construction_site(&pool, construction_site, now).await?;

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
    .unwrap_or(Vec::new());

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
                command_ship.ship.engine.speed as u32,
                command_ship.ship.fuel.current as u32,
                command_ship.ship.fuel.capacity as u32,
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

    let stripped_down_route: Vec<SerializableCoordinate<WaypointSymbol>> = exploration_route
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
    command_ship.set_explore_locations(exploration_route);

    let args = BehaviorArgs {
        blackboard: Arc::new(DbBlackboard { db: pool.clone() }),
    };

    let (ship_updated_tx, mut ship_updated_rx): (Sender<ShipOperations>, Receiver<ShipOperations>) =
        mpsc::channel::<ShipOperations>(32);

    let _ = tokio::spawn(ship_loop(command_ship, args, ship_updated_tx));

    // everything has to be cloned to give ownership to the spawned task
    let _ = tokio::spawn({
        let client_clone = client.clone();
        let pool_clone = pool.clone();
        let hq_system_clone = headquarters_system_symbol.clone();
        let status_clone = status.clone();

        async move {
            if let Err(e) = load_systems_and_waypoints_if_necessary(
                status_clone,
                &*client_clone,
                &pool_clone,
                &hq_system_clone,
            ).await {
                eprintln!("Error loading systems: {}", e);
            }
        }
    });

    let _ = tokio::spawn(listen_to_ship_changes_and_persist(
        ship_updated_rx,
        pool.clone(),
    ));

    //let my_ships: Vec<_> = my_ships.iter().map(|so| so.get_ship()).collect();
    //dbg!(my_ships);
    Ok(())
}

async fn load_home_system_and_waypoints_if_necessary(
    client: &StClient,
    pool: &Pool<Postgres>,
    headquarters_system_symbol: &SystemSymbol,
) -> Result<()> {
    let maybe_home_system = db::select_system(pool, headquarters_system_symbol).await?;

    let (needs_load_system, needs_load_waypoints) = match maybe_home_system {
        None => (true, true),
        Some(home_system) => {
            let waypoints_of_home_system =
                db::select_waypoints_of_system(pool, headquarters_system_symbol).await?;
            (
                false,
                home_system.waypoints.len() > waypoints_of_home_system.len(),
            )
        }
    };

    let now = Utc::now();

    if needs_load_system {
        let system = client.get_system(headquarters_system_symbol).await?;
        let _ = db::upsert_systems_page(pool, vec![system], now).await?;
    }

    if needs_load_waypoints {
        let _ =
            collect_waypoints_of_system(client, pool, headquarters_system_symbol.clone()).await?;
    }
    Ok(())
}

async fn load_systems_and_waypoints_if_necessary(
    status: StStatusResponse,
    authenticated_client: &dyn StClientTrait,
    pool: &Pool<Postgres>,
    headquarters_system_symbol: &SystemSymbol,
) -> Result<()> {
    let number_systems_in_db = db::select_count_of_systems(&pool).await?;

    let need_collect_systems = status.stats.systems as i64 != number_systems_in_db;

    if need_collect_systems {
        event!(
            Level::INFO,
            "Not all {} systems are in database. Currently stored: {}",
            status.stats.systems,
            number_systems_in_db,
        );

        collect_all_systems(authenticated_client, &pool).await?;
    } else {
        event!(
            Level::INFO,
            "No need to collect systems - all {} systems are already in db",
            number_systems_in_db
        );
    }

    let systems_with_waypoint_details_to_be_loaded: Vec<DbSystemCoordinateData> =
        db::select_systems_with_waypoint_details_to_be_loaded(&pool).await?;

    let number_of_systems_with_missing_waypoint_infos =
        systems_with_waypoint_details_to_be_loaded.len();
    let need_collect_waypoints_of_systems = number_of_systems_with_missing_waypoint_infos > 0;
    if need_collect_waypoints_of_systems {
        event!(
            Level::INFO,
            "Not all waypoints are stored in database. Need to update {} of {} systems",
            number_of_systems_with_missing_waypoint_infos,
            status.stats.systems,
        );

        collect_waypoints_for_systems(
            authenticated_client,
            &systems_with_waypoint_details_to_be_loaded,
            &headquarters_system_symbol,
            &pool,
        )
        .await?;
    } else {
        event!(
                        Level::INFO,
                        "No need to collect waypoints for systems - all {} systems have detailed waypoint infos",
                        number_systems_in_db
                    );
    }
    Ok(())
}

pub async fn listen_to_ship_changes_and_persist(
    mut ship_updated_rx: Receiver<ShipOperations>,
    pool: Pool<Postgres>,
) -> Result<()> {
    let mut old_ship_state: Option<ShipOperations> = None;

    while let Some(updated_ship) = ship_updated_rx.recv().await {
        match old_ship_state {
            Some(old_ship_ops) if old_ship_ops.ship == updated_ship.ship => {
                // no need to update
                event!(
                    Level::INFO,
                    "No need to update ship {}. No change detected",
                    updated_ship.symbol.0
                );
            }
            _ => {
                event!(Level::INFO, "Ship {} updated", updated_ship.symbol.0);
                let _ =
                    db::upsert_ships(&pool, &vec![updated_ship.ship.clone()], Utc::now()).await?;
            }
        }

        old_ship_state = Some(updated_ship.clone());
    }

    Ok(())
}

pub async fn ship_loop(
    mut ship: ShipOperations,
    args: BehaviorArgs,
    ship_updated_tx: Sender<ShipOperations>,
) -> Result<()> {
    let behaviors = ship_navigation_behaviors();
    let ship_behavior = behaviors.explorer_behavior;

    println!(
        "Running behavior tree. \n<mermaid>\n{}\n</mermaid>",
        ship_behavior.to_mermaid()
    );

    let mut tick: usize = 0;
    let span = span!(
        Level::INFO,
        "ship_loop",
        tick,
        ship = format!("{}", ship.symbol.0),
    );
    tick += 1;

    let _enter = span.enter();

    let result = ship_behavior
        .run(&args, &mut ship, Duration::from_secs(1), &ship_updated_tx)
        .await;

    match &result {
        Ok(o) => {
            event!(
                name: "Ship Tick done ",
                Level::INFO,
                result = %o,
            );
        }
        Err(e) => {
            event!(
                name: "Ship Tick done with Error",
                Level::INFO,
                result = %e,
            );
        }
    }

    event!(Level::INFO, "Ship Loop done",);

    Ok(())
}

async fn collect_all_systems(client: &dyn StClientTrait, pool: &Pool<Postgres>) -> Result<()> {
    let (tx, rx) = mpsc::channel(100); // Buffer up to 100 pages

    let producer = fetch_all_pages_into_queue(
        |page| client.list_systems_page(page),
        PaginationInput { page: 1, limit: 20 },
        tx,
    );

    tokio::try_join!(producer, upsert_systems_from_receiver(pool, rx))?;
    Ok(())
}

async fn collect_marketplaces(
    client: &StClient,
    marketplace_waypoint_symbols: &[WaypointSymbol],
    pool: &Pool<Postgres>,
) -> Result<()> {
    event!(
        Level::INFO,
        "Collecting marketplace infos (remotely) for {} waypoint_symbols",
        marketplace_waypoint_symbols.len()
    );

    for wp in marketplace_waypoint_symbols {
        let marketplace = client.get_marketplace(wp.clone()).await?;
        db::insert_market_data(pool, vec![marketplace.data], Utc::now()).await?;
    }
    Ok(())
}

async fn collect_waypoints_for_systems(
    client: &dyn StClientTrait,
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
    client: &dyn StClientTrait,
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
