use crate::fleet::fleet::FleetAdmiral;
use anyhow::*;
use itertools::Itertools;
use petgraph::visit::Walker;
use st_domain::{FleetId, MarketObservationFleetConfig, Ship, ShipSymbol, ShipTask, WaypointSymbol};
use std::collections::{HashMap, HashSet};
use std::ops::Not;

pub struct MarketObservationFleet;

impl MarketObservationFleet {
    pub fn compute_ship_tasks(
        admiral: &FleetAdmiral,
        cfg: &MarketObservationFleetConfig,
        ships_without_tasks: &[&Ship],
        all_ships_of_fleet: &[&Ship],
        fleet_id: &FleetId,
    ) -> Result<HashMap<ShipSymbol, ShipTask>> {
        let marketplaces_to_explore = cfg.marketplace_waypoints_of_interest.clone();
        let shipyards_to_explore = cfg.shipyard_waypoints_of_interest.clone();

        // shipyards have higher prio, so they come first
        let all_locations_of_interest = shipyards_to_explore
            .iter()
            .chain(marketplaces_to_explore.iter())
            .unique()
            .cloned()
            .collect_vec();

        let current_ship_tasks = admiral.get_ship_tasks_of_fleet_id(fleet_id);

        // make sure we don't cover the same location twice, because we are unable to code bug-free ;-)
        let already_assigned_without_duplicates: HashMap<WaypointSymbol, ShipSymbol> = current_ship_tasks
            .iter()
            .filter_map(|(ss, task)| match task {
                ShipTask::ObserveWaypointDetails { waypoint_symbol } => Some((waypoint_symbol.clone(), ss.clone())),
                _ => None,
            })
            // make sure we don't have duplicates
            .collect::<HashMap<_, _>>();

        let current_ship_assignments = already_assigned_without_duplicates
            .iter()
            .map(|tup| (tup.1.clone(), tup.0.clone()))
            .collect::<HashMap<_, _>>();

        let non_covered_locations = all_locations_of_interest
            .iter()
            .filter(|wps| already_assigned_without_duplicates.contains_key(wps).not())
            .cloned()
            .collect_vec();

        let available_ships = all_ships_of_fleet
            .iter()
            .filter_map(|ship| {
                current_ship_assignments
                    .contains_key(&ship.symbol)
                    .not()
                    .then_some(ship.symbol.clone())
            })
            .collect_vec();

        let new_ship_tasks = available_ships
            .iter()
            .zip(non_covered_locations)
            .map(|(ss, wps)| (ss.clone(), ShipTask::ObserveWaypointDetails { waypoint_symbol: wps }))
            .collect();

        Ok(new_ship_tasks)
    }
}
