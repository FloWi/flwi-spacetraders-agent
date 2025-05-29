use crate::fleet::fleet::FleetAdmiral;
use anyhow::*;
use itertools::Itertools;
use petgraph::visit::Walker;
use st_domain::{MarketObservationFleetConfig, Ship, ShipSymbol, ShipTask, WaypointSymbol};
use std::collections::{HashMap, HashSet};

pub struct MarketObservationFleet;

impl MarketObservationFleet {
    pub fn compute_ship_tasks(
        admiral: &FleetAdmiral,
        cfg: &MarketObservationFleetConfig,
        ships_without_tasks: &[&Ship],
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

        let already_assigned: HashSet<(ShipSymbol, WaypointSymbol)> = admiral
            .ship_tasks
            .iter()
            .filter_map(|(ss, task)| match task {
                ShipTask::ObserveWaypointDetails { waypoint_symbol } => {
                    let has_stationary_location_assigned = admiral
                        .stationary_probe_locations
                        .iter()
                        .any(|spl| &spl.probe_ship_symbol == ss && &spl.waypoint_symbol == waypoint_symbol);

                    has_stationary_location_assigned.then_some((ss.clone(), waypoint_symbol.clone()))
                }
                _ => None,
            })
            .collect();

        let already_assigned_ships: HashSet<ShipSymbol> = already_assigned.iter().map(|(ss, _)| ss.clone()).collect();
        let already_assigned_waypoints: HashSet<WaypointSymbol> = already_assigned
            .iter()
            .map(|(_, wps)| wps.clone())
            .collect();

        let non_assigned_ships = ships_without_tasks
            .iter()
            .filter(|s| !already_assigned_ships.contains(&s.symbol))
            .cloned()
            .cloned()
            .collect_vec();

        let non_assigned_waypoints_in_order = all_locations_of_interest
            .iter()
            .filter(|wps| !already_assigned_waypoints.contains(wps))
            .cloned()
            .collect_vec();

        let non_assigned_waypoints: HashSet<WaypointSymbol> = non_assigned_waypoints_in_order
            .iter()
            .cloned()
            .collect::<HashSet<_>>();

        let current_ship_locations: HashMap<WaypointSymbol, Vec<(ShipSymbol, WaypointSymbol)>> = ships_without_tasks
            .iter()
            .map(|s| (s.symbol.clone(), s.nav.waypoint_symbol.clone()))
            .into_group_map_by(|(_, wps)| wps.clone());

        let correctly_placed_ships: Vec<(ShipSymbol, WaypointSymbol)> = current_ship_locations
            .into_iter()
            .filter(|(wps, _)| non_assigned_waypoints.contains(wps))
            .filter_map(|(wps, ship_locations)| ship_locations.first().cloned()) // there might be multiple ships at this waypoint - we only need one
            .collect_vec();

        let already_correctly_placed_ships: HashSet<ShipSymbol> = correctly_placed_ships
            .iter()
            .map(|(ss, _wps)| ss.clone())
            .collect();

        let non_assigned_ships = non_assigned_ships
            .iter()
            .filter(|s| !already_correctly_placed_ships.contains(&s.symbol))
            .cloned()
            .collect_vec();

        let result = non_assigned_ships
            .iter()
            .zip(non_assigned_waypoints_in_order.iter())
            .map(|(s, wps)| (s.symbol.clone(), ShipTask::ObserveWaypointDetails { waypoint_symbol: wps.clone() }))
            .chain(
                correctly_placed_ships
                    .iter()
                    .map(|(ss, wps)| (ss.clone(), ShipTask::ObserveWaypointDetails { waypoint_symbol: wps.clone() })),
            ) // issue tasks for the probes that are already placed correctly
            .collect();

        Ok(result)
    }
}
