use crate::fleet::fleet::{diff_waypoint_symbols, FleetAdmiral};
use anyhow::*;
use itertools::Itertools;
use st_domain::{Fleet, FleetDecisionFacts, Ship, ShipSymbol, ShipTask, SystemSpawningFleetConfig};
use std::collections::HashMap;

pub struct SystemSpawningFleet;

impl SystemSpawningFleet {
    pub async fn compute_ship_tasks(
        admiral: &mut FleetAdmiral,
        cfg: &SystemSpawningFleetConfig,
        fleet: &Fleet,
        facts: &FleetDecisionFacts,
    ) -> Result<HashMap<ShipSymbol, ShipTask>> {
        let ships: Vec<&Ship> = admiral.get_ships_of_fleet(fleet);
        assert_eq!(ships.len(), 1, "expecting 1 ship");

        // TODO: to optimize we could remove the waypoint from the list where the probe has been spawned

        let marketplaces_to_explore = diff_waypoint_symbols(&cfg.marketplace_waypoints_of_interest, &facts.marketplaces_with_up_to_date_infos);
        let shipyards_to_explore = diff_waypoint_symbols(&cfg.shipyard_waypoints_of_interest, &facts.shipyards_with_up_to_date_infos);

        let all_locations_of_interest = marketplaces_to_explore.iter().chain(shipyards_to_explore.iter()).unique().cloned().collect_vec();

        let result = ships
            .iter()
            .map(|s| {
                (
                    s.symbol.clone(),
                    ShipTask::ObserveAllWaypointsOnce {
                        waypoint_symbols: all_locations_of_interest.clone(),
                    },
                )
            })
            .collect();
        Ok(result)
    }
}
