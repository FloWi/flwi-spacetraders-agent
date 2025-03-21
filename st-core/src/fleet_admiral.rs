use crate::fleet::{Fleet, SystemSpawningFleet};
use crate::pagination::fetch_all_pages;
use crate::ship::ShipOperations;
use crate::st_client::StClientTrait;
use anyhow::*;
use futures::future::join_all;
use log::{log, Level};
use sqlx::{Pool, Postgres};
use st_domain::{FleetUpdateMessage, Ship, SystemSymbol, Waypoint};
use st_store::{db, DbModelManager};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::sync::mpsc::{Receiver, Sender};

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
        let (fleet_updated_tx, mut fleet_updated_rx): (
            Sender<FleetUpdateMessage>,
            Receiver<FleetUpdateMessage>,
        ) = mpsc::channel::<FleetUpdateMessage>(32);

        let fleets = Self::create_or_load_fleets(
            Arc::clone(&client),
            model_manager.clone(),
            home_system_symbol,
            waypoints_of_home_system,
            ship_updated_tx.clone(),
        )
        .await?;

        log!(
            Level::Info,
            "FleetAdmiral: starting {} fleets",
            fleets.len()
        );

        let fleet_admiral = Self {
            fleets,
            ship_updated_tx,
            fleet_updated_tx,
        };

        fleet_admiral.run_fleets(model_manager).await;
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

            let fleets = crate::fleet::compute_initial_fleet(
                ships,
                home_system_symbol,
                waypoints_of_home_system,
                Arc::clone(&client),
            )
            .await?;
            log!(Level::Info, "computed {} fleets.", fleets.len());

            // persist fleet config
            Ok(fleets)
        } else {
            Ok(db_fleets)
        }
    }

    async fn load_or_collect_ships(
        client: &dyn StClientTrait,
        pool: &Pool<Postgres>,
    ) -> Result<Vec<Ship>> {
        let ships_from_db: Vec<Ship> = db::select_ships(&pool).await?;

        if ships_from_db.is_empty() {
            let ships = Self::collect_all_ships(client).await?;
            Ok(ships)
        } else {
            Ok(ships_from_db)
        }
    }

    async fn collect_all_ships(client: &dyn StClientTrait) -> Result<Vec<Ship>> {
        let ships: Vec<Ship> = fetch_all_pages(|p| client.list_ships(p)).await?;

        Ok(ships)
    }

    async fn run_fleets(&self, db_model_manager: DbModelManager) {
        let handles = self
            .fleets
            .clone()
            .into_iter()
            .map(|fleet| {
                match fleet {
                    Fleet::SystemSpawning(mut system_spawning_fleet) => {
                        log!(Level::Info, "Preparing to run SystemSpawningFleet",);

                        tokio::spawn({
                            // ok, ok - borrow checker is more stubborn than me
                            let db_model_manager = db_model_manager.clone();
                            let ship_updated_tx = self.ship_updated_tx.clone();
                            let fleet_updated_tx = self.fleet_updated_tx.clone();

                            async move {
                                SystemSpawningFleet::run(Arc::new(Mutex::new(system_spawning_fleet)), db_model_manager, ship_updated_tx, fleet_updated_tx)
                                    .await
                            }
                        })
                    }
                    Fleet::MarketObservation(market_observation_fleet) => {
                        tokio::spawn(async move { market_observation_fleet.run().await })
                    }
                    Fleet::Mining(mining_fleet) => {
                        tokio::spawn(async move { mining_fleet.run().await })
                    }
                }
            })
            .collect::<Vec<_>>();

        let results = join_all(handles).await;
        if let Some(err) = results.into_iter().find_map(Result::err) {
            eprintln!("Task error: {}", err);
            // Handle first error
        }
    }
}
