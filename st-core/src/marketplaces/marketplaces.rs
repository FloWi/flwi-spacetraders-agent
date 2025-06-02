use chrono::{TimeDelta, Utc};
use itertools::Itertools;
use st_domain::{MarketEntry, ShipyardData, Waypoint, WaypointSymbol, WaypointTraitSymbol};

pub fn find_marketplaces_for_exploration(all_marketplaces: Vec<MarketEntry>) -> Vec<WaypointSymbol> {
    let check_me = all_marketplaces
        .iter()
        .find(|me| &me.waypoint_symbol.0 == "X1-XM48-G52");

    if let Some(me) = check_me {
        let has_details = me.market_data.has_detailed_price_information();
        let is_out_of_date = Utc::now() - me.created_at > TimeDelta::hours(3);

        eprintln!("has_details: {} is_out_of_date: {}", has_details, is_out_of_date)
    }

    let waypoint_symbols: Vec<_> = all_marketplaces
        .into_iter()
        .filter(|mp| !mp.market_data.has_detailed_price_information() || Utc::now() - mp.created_at > TimeDelta::hours(3))
        .map(|mp| mp.waypoint_symbol.clone())
        .collect();
    waypoint_symbols
}
pub fn find_shipyards_for_exploration(all_shipyards: Vec<ShipyardData>) -> Vec<WaypointSymbol> {
    let waypoint_symbols: Vec<_> = all_shipyards
        .into_iter()
        .filter(|sd| !sd.shipyard.has_detailed_price_information() || Utc::now() - sd.created_at > TimeDelta::hours(3))
        .map(|mp| mp.waypoint_symbol.clone())
        .collect();
    waypoint_symbols
}

pub fn filter_waypoints_with_trait(waypoints_of_system: &[Waypoint], filter_trait: WaypointTraitSymbol) -> impl Iterator<Item = &Waypoint> + '_ {
    let filtered_waypoints = waypoints_of_system.iter().filter(move |wp| {
        wp.traits
            .iter()
            .any(|waypoint_trait| waypoint_trait.symbol == filter_trait)
    });

    filtered_waypoints
}

pub fn find_marketplaces_to_collect_remotely(all_marketplaces: Vec<MarketEntry>, waypoints_of_system: &[Waypoint]) -> Vec<WaypointSymbol> {
    filter_waypoints_with_trait(waypoints_of_system, WaypointTraitSymbol::MARKETPLACE)
        .map(|waypoint| waypoint.symbol.clone())
        .filter(|wps| !all_marketplaces.iter().any(|me| wps == &me.waypoint_symbol))
        .collect_vec()
}

pub fn find_shipyards_to_collect_remotely(all_shipyards: Vec<ShipyardData>, waypoints_of_system: &[Waypoint]) -> Vec<WaypointSymbol> {
    filter_waypoints_with_trait(waypoints_of_system, WaypointTraitSymbol::SHIPYARD)
        .map(|waypoint| waypoint.symbol.clone())
        .filter(|wps| !all_shipyards.iter().any(|sd| wps == &sd.waypoint_symbol))
        .collect_vec()
}
