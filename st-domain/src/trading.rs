use crate::{
    EvaluatedTradingOpportunity, LabelledCoordinate, MarketData, MarketTradeGood, Ship, ShipSymbol, TradeGoodType, TradingOpportunity, Waypoint, WaypointSymbol,
};
use itertools::Itertools;
use std::collections::{HashMap, HashSet};

pub fn find_trading_opportunities(
    market_data: &[(WaypointSymbol, Vec<MarketTradeGood>)],
    waypoint_map: &HashMap<WaypointSymbol, &Waypoint>,
) -> Vec<TradingOpportunity> {
    let denormalized_trade_goods_with_wp_symbols =
        market_data.iter().flat_map(|(wp_symbol, market_trade_goods)| market_trade_goods.iter().map(|mtg| (wp_symbol.clone(), mtg.clone()))).collect_vec();
    let exports = denormalized_trade_goods_with_wp_symbols
        .iter()
        .filter(|(wp_sym, mtg)| mtg.trade_good_type == TradeGoodType::Export || mtg.trade_good_type == TradeGoodType::Exchange)
        .collect_vec();
    let imports = denormalized_trade_goods_with_wp_symbols
        .iter()
        .filter(|(wp_sym, mtg)| mtg.trade_good_type == TradeGoodType::Import || mtg.trade_good_type == TradeGoodType::Exchange)
        .collect_vec();

    let trades_by_profit = exports
        .iter()
        .flat_map(|(export_wps, export_mtg)| {
            let export_wp = waypoint_map.get(export_wps).unwrap();
            imports
                .iter()
                .filter(move |(import_wps, import_mtg)| {
                    export_wps != import_wps && export_mtg.symbol == import_mtg.symbol && import_mtg.sell_price > export_mtg.purchase_price
                })
                .map(|(import_wps, import_mtg)| {
                    let import_wp = waypoint_map.get(import_wps).unwrap();
                    TradingOpportunity {
                        purchase_waypoint_symbol: export_wps.clone(),
                        purchase_market_trade_good_entry: export_mtg.clone(),
                        sell_waypoint_symbol: import_wps.clone(),
                        sell_market_trade_good_entry: import_mtg.clone(),
                        direct_distance: import_wp.distance_to(export_wp),
                        profit_per_unit: (import_mtg.sell_price - export_mtg.purchase_price) as u64,
                    }
                })
        })
        .sorted_by_key(|trading_opp| trading_opp.profit_per_unit)
        .rev()
        .collect_vec();

    trades_by_profit
}

pub fn to_trade_goods_with_locations(market_data: &Vec<MarketData>) -> Vec<(WaypointSymbol, Vec<MarketTradeGood>)> {
    market_data
        .iter()
        .filter_map(|md| match &md.trade_goods {
            None => None,
            Some(trade_goods) => Some((md.symbol.clone(), trade_goods.clone())),
        })
        .collect_vec()
}

