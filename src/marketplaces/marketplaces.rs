use crate::db::DbMarketEntry;
use crate::st_model::WaypointSymbol;

pub fn find_marketplaces_for_exploration(
    all_marketplaces: Vec<DbMarketEntry>,
) -> Vec<WaypointSymbol> {
    let waypoint_symbols: Vec<_> = all_marketplaces
        .into_iter()
        .filter(|mp| mp.entry.has_detailed_price_information() == false)
        .map(|mp| WaypointSymbol(mp.waypoint_symbol.clone()))
        .collect();
    waypoint_symbols
}
