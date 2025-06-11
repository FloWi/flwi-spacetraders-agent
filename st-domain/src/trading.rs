use crate::budgeting::treasury_redesign::ActiveTradeRoute;
use crate::{
    EvaluatedTradingOpportunity, LabelledCoordinate, MarketEntry, MarketTradeGood, Ship, ShipSymbol, TradeGoodSymbol, TradeGoodType, TradingOpportunity,
    Waypoint, WaypointSymbol,
};
use itertools::Itertools;
use ordered_float::OrderedFloat;
use std::collections::{HashMap, HashSet};

pub fn find_trading_opportunities_sorted_by_profit_per_distance_unit(
    market_data: &[(WaypointSymbol, Vec<MarketTradeGood>)],
    waypoint_map: &HashMap<WaypointSymbol, &Waypoint>,
    no_go_trades: &HashSet<(TradeGoodSymbol, TradeGoodSymbol)>,
) -> Vec<TradingOpportunity> {
    let denormalized_trade_goods_with_wp_symbols = market_data
        .iter()
        .flat_map(|(wp_symbol, market_trade_goods)| {
            market_trade_goods
                .iter()
                .map(|mtg| (wp_symbol.clone(), mtg.clone()))
        })
        .collect_vec();

    let exports = denormalized_trade_goods_with_wp_symbols
        .iter()
        .filter(|(wp_sym, mtg)| mtg.trade_good_type == TradeGoodType::Export || mtg.trade_good_type == TradeGoodType::Exchange)
        .collect_vec();

    let imports = denormalized_trade_goods_with_wp_symbols
        .iter()
        .filter(|(wp_sym, mtg)| mtg.trade_good_type == TradeGoodType::Import || mtg.trade_good_type == TradeGoodType::Exchange)
        .collect_vec();

    let forbidden_trade_symbols = no_go_trades
        .iter()
        .map(|(from, to)| from.clone())
        .collect::<HashSet<_>>();

    let trades_by_profit = exports
        .iter()
        .filter(|(export_wps, export_mtg)| !forbidden_trade_symbols.contains(&export_mtg.symbol))
        .flat_map(|(export_wps, export_mtg)| {
            let export_wp = waypoint_map.get(export_wps).unwrap();
            imports
                .iter()
                .filter(move |(import_wps, import_mtg)| {
                    export_wps != import_wps && export_mtg.symbol == import_mtg.symbol && import_mtg.sell_price > export_mtg.purchase_price
                })
                .map(|(import_wps, import_mtg)| {
                    let import_wp = waypoint_map.get(import_wps).unwrap();
                    let profit_per_unit = (import_mtg.sell_price - export_mtg.purchase_price) as u64;
                    let direct_distance = import_wp.distance_to(export_wp);
                    let direct_distance = if direct_distance < 1 {
                        1
                    } else {
                        direct_distance
                    };
                    let profit_per_unit_per_distance = profit_per_unit as f64 / direct_distance as f64;

                    TradingOpportunity {
                        purchase_waypoint_symbol: export_wps.clone(),
                        purchase_market_trade_good_entry: export_mtg.clone(),
                        sell_waypoint_symbol: import_wps.clone(),
                        sell_market_trade_good_entry: import_mtg.clone(),
                        direct_distance,
                        profit_per_unit,
                        profit_per_unit_per_distance: OrderedFloat(profit_per_unit_per_distance),
                    }
                })
        })
        .sorted_by_key(|trading_opp| trading_opp.profit_per_unit_per_distance)
        .rev()
        .collect_vec();

    trades_by_profit
}

pub fn to_trade_goods_with_locations(market_data: &[MarketEntry]) -> Vec<(WaypointSymbol, Vec<MarketTradeGood>)> {
    market_data
        .iter()
        .filter_map(|md| {
            md.market_data
                .trade_goods
                .as_ref()
                .map(|trade_goods| (md.waypoint_symbol.clone(), trade_goods.clone()))
        })
        .collect_vec()
}

pub fn evaluate_trading_opportunities(
    unassigned_ships: &[&Ship],
    waypoint_map: &HashMap<WaypointSymbol, &Waypoint>,
    trading_opportunities: &[TradingOpportunity],
    budget_for_trading: i64,
) -> Vec<EvaluatedTradingOpportunity> {
    let top_trading_opps = trading_opportunities
        .iter()
        .sorted_by_key(|t| -t.profit_per_unit_per_distance)
        .take(15)
        .collect_vec();

    let budget_for_ship = if unassigned_ships.is_empty() {
        0
    } else {
        u64::try_from(budget_for_trading).unwrap_or(0) / unassigned_ships.len() as u64
    };

    let evaluated_trading_opportunities = unassigned_ships
        .iter()
        .flat_map(|ship| {
            let ship_wp = waypoint_map.get(&ship.nav.waypoint_symbol).unwrap();
            let ship_cargo_space = (ship.cargo.capacity - ship.cargo.units) as u32;

            top_trading_opps.iter().filter_map(move |trading_opp| {
                let purchase_wp = waypoint_map
                    .get(&trading_opp.purchase_waypoint_symbol)
                    .unwrap();
                let distance_to_start = ship_wp.distance_to(purchase_wp);
                let total_distance = distance_to_start + trading_opp.direct_distance;
                let total_distance = if total_distance == 0 {
                    1
                } else {
                    total_distance
                };
                //TODO: maybe relax restriction when supply is abundant or high (check old scala solution of batches again)

                // in the beginning we have to respect the budget constraint.
                // after a bit of trading we should always be able to afford a full cargo load

                let num_units_within_budget = budget_for_ship as u32 / trading_opp.purchase_market_trade_good_entry.purchase_price as u32;
                let units = (trading_opp
                    .purchase_market_trade_good_entry
                    .trade_volume
                    .min(trading_opp.sell_market_trade_good_entry.trade_volume) as u32)
                    .min(num_units_within_budget)
                    .min(ship_cargo_space);

                let total_profit = trading_opp.profit_per_unit * units as u64;
                let profit_per_distance = (total_profit as f64 / total_distance as f64) as u64;

                (units > 0).then_some(EvaluatedTradingOpportunity {
                    ship_symbol: ship.symbol.clone(),
                    distance_to_start,
                    total_distance,
                    total_profit,
                    profit_per_distance_unit: profit_per_distance,
                    units,
                    trading_opportunity: (*trading_opp).clone(),
                })
            })
        })
        .sorted_by_key(|ev| -(ev.profit_per_distance_unit as i64))
        .collect_vec();

    evaluated_trading_opportunities
}