pub fn evaluate_trading_opportunities(
    unassigned_ships: &[&Ship],
    waypoint_map: &HashMap<WaypointSymbol, &Waypoint>,
    trading_opportunities: Vec<TradingOpportunity>,
    budget_for_trading: i64,
) -> Vec<EvaluatedTradingOpportunity> {
    let top_trading_opps = trading_opportunities.iter().sorted_by_key(|t| -(t.profit_per_unit as i64)).take(15).collect_vec();

    let budget_for_ship = if unassigned_ships.is_empty() {
        0
    } else {
        u64::try_from(budget_for_trading).unwrap_or(0) / unassigned_ships.len() as u64
    };

    let evaluated_trading_opportunities = unassigned_ships
        .iter()
        .flat_map(|ship| {
            let ship_wp = waypoint_map.get(&ship.nav.waypoint_symbol).unwrap();

            top_trading_opps.iter().map(|trading_opp| {
                let purchase_wp = waypoint_map.get(&trading_opp.purchase_waypoint_symbol).unwrap();
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
                let units = (trading_opp.purchase_market_trade_good_entry.trade_volume.min(trading_opp.sell_market_trade_good_entry.trade_volume) as u32)
                    .min(num_units_within_budget);

                /*
                "trade_good": "CLOTHING",
                "ship_symbol": "FLWI-1",
                "price_per_unit": 5125,
                "waypoint_symbol": "X1-FN42-D48"

                       */
                println!(
                    r#"
trade_good: {}
purchase_waypoint: {}
sell_waypoint: {}
let num_units_within_budget = budget_for_ship as u32 / trading_opp.purchase_market_trade_good_entry.purchase_price as u32;
let units = (trading_opp.purchase_market_trade_good_entry.trade_volume.min(trading_opp.sell_market_trade_good_entry.trade_volume) as u32)
            .min(num_units_within_budget);
let {} = {} / {};
let units = ({}.min({}) as u32)
            .min({});
                "#,
                    trading_opp.purchase_market_trade_good_entry.symbol.to_string(),
                    trading_opp.purchase_waypoint_symbol.0,
                    trading_opp.sell_waypoint_symbol.0,
                    num_units_within_budget,
                    budget_for_ship as u32,
                    trading_opp.purchase_market_trade_good_entry.purchase_price as u32,
                    trading_opp.purchase_market_trade_good_entry.trade_volume,
                    trading_opp.sell_market_trade_good_entry.trade_volume,
                    num_units_within_budget
                );

                let total_profit = trading_opp.profit_per_unit * units as u64;
                let profit_per_distance = (total_profit as f64 / total_distance as f64) as u64;

                EvaluatedTradingOpportunity {
                    ship_symbol: ship.symbol.clone(),
                    distance_to_start,
                    total_distance,
                    total_profit,
                    profit_per_distance_unit: profit_per_distance,
                    units,
                    trading_opportunity: (*trading_opp).clone(),
                }
            })
        })
        .sorted_by_key(|ev| -(ev.profit_per_distance_unit as i64))
        .collect_vec();

    evaluated_trading_opportunities
}

// This is computationally expensive for many ships/routes but will find the optimal solution
pub fn find_optimal_trading_routes_exhaustive(
    options: &[EvaluatedTradingOpportunity],
    active_trades: &[EvaluatedTradingOpportunity],
) -> Vec<EvaluatedTradingOpportunity> {
    // Create a unique identifier for each trading route
    let route_key_fn = |option: &EvaluatedTradingOpportunity| -> String {
        format!(
            "{}_{}_{}",
            option.trading_opportunity.purchase_waypoint_symbol.0,
            option.trading_opportunity.sell_waypoint_symbol.0,
            option.trading_opportunity.purchase_market_trade_good_entry.symbol
        )
    };

    // Group options by ship
    let ship_options: HashMap<ShipSymbol, Vec<EvaluatedTradingOpportunity>> = options.iter().cloned().into_group_map_by(|option| option.ship_symbol.clone());

    let ships: Vec<ShipSymbol> = ship_options.keys().cloned().collect();
    let num_ships = ships.len();

    // For each ship, we need to select one trading option
    // We'll try all valid permutations and keep the best one

    // This is a simplified approach that works for a small number of ships
    // For larger numbers, you would need a more sophisticated algorithm

    let mut best_assignments: Vec<EvaluatedTradingOpportunity> = Vec::new();
    let mut best_profit = 0;

    let active_assigned_routes: HashSet<String> = active_trades.iter().map(route_key_fn).collect();

    // For each possible assignment of ships to positions
    for ship_perm in ships.iter().permutations(num_ships) {
        // Try to assign each ship to its best available route
        let mut current_assignments: Vec<EvaluatedTradingOpportunity> = Vec::new();
        let mut assigned_routes: HashSet<String> = active_assigned_routes.clone();
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

    best_assignments
}

// Function to calculate total profit across all assigned routes
fn calculate_total_profit(assignments: &[EvaluatedTradingOpportunity]) -> u64 {
    assignments.iter().map(|option| option.profit_per_distance_unit).sum()
}
