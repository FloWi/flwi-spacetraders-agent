use crate::fleet::fleet::FleetAdmiral;
use anyhow::*;
use itertools::Itertools;
use st_domain::budgeting::credits::Credits;
use st_domain::budgeting::treasury_redesign::{ActiveTradeRoute, FleetBudget};
use st_domain::{
    trading, ConstructJumpGateFleetConfig, EvaluatedTradingOpportunity, Fleet, FleetDecisionFacts, MarketEntry, MarketTradeGood, Ship, ShipPriceInfo,
    ShipSymbol, Waypoint, WaypointSymbol,
};
use std::collections::{HashMap, HashSet};

pub struct ConstructJumpGateFleet;

impl ConstructJumpGateFleet {
    pub fn compute_ship_tasks(
        admiral: &FleetAdmiral,
        cfg: &ConstructJumpGateFleetConfig,
        fleet: &Fleet,
        facts: &FleetDecisionFacts,
        latest_market_entries: &Vec<MarketEntry>,
        ship_prices: &ShipPriceInfo,
        waypoints: &Vec<Waypoint>,
        unassigned_ships_of_fleet: &[&Ship],
        active_trade_routes: &HashSet<ActiveTradeRoute>,
        fleet_budget: &FleetBudget,
    ) -> Result<Vec<PotentialTradingTask>> {
        let fleet_ships: Vec<&Ship> = admiral.get_ships_of_fleet(fleet);
        let fleet_ship_symbols = fleet_ships.iter().map(|&s| s.symbol.clone()).collect_vec();

        // println!("facts:\n{}", serde_json::to_string(&facts)?);
        // println!("latest_market_data: {}", serde_json::to_string(&latest_market_data)?);

        if unassigned_ships_of_fleet.is_empty() {
            return Ok(vec![]);
        }

        let waypoint_map: HashMap<WaypointSymbol, &Waypoint> = waypoints
            .iter()
            .map(|wp| (wp.symbol.clone(), wp))
            .collect::<HashMap<_, _>>();

        let market_data: Vec<(WaypointSymbol, Vec<MarketTradeGood>)> = trading::to_trade_goods_with_locations(latest_market_entries);
        let trading_opportunities = trading::find_trading_opportunities_sorted_by_profit_per_distance_unit(&market_data, &waypoint_map);

        let evaluated_trading_opportunities = trading::evaluate_trading_opportunities(
            unassigned_ships_of_fleet,
            &waypoint_map,
            &trading_opportunities,
            fleet_budget.available_capital().0,
        );

        // FIXME: only allow one trade per route

        let best_new_trading_opportunities: Vec<EvaluatedTradingOpportunity> =
            trading::find_optimal_trading_routes_exhaustive(&evaluated_trading_opportunities, active_trade_routes);

        let new_tasks = create_trading_tickets(&best_new_trading_opportunities);

        if new_tasks.is_empty() {
            println!("Hello, breakpoint")
        }

        Ok(new_tasks)
    }
}

pub fn create_trading_tickets(trading_opportunities_within_budget: &[EvaluatedTradingOpportunity]) -> Vec<PotentialTradingTask> {
    let mut new_tasks_with_tickets = Vec::new();
    for opp in trading_opportunities_within_budget.iter() {
        new_tasks_with_tickets.push(PotentialTradingTask {
            ship_symbol: opp.ship_symbol.clone(),
            evaluation_result: opp.clone(),
            first_purchase_location: opp.trading_opportunity.purchase_waypoint_symbol.clone(),
        });
    }
    new_tasks_with_tickets
}

pub struct PotentialTradingTask {
    pub ship_symbol: ShipSymbol,
    pub evaluation_result: EvaluatedTradingOpportunity,
    pub first_purchase_location: WaypointSymbol,
}

impl PotentialTradingTask {
    pub(crate) fn total_purchase_price(&self) -> Credits {
        let ev = &self.evaluation_result;
        let opp = &ev.trading_opportunity;
        let total_purchase_price = (opp.purchase_market_trade_good_entry.purchase_price * ev.units as i32) as i64;
        total_purchase_price.into()
    }
}
