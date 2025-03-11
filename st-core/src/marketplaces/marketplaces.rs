use itertools::Itertools;
use st_domain::{Waypoint, WaypointSymbol, WaypointTrait, WaypointTraitSymbol};
use st_store::{DbMarketEntry, DbWaypointEntry};

pub fn find_marketplaces_for_exploration(
    all_marketplaces: Vec<DbMarketEntry>,
) -> Vec<WaypointSymbol> {

    let waypoint_symbols: Vec<_> = all_marketplaces
        .into_iter()
        .filter(|mp| !mp.entry.has_detailed_price_information())
        .map(|mp| WaypointSymbol(mp.waypoint_symbol.clone()))
        .collect();
    waypoint_symbols
}


pub fn find_marketplaces_to_collect_remotely(
    all_marketplaces: Vec<DbMarketEntry>,
    waypoints_of_system: &[DbWaypointEntry],
) -> Vec<WaypointSymbol> {

    let marketplace_waypoints =  waypoints_of_system.into_iter().filter(|wp| wp.entry.traits.iter().any(|waypoint_trait| waypoint_trait.symbol == WaypointTraitSymbol::MARKETPLACE)).collect_vec();

    marketplace_waypoints
        .into_iter()
        .map(|db_waypoint_entry| db_waypoint_entry.entry.symbol.clone())
        .filter(|wps| !all_marketplaces.iter().any(|db_market_entry| wps.0 == db_market_entry.waypoint_symbol) )
        .collect_vec()

}