// This is computationally expensive for many ships/routes but will find the optimal solution
pub fn find_optimal_trading_routes_exhaustive(
    trading_options: &[EvaluatedTradingOpportunity],
    active_trade_routes: &HashSet<ActiveTradeRoute>,
) -> Vec<EvaluatedTradingOpportunity> {
    // Create a unique identifier for each trading route
    let route_key_fn = |option: &EvaluatedTradingOpportunity| -> (WaypointSymbol, WaypointSymbol, TradeGoodSymbol) {
        (
            option.trading_opportunity.purchase_waypoint_symbol.clone(),
            option.trading_opportunity.sell_waypoint_symbol.clone(),
            option
                .trading_opportunity
                .purchase_market_trade_good_entry
                .symbol
                .clone(),
        )
    };

    // Group options by ship
    let ship_options: HashMap<ShipSymbol, Vec<EvaluatedTradingOpportunity>> = trading_options
        .iter()
        .cloned()
        .into_group_map_by(|option| option.ship_symbol.clone());

    let ships: Vec<ShipSymbol> = ship_options.keys().cloned().collect();
    let num_ships = ships.len();

    // For each ship, we need to select one trading option
    // We'll try all valid permutations and keep the best one

    // This is a simplified approach that works for a small number of ships
    // For larger numbers, you would need a more sophisticated algorithm

    let mut best_assignments: Vec<EvaluatedTradingOpportunity> = Vec::new();
    let mut best_profit = 0;

    // For each possible assignment of ships to positions
    for ship_perm in ships.iter().permutations(num_ships) {
        // Try to assign each ship to its best available route
        let mut current_assignments: Vec<EvaluatedTradingOpportunity> = Vec::new();
        let mut assigned_routes: HashSet<(WaypointSymbol, WaypointSymbol, TradeGoodSymbol)> = active_trade_routes
            .iter()
            .map(|r| (r.from.clone(), r.to.clone(), r.trade_good.clone()))
            .collect();
        let mut valid_assignment = true;

        for ship in ship_perm {
            if let Some(ship_opts) = ship_options.get(ship) {
                let mut assigned = false;

                // Sort options by profit per distance unit (descending)
                let mut sorted_options = ship_opts.clone();
                sorted_options.sort_by(|a, b| b.profit_per_distance_unit.cmp(&a.profit_per_distance_unit));

                // Find the best unassigned route
                for option in sorted_options {
                    let key = route_key_fn(&option);
                    if !assigned_routes.contains(&key) {
                        current_assignments.push(option.clone());
                        assigned_routes.insert(key);
                        assigned = true;
                        break;
                    }
                }

                if !assigned {
                    valid_assignment = false;
                    break;
                }
            }
        }

        if valid_assignment {
            let current_profit = calculate_total_profit(&current_assignments);
            if current_profit > best_profit {
                best_profit = current_profit;
                best_assignments = current_assignments;
            }
        }
    }

    if best_assignments.is_empty() {
        eprintln!("best_assignments should not be empty. Taking best trade per ship that's not already taken");

        let alternative_best_per_ship = ship_options
            .iter()
            .filter_map(|(_, opportunities_of_ship)| {
                opportunities_of_ship
                    .iter()
                    .max_by_key(|ev| ev.profit_per_distance_unit)
            })
            .cloned()
            .collect_vec();

        alternative_best_per_ship
    } else {
        best_assignments
    }
}

// Function to calculate total profit across all assigned routes
fn calculate_total_profit(assignments: &[EvaluatedTradingOpportunity]) -> u64 {
    assignments
        .iter()
        .map(|option| option.profit_per_distance_unit)
        .sum()
}

pub fn group_markets_by_type(
    market_data: &[(WaypointSymbol, Vec<MarketTradeGood>)],
    trade_good_type: TradeGoodType,
) -> HashMap<TradeGoodSymbol, Vec<(WaypointSymbol, MarketTradeGood)>> {
    market_data
        .iter()
        .flat_map(|(wps, entries)| {
            entries
                .iter()
                .filter(|mtg| mtg.trade_good_type == trade_good_type)
                .map(|mtg| (mtg.symbol.clone(), (wps.clone(), mtg.clone())))
        })
        .into_group_map()
}
