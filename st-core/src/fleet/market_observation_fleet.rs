use crate::fleet::fleet::{diff_waypoint_symbols, FleetAdmiral};
use anyhow::*;
use itertools::Itertools;
use st_domain::{Fleet, FleetDecisionFacts, MarketObservationFleetConfig, Ship, ShipSymbol, ShipTask, WaypointSymbol};
use std::collections::{HashMap, HashSet};

pub struct MarketObservationFleet;

impl MarketObservationFleet {
    pub async fn compute_ship_tasks(
        admiral: &mut FleetAdmiral,
        cfg: &MarketObservationFleetConfig,
        fleet: &Fleet,
        facts: &FleetDecisionFacts,
    ) -> Result<HashMap<ShipSymbol, ShipTask>> {
        let ships: Vec<&Ship> = admiral.get_ships_of_fleet(fleet);

        let marketplaces_to_explore = diff_waypoint_symbols(&cfg.marketplace_waypoints_of_interest, &facts.marketplaces_with_up_to_date_infos);
        let shipyards_to_explore = diff_waypoint_symbols(&cfg.shipyard_waypoints_of_interest, &facts.shipyards_with_up_to_date_infos);

        // shipyards have higher prio, so they come first
        let all_locations_of_interest = shipyards_to_explore.iter().chain(marketplaces_to_explore.iter()).unique().cloned().collect_vec();

        let already_assigned: HashSet<_> = admiral
            .ship_tasks
            .iter()
            .filter_map(|(ss, task)| match task {
                ShipTask::ObserveWaypointDetails { waypoint_symbol } => Some((ss.clone(), waypoint_symbol.clone())),
                _ => None,
            })
            .collect();

        let already_assigned_ships: HashSet<ShipSymbol> = already_assigned.iter().map(|(ss, _)| ss.clone()).collect();
        let already_assigned_waypoints: HashSet<WaypointSymbol> = already_assigned.iter().map(|(_, wps)| wps.clone()).collect();

        let non_assigned_ships = ships.iter().filter(|s| !already_assigned_ships.contains(&s.symbol));
        let non_assigned_waypoints = all_locations_of_interest.iter().filter(|wps| !already_assigned_waypoints.contains(&wps));

        let result = non_assigned_ships
            .zip(non_assigned_waypoints)
            .map(|(s, wps)| (s.symbol.clone(), ShipTask::ObserveWaypointDetails { waypoint_symbol: wps.clone() }))
            .collect();

        Ok(result)
    }
}
