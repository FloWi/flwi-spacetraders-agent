use st_store::{db, upsert_systems_from_receiver, upsert_waypoints_from_receiver, DbSystemCoordinateData};

use anyhow::Result;
use chrono::{Local, Utc};
use itertools::Itertools;
use sqlx::{Pool, Postgres};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{event, Level};

use crate::fleet::fleet::FleetAdmiral;
use crate::format_time_delta_hh_mm_ss;
use crate::pagination::{fetch_all_pages_into_queue, PaginationInput};
use crate::st_client::{StClient, StClientTrait};
use crate::transfer_cargo_manager::TransferCargoManager;
use st_domain::{StStatusResponse, SystemSymbol, WaypointSymbol};
use st_store::bmc::Bmc;

pub async fn run_agent(client: Arc<dyn StClientTrait>, bmc: Arc<dyn Bmc>, transfer_cargo_manager: Arc<TransferCargoManager>) -> Result<()> {
    let headquarters_system_symbol = client.get_agent().await?.data.headquarters.system_symbol();

    // everything has to be cloned to give ownership to the spawned task
    let _running = tokio::spawn({
        let client_clone = client.clone();
        let hq_system_clone = headquarters_system_symbol.clone();
        let (admiral, treasurer_archiver_join_handle) = FleetAdmiral::load_or_create(Arc::clone(&bmc), hq_system_clone, Arc::clone(&client_clone)).await?;

        let admiral = Arc::new(Mutex::new(admiral));

        async move {
            if let Err(e) = FleetAdmiral::run_fleets(
                Arc::clone(&admiral),
                Arc::clone(&client_clone),
                Arc::clone(&bmc),
                Arc::clone(&transfer_cargo_manager),
                treasurer_archiver_join_handle,
            )
            .await
            {
                eprintln!("Error on FleetAdmiral::start_fleets: {}", e);
            }
        }
    });
    Ok(())
}

#[allow(dead_code)]
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
        db::upsert_systems_page(pool, vec![system], now).await?;
    }

    if needs_load_waypoints {
        collect_waypoints_of_system(client, pool, headquarters_system_symbol.clone()).await?;
    }
    Ok(())
}

#[allow(dead_code)]
async fn load_systems_and_waypoints_if_necessary(
    status: StStatusResponse,
    authenticated_client: &dyn StClientTrait,
    pool: &Pool<Postgres>,
    headquarters_system_symbol: &SystemSymbol,
) -> Result<()> {
    let number_systems_in_db = db::select_count_of_systems(pool).await?;

    let need_collect_systems = status.stats.systems as i64 != number_systems_in_db;

    if need_collect_systems {
        event!(
            Level::INFO,
            "Not all {} systems are in database. Currently stored: {}",
            status.stats.systems,
            number_systems_in_db,
        );

        collect_all_systems(authenticated_client, pool).await?;
    } else {
        event!(
            Level::INFO,
            "No need to collect systems - all {} systems are already in db",
            number_systems_in_db
        );
    }

    let systems_with_waypoint_details_to_be_loaded: Vec<DbSystemCoordinateData> = db::select_systems_with_waypoint_details_to_be_loaded(pool).await?;

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
            headquarters_system_symbol,
            pool,
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

#[allow(dead_code)]
async fn collect_all_systems(client: &dyn StClientTrait, pool: &Pool<Postgres>) -> Result<()> {
    let (tx, rx) = mpsc::channel(100); // Buffer up to 100 pages

    let producer = fetch_all_pages_into_queue(|page| client.list_systems_page(page), PaginationInput { page: 1, limit: 20 }, tx);

    tokio::try_join!(producer, upsert_systems_from_receiver(pool, rx))?;
    Ok(())
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[allow(dead_code)]
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
