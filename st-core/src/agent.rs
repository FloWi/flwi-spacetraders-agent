use crate::behavior_tree::behavior_tree::Actionable;
use st_store::{
    db, select_latest_marketplace_entry_of_system, select_latest_shipyard_entry_of_system, select_waypoints_of_system, upsert_systems_from_receiver,
    upsert_waypoints_from_receiver, Ctx, DbModelManager, DbSystemCoordinateData, DbWaypointEntry,
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
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::{mpsc, Mutex};
use tracing::{event, span, Level};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::behavior_tree::behavior_args::{BehaviorArgs, DbBlackboard};
use crate::behavior_tree::ship_behaviors::ship_behaviors;
use crate::configuration::AgentConfiguration;
use crate::exploration::exploration::generate_exploration_route;
use crate::fleet::fleet::FleetAdmiral;
use crate::format_time_delta_hh_mm_ss;
use crate::marketplaces::marketplaces::{find_marketplaces_for_exploration, find_marketplaces_to_collect_remotely, find_shipyards_to_collect_remotely};
use crate::pagination::{fetch_all_pages, fetch_all_pages_into_queue, PaginationInput};
use crate::pathfinder::pathfinder;
use crate::reqwest_helpers::create_client;
use crate::ship::ShipOperations;
use crate::st_client::{StClient, StClientTrait};
use st_domain::blackboard_ops::BlackboardOps;
use st_domain::{
    FactionSymbol, LabelledCoordinate, RegistrationRequest, SerializableCoordinate, Ship, ShipSymbol, StStatusResponse, SupplyChain, SystemSymbol, Waypoint,
    WaypointSymbol, WaypointType,
};
use st_store::bmc::{Bmc, DbBmc};

pub async fn run_agent(cfg: AgentConfiguration, status: StStatusResponse, authenticated_client: StClient, pool: Pool<Postgres>) -> Result<()> {
    let my_agent = authenticated_client.get_agent().await?;
    dbg!(my_agent.clone());

    let model_manager = DbModelManager::new(pool.clone());

    let db_bmc = DbBmc::new(model_manager);
    let db_blackboard = DbBlackboard { bmc: db_bmc.clone() };

    let bmc = Arc::new(db_bmc) as Arc<dyn Bmc>;
    let blackboard = Arc::new(db_blackboard) as Arc<dyn BlackboardOps>;

    let headquarters_waypoint_symbol = my_agent.data.headquarters.clone();
    let headquarters_system_symbol = headquarters_waypoint_symbol.system_symbol();

    let now = Local::now().to_utc();

    //let _ = db::upsert_ships(&pool, &ships, now).await?;

    load_home_system_and_waypoints_if_necessary(&authenticated_client, &pool, &headquarters_system_symbol).await?;

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

    let marketplace_entries = select_latest_marketplace_entry_of_system(&pool, &headquarters_system_symbol).await?;

    let shipyard_entries = select_latest_shipyard_entry_of_system(&pool, &headquarters_system_symbol).await?;

    let waypoint_entries_of_home_system = select_waypoints_of_system(&pool, &headquarters_system_symbol).await?;

    let marketplaces_to_collect_remotely = find_marketplaces_to_collect_remotely(marketplace_entries.clone(), &waypoint_entries_of_home_system);

    let shipyards_to_collect_remotely = find_shipyards_to_collect_remotely(shipyard_entries.clone(), &waypoint_entries_of_home_system);

    let _ = collect_marketplaces(&authenticated_client, &marketplaces_to_collect_remotely, &pool).await?;

    let _ = collect_shipyards(&authenticated_client, &shipyards_to_collect_remotely, &pool).await?;

    let client: Arc<dyn StClientTrait> = Arc::new(authenticated_client);

    let jump_gate_wp_of_home_system =
        waypoint_entries_of_home_system.iter().find(|wp| wp.r#type == WaypointType::JUMP_GATE).expect("home system should have a jump-gate");

    let construction_site = client.get_construction_site(&jump_gate_wp_of_home_system.symbol).await?;

    let _ = db::upsert_construction_site(&pool, construction_site, now).await?;

    let _ = match db::load_supply_chain(&pool).await? {
        None => {
            let supply_chain: SupplyChain = client.get_supply_chain().await?.into();
            let _ = db::insert_supply_chain(&pool, supply_chain, Utc::now()).await?;
        }
        Some(_) => {}
    };

    let (ship_updated_tx, mut ship_updated_rx): (Sender<ShipOperations>, Receiver<ShipOperations>) = mpsc::channel::<ShipOperations>(32);

    // everything has to be cloned to give ownership to the spawned task
    let _ = tokio::spawn({
        let client_clone = client.clone();
        let hq_system_clone = headquarters_system_symbol.clone();
        let waypoint_entries_of_home_system_clone = waypoint_entries_of_home_system.clone();
        let ship_updated_tx_clone = ship_updated_tx.clone();
        let admiral = Arc::new(Mutex::new(
            FleetAdmiral::load_or_create(Arc::clone(&bmc), hq_system_clone, Arc::clone(&client_clone)).await?,
        ));

        async move {
            if let Err(e) = FleetAdmiral::run_fleets(Arc::clone(&admiral), Arc::clone(&client_clone), Arc::clone(&bmc), Arc::clone(&blackboard)).await {
                eprintln!("Error on FleetAdmiral::start_fleets: {}", e);
            }
        }
    });

    // everything has to be cloned to give ownership to the spawned task
    let _ = tokio::spawn({
        let client_clone = client.clone();
        let pool_clone = pool.clone();
        let hq_system_clone = headquarters_system_symbol.clone();
        let status_clone = status.clone();

        async move {
            if let Err(e) = load_systems_and_waypoints_if_necessary(status_clone, &*client_clone, &pool_clone, &hq_system_clone).await {
                eprintln!("Error loading systems: {}", e);
            }
        }
    });

    // let _ = tokio::spawn(listen_to_ship_changes_and_persist(ship_updated_rx, pool.clone()));

    //let my_ships: Vec<_> = my_ships.iter().map(|so| so.get_ship()).collect();
    //dbg!(my_ships);
    Ok(())
}

async fn load_home_system_and_waypoints_if_necessary(client: &StClient, pool: &Pool<Postgres>, headquarters_system_symbol: &SystemSymbol) -> Result<()> {
    let maybe_home_system = db::select_system(pool, headquarters_system_symbol).await?;

    let (needs_load_system, needs_load_waypoints) = match maybe_home_system {
        None => (true, true),
        Some(home_system) => {
            let waypoints_of_home_system = db::select_waypoints_of_system(pool, headquarters_system_symbol).await?;
            (false, home_system.waypoints.len() > waypoints_of_home_system.len())
        }
    };

    let now = Utc::now();

    if needs_load_system {
        let system = client.get_system(headquarters_system_symbol).await?.data;
        let _ = db::upsert_systems_page(pool, vec![system], now).await?;
    }

    if needs_load_waypoints {
        let _ = collect_waypoints_of_system(client, pool, headquarters_system_symbol.clone()).await?;
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

    let systems_with_waypoint_details_to_be_loaded: Vec<DbSystemCoordinateData> = db::select_systems_with_waypoint_details_to_be_loaded(&pool).await?;

    let number_of_systems_with_missing_waypoint_infos = systems_with_waypoint_details_to_be_loaded.len();
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

async fn collect_all_systems(client: &dyn StClientTrait, pool: &Pool<Postgres>) -> Result<()> {
    let (tx, rx) = mpsc::channel(100); // Buffer up to 100 pages

    let producer = fetch_all_pages_into_queue(|page| client.list_systems_page(page), PaginationInput { page: 1, limit: 20 }, tx);

    tokio::try_join!(producer, upsert_systems_from_receiver(pool, rx))?;
    Ok(())
}

async fn collect_marketplaces(client: &StClient, marketplace_waypoint_symbols: &[WaypointSymbol], pool: &Pool<Postgres>) -> Result<()> {
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

async fn collect_shipyards(client: &StClient, shipyard_waypoint_symbols: &[WaypointSymbol], pool: &Pool<Postgres>) -> Result<()> {
    event!(
        Level::INFO,
        "Collecting shipyard infos (remotely) for {} waypoint_symbols",
        shipyard_waypoint_symbols.len()
    );

    for wp in shipyard_waypoint_symbols {
        let shipyard = client.get_shipyard(wp.clone()).await?;
        db::insert_shipyards(pool, vec![shipyard.data], Utc::now()).await?;
    }
    Ok(())
}

async fn collect_waypoints_for_systems(
    client: &dyn StClientTrait,
    systems: &[DbSystemCoordinateData],
    home_system: &SystemSymbol,
    pool: &Pool<Postgres>,
) -> Result<()> {
    let home_system = db::select_system_with_coordinate(pool, home_system).await?.unwrap();

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
        let estimated_rest_duration = chrono::Duration::seconds((number_elements_left as f32 / download_speed) as i64);

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

        collect_waypoints_of_system(client, pool, SystemSymbol(system.system_symbol.clone())).await?
    }

    Ok(())
}

async fn collect_waypoints_of_system(client: &dyn StClientTrait, pool: &Pool<Postgres>, system_symbol: SystemSymbol) -> Result<()> {
    let (tx, rx) = mpsc::channel(100); // Buffer up to 100 pages

    let producer = fetch_all_pages_into_queue(
        |page| client.list_waypoints_of_system_page(&system_symbol, page),
        PaginationInput { page: 1, limit: 20 },
        tx,
    );

    tokio::try_join!(producer, upsert_waypoints_from_receiver(pool, rx))?;
    Ok(())
}
