use anyhow::{anyhow, Result};
use log::{log, Level};
use st_domain::{Ship, ShipRegistrationRole, ShipSymbol, SystemSymbol, Waypoint, WaypointSymbol, WaypointTraitSymbol};
use std::collections::HashMap;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use st_store::{db, DbWaypointEntry};
use crate::marketplaces::marketplaces::filter_waypoints_with_trait;

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
    ship_role_assignment: HashMap<ShipSymbol, Vec<ShipRole>>,
    budget: u64,
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
pub enum Fleet {
    MarketObservation(MarketObservationFleet),
    SystemSpawning(SystemSpawningFleet),
}

/*

- Game starts with two ships - command ship and one probe
- we first need some data for markets and shipyards in order to earn money for more ships
- we assign the command ship to the SystemSpawningFleet and give it the relevant waypoints
- we assign the probe to the MarketObservationFleet

 */
pub(crate) fn compute_initial_fleet(ships: Vec<Ship>, home_system_symbol: &SystemSymbol, waypoints_of_home_system: &[Waypoint]) -> Result<Vec<Fleet>> {
    assert_eq!(ships.len(), 2, "Expecting two ships to start");


    if ships.len() != 2 {
        return anyhow::bail!("Expected 2 ships, but found {}", ships.len());
    }

    let marketplace_waypoints = filter_waypoints_with_trait(waypoints_of_home_system, WaypointTraitSymbol::MARKETPLACE).map(|wp| wp.symbol.clone()).collect_vec();
    let shipyard_waypoints = filter_waypoints_with_trait(waypoints_of_home_system, WaypointTraitSymbol::SHIPYARD).map(|wp| wp.symbol.clone()).collect_vec();

    let command_ship = ships.iter().find(|ship| ship.registration.role == ShipRegistrationRole::Command).unwrap();
    let probe_ship = ships.iter().find(|ship| ship.registration.role == ShipRegistrationRole::Satellite).unwrap();

    // iirc the probe gets spawned at a shipyard
    // make sure, this is the case
    let probe_at_shipyard_location = shipyard_waypoints.iter().find(|wps| **wps == probe_ship.nav.waypoint_symbol).cloned().expect("expecting probe to be spawned at shipyard");

    let unexplored_shipyards = shipyard_waypoints.iter().filter(|wp| **wp != probe_at_shipyard_location).cloned().collect_vec();

    log!(Level::Info, "found {} ships: {}", &ships.len(), serde_json::to_string_pretty(&ships)?);

    log!(Level::Info, "command_ship: {}", serde_json::to_string_pretty(&command_ship)?);
    log!(Level::Info, "probe_ship: {}", serde_json::to_string_pretty(&probe_ship)?);

    let system_spawning_fleet = SystemSpawningFleet {
        id: FleetId(1),
        system_symbol: home_system_symbol.clone(),
        marketplace_waypoints_of_interest: marketplace_waypoints.clone(),
        shipyard_waypoints_of_interest: unexplored_shipyards.clone(),
        ship_role_assignment: HashMap::from([(command_ship.symbol.clone(), vec![ShipRole::MarketObserver, ShipRole::ShipPurchaser, ShipRole::Trader])]),
        budget: 0,
    };

    let market_observation_fleet = MarketObservationFleet {
        id: FleetId(2),
        system_symbol: home_system_symbol.clone(),
        marketplace_waypoints_of_interest: marketplace_waypoints.clone(),
        shipyard_waypoints_of_interest: shipyard_waypoints.clone(),
        ship_assignment: HashMap::from([(probe_ship.symbol.clone(), probe_at_shipyard_location.clone())]),
        ship_role_assignment: HashMap::from([(command_ship.symbol.clone(), vec![ShipRole::MarketObserver, ShipRole::ShipPurchaser])]),
        budget: 0,
    };

    let fleets = vec![Fleet::SystemSpawning(system_spawning_fleet), Fleet::MarketObservation(market_observation_fleet)];

    log!(Level::Info, "Created these fleets: {}", serde_json::to_string_pretty(&fleets)?);

    Ok(Vec::new())
}
