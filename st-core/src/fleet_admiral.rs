use crate::fleet::{Fleet, SystemSpawningFleet};
use crate::pagination::fetch_all_pages;
use crate::ship::ShipOperations;
use crate::st_client::StClientTrait;
use anyhow::*;
use chrono::Utc;
use futures::future::join_all;
use log::{log, Level};
use sqlx::{Pool, Postgres};
use st_domain::{FleetUpdateMessage, Ship, SystemSymbol, Waypoint};
use st_store::{db, Ctx, DbModelManager, FleetBmc};
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::{mpsc, Mutex, RwLock};

pub struct FleetAdmiral {
    fleets: Vec<Fleet>,
    ship_updated_tx: Sender<ShipOperations>,
    fleet_updated_tx: Sender<FleetUpdateMessage>,
}

impl FleetAdmiral {
    pub async fn start_fleets(
        client: Arc<dyn StClientTrait>,

        model_manager: DbModelManager,
        home_system_symbol: &SystemSymbol,
        waypoints_of_home_system: &[Waypoint],
        ship_updated_tx: Sender<ShipOperations>,
    ) -> Result<()> {
        let (fleet_updated_tx, mut fleet_updated_rx): (Sender<FleetUpdateMessage>, Receiver<FleetUpdateMessage>) = mpsc::channel::<FleetUpdateMessage>(32);

        let fleets = Self::create_or_load_fleets(
            Arc::clone(&client),
            model_manager.clone(),
            home_system_symbol,
            waypoints_of_home_system,
            ship_updated_tx.clone(),
        )
        .await?;

        log!(Level::Info, "FleetAdmiral: starting {} fleets", fleets.len());

        let fleet_admiral = Arc::new(RwLock::new(Self {
            fleets,
            ship_updated_tx,
            fleet_updated_tx,
        }));

        // Spawn a task to listen for fleet updates with shared access to fleet_admiral
        tokio::spawn({
            let fleet_admiral_clone = Arc::clone(&fleet_admiral);
            let mm_clone = model_manager.clone();
            async move { Self::listen_to_fleet_updated_messages(fleet_updated_rx, fleet_admiral_clone, mm_clone).await }
        });

        Self::run_fleets(&fleet_admiral, &model_manager).await;
        Ok(())
    }

    // Modified to take the receiver and shared access to FleetAdmiral
    async fn listen_to_fleet_updated_messages(
        mut fleet_updated_rx: Receiver<FleetUpdateMessage>,
        fleet_admiral: Arc<RwLock<FleetAdmiral>>,
        model_manager: DbModelManager,
    ) -> Result<()> {
        while let Some(msg) = fleet_updated_rx.recv().await {
            match msg {
                FleetUpdateMessage::FleetTaskCompleted {
                    fleet_task_completion,
                    fleet_id,
                } => {
                    // Save to database
                    let _ = FleetBmc::save_completed_fleet_tasks(&Ctx::Anonymous, &model_manager, vec![fleet_task_completion.clone()]).await?;

                    log!(
                        Level::Info,
                        "FleetAdmiral: Hooray fleet {} completed task {:?}",
                        fleet_id.0,
                        fleet_task_completion
                    );

                    // Modify fleet admiral state
                    {
                        let mut admiral = fleet_admiral.write().await;
                        // Now you can modify the admiral state here
                        // For example:
                        // admiral.compute_new_tasks(&model_manager, fleet_id).await?;
                        // Or any other state modifications you need
                    }
                }
            }
        }

        Ok(())
    }
    async fn create_or_load_fleets(
        client: Arc<dyn StClientTrait>,
        model_manager: DbModelManager,
        home_system_symbol: &SystemSymbol,
        waypoints_of_home_system: &[Waypoint],
        ship_updated_tx: Sender<ShipOperations>,
    ) -> Result<Vec<Fleet>> {
        let ships: Vec<Ship> = Self::load_or_collect_ships(&*client, model_manager.pool()).await?;
        let db_fleets: Vec<Fleet> = Vec::new();

        if db_fleets.is_empty() {
            log!(Level::Info, "db_fleets is empty. Computing fleets",);

            let fleets = crate::fleet::compute_fleets(ships, home_system_symbol, waypoints_of_home_system, Arc::clone(&client), model_manager).await?;
            log!(Level::Info, "computed {} fleets.", fleets.len());

            // persist fleet config
            Ok(fleets)
        } else {
            Ok(db_fleets)
        }
    }

    async fn load_or_collect_ships(client: &dyn StClientTrait, pool: &Pool<Postgres>) -> Result<Vec<Ship>> {
        let ships_from_db: Vec<Ship> = db::select_ships(&pool).await?;

        if ships_from_db.is_empty() {
            let ships = Self::collect_all_ships(client).await?;
            db::upsert_ships(&pool, &ships, Utc::now()).await?;
            Ok(ships)
        } else {
            Ok(ships_from_db)
        }
    }

    async fn collect_all_ships(client: &dyn StClientTrait) -> Result<Vec<Ship>> {
        let ships: Vec<Ship> = fetch_all_pages(|p| client.list_ships(p)).await?;

        Ok(ships)
    }

    // Modified to take shared access to FleetAdmiral
    async fn run_fleets(fleet_admiral: &Arc<RwLock<FleetAdmiral>>, db_model_manager: &DbModelManager) {
        let handles = {
            // Lock the mutex only to read the fleets and clone what we need
            let admiral = fleet_admiral.read().await;

            admiral
                .fleets
                .clone()
                .into_iter()
                .map(|fleet| match fleet {
                    Fleet::SystemSpawning(system_spawning_fleet) => {
                        log!(Level::Info, "Preparing to run SystemSpawningFleet");

                        let db_model_manager = db_model_manager.clone();
                        let ship_updated_tx = admiral.ship_updated_tx.clone();
                        let fleet_updated_tx = admiral.fleet_updated_tx.clone();
                        let fleet_admiral_clone = Arc::clone(fleet_admiral);

                        tokio::spawn(async move {
                            SystemSpawningFleet::run(Arc::new(Mutex::new(system_spawning_fleet)), db_model_manager, ship_updated_tx, fleet_updated_tx).await
                        })
                    }
                    Fleet::MarketObservation(market_observation_fleet) => {
                        let fleet_admiral_clone = Arc::clone(fleet_admiral);
                        tokio::spawn(async move { market_observation_fleet.run().await })
                    }
                    Fleet::Mining(mining_fleet) => {
                        let fleet_admiral_clone = Arc::clone(fleet_admiral);
                        tokio::spawn(async move { mining_fleet.run().await })
                    }
                    fleet => tokio::spawn(async move {
                        println!("Fleet {fleet:?} not implemented yet");
                        Ok(())
                    }),
                })
                .collect::<Vec<_>>()
        };

        let results = join_all(handles).await;
        if let Some(err) = results.into_iter().find_map(Result::err) {
            eprintln!("Task error: {}", err);
            // Handle first error
        }
    }
}
