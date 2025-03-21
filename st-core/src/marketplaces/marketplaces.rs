use chrono::{TimeDelta, Utc};
use itertools::Itertools;
use st_domain::{Waypoint, WaypointSymbol, WaypointTraitSymbol};
use st_store::{DbMarketEntry, DbShipyardData};

pub fn find_marketplaces_for_exploration(all_marketplaces: Vec<DbMarketEntry>) -> Vec<WaypointSymbol> {
    let waypoint_symbols: Vec<_> = all_marketplaces
        .into_iter()
        .filter(|mp| !mp.entry.has_detailed_price_information() || Utc::now() - mp.created_at > TimeDelta::hours(1))
        .map(|mp| WaypointSymbol(mp.waypoint_symbol.clone()))
        .collect();
    waypoint_symbols
}
pub fn find_shipyards_for_exploration(all_shipyards: Vec<DbShipyardData>) -> Vec<WaypointSymbol> {
    let waypoint_symbols: Vec<_> = all_shipyards
        .into_iter()
        .filter(|mp| !mp.has_detailed_price_information() || Utc::now() - mp.updated_at > TimeDelta::hours(1))
        .map(|mp| WaypointSymbol(mp.waypoint_symbol.clone()))
        .collect();
    waypoint_symbols
}

pub fn filter_waypoints_with_trait<'a>(waypoints_of_system: &'a [Waypoint], filter_trait: WaypointTraitSymbol) -> impl Iterator<Item = &'a Waypoint> + 'a {
    let filtered_waypoints = waypoints_of_system.into_iter().filter(move |wp| wp.traits.iter().any(|waypoint_trait| waypoint_trait.symbol == filter_trait));

    filtered_waypoints
}

pub fn find_marketplaces_to_collect_remotely(all_marketplaces: Vec<DbMarketEntry>, waypoints_of_system: &[Waypoint]) -> Vec<WaypointSymbol> {
    filter_waypoints_with_trait(waypoints_of_system, WaypointTraitSymbol::MARKETPLACE)
        .into_iter()
        .map(|waypoint| waypoint.symbol.clone())
        .filter(|wps| !all_marketplaces.iter().any(|db_market_entry| wps.0 == db_market_entry.waypoint_symbol))
        .collect_vec()
}

pub fn find_shipyards_to_collect_remotely(all_shipyards: Vec<DbShipyardData>, waypoints_of_system: &[Waypoint]) -> Vec<WaypointSymbol> {
    filter_waypoints_with_trait(waypoints_of_system, WaypointTraitSymbol::SHIPYARD)
        .into_iter()
        .map(|waypoint| waypoint.symbol.clone())
        .filter(|wps| !all_shipyards.iter().any(|db_market_entry| wps.0 == db_market_entry.waypoint_symbol))
        .collect_vec()
}
