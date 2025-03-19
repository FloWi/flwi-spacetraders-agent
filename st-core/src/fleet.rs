use crate::exploration::exploration::generate_exploration_route;
use crate::marketplaces::marketplaces::{
    filter_waypoints_with_trait, find_marketplaces_for_exploration, find_shipyards_for_exploration,
};
use crate::ship::ShipOperations;
use crate::st_client::StClientTrait;
use anyhow::{anyhow, Result};
use itertools::Itertools;
use log::{log, Level};
use serde::{Deserialize, Serialize};
use st_domain::{
    Ship, ShipRegistrationRole, ShipSymbol, ShipType, SystemSymbol, TradeGoodSymbol, Waypoint,
    WaypointSymbol, WaypointTraitSymbol,
};
use st_store::{
    db, select_latest_marketplace_entry_of_system, select_latest_shipyard_entry_of_system, Ctx,
    DbModelManager, DbWaypointEntry, MarketBmc, ShipBmc, SystemBmc,
};
use std::collections::HashMap;
use std::ops::Not;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};
use crate::agent::ship_loop;
use crate::behavior_tree::behavior_args::{BehaviorArgs, DbBlackboard};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FleetId(u32);

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ShipRole {
    MarketObserver,
    ShipPurchaser,
    Miner,
    MiningHauler,
    Trader,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SystemSpawningFleet {
    id: FleetId,
    system_symbol: SystemSymbol,
    marketplace_waypoints_of_interest: Vec<WaypointSymbol>,
    shipyard_waypoints_of_interest: Vec<WaypointSymbol>,
    spawn_ship_symbol: ShipSymbol,
    #[serde(skip)]
    ship_operations: HashMap<ShipSymbol, ShipOperations>,
    budget: u64,
}

impl SystemSpawningFleet {
    pub async fn run_fleet(&mut self, mm: &DbModelManager) -> Result<()> {
        let task = Self::compute_initial_exploration_ship_task(&self, &mm).await?;

        log!(
            Level::Info,
            "Computed this task for the command ship: {}",
            serde_json::to_string_pretty(&task)?
        );

        match task {
            Some(ShipTask::ObserveAllWaypointsOnce {
                     waypoint_symbols
                 }) => {
                let mut command_ship = self.ship_operations.get(&self.spawn_ship_symbol).unwrap().clone();
                command_ship.set_explore_locations(waypoint_symbols);

                let args = BehaviorArgs {
                    blackboard: Arc::new(DbBlackboard { db: mm.pool().clone() }),
                };

                let (ship_updated_tx, mut ship_updated_rx): (Sender<ShipOperations>, Receiver<ShipOperations>) =
                    mpsc::channel::<ShipOperations>(32);

                let _ = tokio::spawn(ship_loop(command_ship, args, ship_updated_tx));

            }
            maybe_task => {
                log!(Level::Warn, "Not implemented yet. {maybe_task:?}");
            }
        }


        Ok(())
    }

    pub async fn compute_initial_exploration_ship_task(
        &self,
        mm: &DbModelManager,
    ) -> Result<Option<ShipTask>> {
        let waypoints_of_system =
            SystemBmc::get_waypoints_of_system(&Ctx::Anonymous, mm, &self.system_symbol).await?;

        let marketplace_entries =
            select_latest_marketplace_entry_of_system(mm.pool(), &self.system_symbol).await?;

        let marketplaces_to_explore =
            find_marketplaces_for_exploration(marketplace_entries.clone());

        let shipyard_entries =
            select_latest_shipyard_entry_of_system(mm.pool(), &self.system_symbol).await?;

        let shipyards_to_explore = find_shipyards_for_exploration(shipyard_entries.clone());

        log!(Level::Debug, "waypoints_of_system: {waypoints_of_system:?}");
        log!(Level::Debug, "marketplace_entries: {marketplace_entries:?}");
        log!(
            Level::Debug,
            "marketplaces_to_explore: {marketplaces_to_explore:?}"
        );

        log!(Level::Debug, "shipyard_entries: {shipyard_entries:?}");
        log!(
            Level::Debug,
            "shipyards_to_explore: {shipyards_to_explore:?}"
        );

        let relevant_exploration_targets = marketplaces_to_explore
            .into_iter()
            .chain(shipyards_to_explore.into_iter())
            .filter(|wp_symbol| {
                self.marketplace_waypoints_of_interest.contains(wp_symbol)
                    || self.shipyard_waypoints_of_interest.contains(wp_symbol)
            })
            .unique()
            .collect_vec();

        log!(
            Level::Info,
            "relevant_exploration_targets: {relevant_exploration_targets:?}"
        );

        let current_location = self
            .ship_operations
            .get(&self.spawn_ship_symbol)
            .unwrap()
            .nav
            .waypoint_symbol
            .clone();

        let exploration_route = generate_exploration_route(
            &relevant_exploration_targets,
            &waypoints_of_system,
            &current_location,
        );

        let exploration_route_symbols = exploration_route
            .unwrap_or_default()
            .into_iter()
            .map(|wp| wp.symbol)
            .collect_vec();

        Ok(exploration_route_symbols.is_empty().not().then_some(
            ShipTask::ObserveAllWaypointsOnce {
                waypoint_symbols: exploration_route_symbols,
            },
        ))
    }

    fn compute_shopping_list() {
        /*
           List(
             // IMPORTANT: the shopping list is persisted to DB, later changes might not be effective
             (1, SHIP_COMMAND_FRIGATE, FRIGATE_GENERALIST),
             (numExplorersForSystem, SHIP_PROBE, INNER_SYSTEM_EXPLORER),
             (2, SHIP_SURVEYOR, SURVEYOR),
             (2, SHIP_SIPHON_DRONE, STARTING_SIPHONER_I),
             (1, SHIP_MINING_DRONE, STARTING_MINER_I),
             (1, SHIP_LIGHT_HAULER, LIGHT_MINING_HAULER),
             (4, SHIP_MINING_DRONE, STARTING_MINER_I),
             (1, SHIP_LIGHT_HAULER, CONSTRUCTOR),
             (3, SHIP_SIPHON_DRONE, STARTING_SIPHONER_I),
             (3, SHIP_MINING_DRONE, STARTING_MINER_I),
             (2, SHIP_LIGHT_HAULER, CONSTRUCTOR),
             (1, SHIP_LIGHT_HAULER, LIGHT_MINING_HAULER),
             (5, SHIP_MINING_DRONE, STARTING_MINER_I),
             (1, SHIP_LIGHT_HAULER, CONTRACTOR),
             (2, SHIP_LIGHT_HAULER, CONSTRUCTOR),
             // IMPORTANT: the shopping list is persisted to DB, later changes might not be effective
           ).flatMap { case (num, tpe, role) => 1.to(num).map(_ => ShipShoppingListEntry(tpe, role)) }
         }
        */
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MarketObservationFleet {
    id: FleetId,
    system_symbol: SystemSymbol,
    marketplace_waypoints_of_interest: Vec<WaypointSymbol>,
    shipyard_waypoints_of_interest: Vec<WaypointSymbol>,
    ship_assignment: HashMap<ShipSymbol, WaypointSymbol>,
    ship_role_assignment: HashMap<ShipSymbol, Vec<ShipRole>>,
    budget: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MiningFleet {
    id: FleetId,
    system_symbol: SystemSymbol,
    mining_waypoint: WaypointSymbol,
    materials: Vec<TradeGoodSymbol>,
    mining_ships: Vec<ShipSymbol>,
    mining_haulers: Vec<ShipSymbol>,
    delivery_locations: HashMap<WaypointSymbol, Vec<TradeGoodSymbol>>,
    budget: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Fleet {
    MarketObservation(MarketObservationFleet),
    SystemSpawning(SystemSpawningFleet),
    Mining(MiningFleet),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum FleetTasks {
    CollectMarketInfosOnce { system_symbol: SystemSymbol },

    ObserveAllWaypointsOfSystemWithProbes { system_symbol: SystemSymbol },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ShipTask {
    PurchaseShip {
        r#type: ShipType,
        max_amount: u32,
        system_symbol: SystemSymbol,
    },

    ObserveWaypointDetails {
        waypoint_symbol: WaypointSymbol,
    },

    ObserveAllWaypointsOnce {
        waypoint_symbols: Vec<WaypointSymbol>,
    },
}

/*

- Game starts with two ships - command ship and one probe
- we first need some data for markets and shipyards in order to earn money for more ships
- we assign the command ship to the SystemSpawningFleet and give it the relevant waypoints
- we assign the probe to the MarketObservationFleet. It should already be placed at the shipyard, so we can assign this waypoint already

 */
pub(crate) async fn compute_initial_fleet(
    ships: Vec<Ship>,
    home_system_symbol: &SystemSymbol,
    waypoints_of_home_system: &[Waypoint],
    model_manager: DbModelManager,
    client: Arc<dyn StClientTrait>,
) -> Result<Vec<Fleet>> {
    assert_eq!(ships.len(), 2, "Expecting two ships to start");

    if ships.len() != 2 {
        return anyhow::bail!("Expected 2 ships, but found {}", ships.len());
    }

    let marketplace_waypoints =
        filter_waypoints_with_trait(waypoints_of_home_system, WaypointTraitSymbol::MARKETPLACE)
            .map(|wp| wp.symbol.clone())
            .collect_vec();
    let shipyard_waypoints =
        filter_waypoints_with_trait(waypoints_of_home_system, WaypointTraitSymbol::SHIPYARD)
            .map(|wp| wp.symbol.clone())
            .collect_vec();

    let command_ship = ships
        .iter()
        .find(|ship| ship.registration.role == ShipRegistrationRole::Command)
        .unwrap()
        .clone();

    let probe_ship = ships
        .iter()
        .find(|ship| ship.registration.role == ShipRegistrationRole::Satellite)
        .unwrap();

    // iirc the probe gets spawned at a shipyard
    // make sure, this is the case and expect it
    let probe_at_shipyard_location = shipyard_waypoints
        .iter()
        .find(|wps| **wps == probe_ship.nav.waypoint_symbol)
        .cloned()
        .expect("expecting probe to be spawned at shipyard");

    let unexplored_shipyards = shipyard_waypoints
        .iter()
        .filter(|wp| **wp != probe_at_shipyard_location)
        .cloned()
        .collect_vec();

    log!(
        Level::Info,
        "found {} ships: {}",
        &ships.len(),
        serde_json::to_string_pretty(&ships)?
    );

    log!(
        Level::Info,
        "command_ship: {}",
        serde_json::to_string_pretty(&command_ship)?
    );
    log!(
        Level::Info,
        "probe_ship: {}",
        serde_json::to_string_pretty(&probe_ship)?
    );

    let command_ship_op = ShipOperations::new(command_ship.clone(), Arc::clone(&client));

    let system_spawning_fleet = SystemSpawningFleet {
        id: FleetId(1),
        system_symbol: home_system_symbol.clone(),
        marketplace_waypoints_of_interest: marketplace_waypoints.clone(),
        shipyard_waypoints_of_interest: unexplored_shipyards.clone(),

        spawn_ship_symbol: command_ship.symbol.clone(),
        ship_operations: HashMap::from([(command_ship.symbol.clone(), command_ship_op)]),
        budget: 0,
    };

    // let command_ship_tasks: ShipTask = system_spawning_fleet
    //     .compute_ship_task(&model_manager)
    //     .await?;

    let market_observation_fleet = MarketObservationFleet {
        id: FleetId(2),
        system_symbol: home_system_symbol.clone(),
        marketplace_waypoints_of_interest: marketplace_waypoints.clone(),
        shipyard_waypoints_of_interest: shipyard_waypoints.clone(),
        ship_assignment: HashMap::from([(
            probe_ship.symbol.clone(),
            probe_at_shipyard_location.clone(),
        )]),
        ship_role_assignment: HashMap::from([(
            command_ship.symbol.clone(),
            vec![ShipRole::MarketObserver, ShipRole::ShipPurchaser],
        )]),
        budget: 0,
    };

    let fleets = vec![
        Fleet::SystemSpawning(system_spawning_fleet),
        Fleet::MarketObservation(market_observation_fleet),
    ];

    log!(
        Level::Info,
        "Created these fleets: {}",
        serde_json::to_string_pretty(&fleets)?
    );

    Ok(Vec::new())
}
