use crate::fleet::fleet::FleetAdmiral;
use anyhow::*;
use itertools::Itertools;
use st_domain::budgeting::budgeting::{FleetBudget, PurchaseTradeGoodsTransactionGoal, SellTradeGoodsTransactionGoal, TransactionGoal, TransactionTicket};
use st_domain::budgeting::credits::Credits;
use st_domain::{
    trading, ConstructJumpGateFleetConfig, EvaluatedTradingOpportunity, Fleet, FleetDecisionFacts, LabelledCoordinate, MarketEntry, MarketTradeGood,
    PurchaseGoodTicketDetails, PurchaseShipTicketDetails, SellGoodTicketDetails, Ship, ShipPriceInfo, ShipSymbol, ShipTask, TicketId, TradeGoodSymbol,
    TradeTicket, TransactionTicketId, Waypoint, WaypointSymbol,
};
use std::collections::{HashMap, HashSet};
use std::ops::Not;
use uuid::Uuid;

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
        active_trades_of_fleet: &Vec<TransactionTicket>,
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

        let market_data: Vec<(WaypointSymbol, Vec<MarketTradeGood>)> = trading::to_trade_goods_with_locations(&latest_market_entries);
        let trading_opportunities = trading::find_trading_opportunities_sorted_by_profit_per_distance_unit(&market_data, &waypoint_map);

        let evaluated_trading_opportunities = trading::evaluate_trading_opportunities(
            unassigned_ships_of_fleet,
            &waypoint_map,
            &trading_opportunities,
            fleet_budget.available_capital.0,
        );

        let active_trade_routes: HashSet<(WaypointSymbol, WaypointSymbol, TradeGoodSymbol)> = active_trades_of_fleet
            .iter()
            .filter_map(|t| {
                let maybe_purchase = t.goals.iter().find_map(|goal| match goal {
                    TransactionGoal::PurchaseTradeGoods(p) => Some((p.good.clone(), p.source_waypoint.clone())),
                    TransactionGoal::SellTradeGoods(_) => None,
                    TransactionGoal::PurchaseShip(_) => None,
                });

                let maybe_sell_wp = t.goals.iter().find_map(|goal| match goal {
                    TransactionGoal::PurchaseTradeGoods(_) => None,
                    TransactionGoal::SellTradeGoods(s) => Some(s.destination_waypoint.clone()),
                    TransactionGoal::PurchaseShip(_) => None,
                });

                maybe_purchase
                    .zip(maybe_sell_wp)
                    .map(|((good, purchase_wp), sell_wp)| (purchase_wp, sell_wp, good))
            })
            .collect::<HashSet<_>>();

        // FIXME: only allow one trade per route

        let best_new_trading_opportunities: Vec<EvaluatedTradingOpportunity> =
            trading::find_optimal_trading_routes_exhaustive(&evaluated_trading_opportunities, &active_trade_routes);

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

    pub(crate) fn to_trading_goals(&self) -> Vec<TransactionGoal> {
        let ev = &self.evaluation_result;
        let opp = &ev.trading_opportunity;
        let trade_good = opp.purchase_market_trade_good_entry.symbol.clone();
        vec![
            TransactionGoal::PurchaseTradeGoods(PurchaseTradeGoodsTransactionGoal {
                id: TransactionTicketId::new(),
                good: trade_good.clone(),
                target_quantity: ev.units,
                available_quantity: Some(ev.units),
                acquired_quantity: 0,
                estimated_price_per_unit: Credits(opp.purchase_market_trade_good_entry.purchase_price as i64),
                max_acceptable_price_per_unit: Some(Credits(opp.purchase_market_trade_good_entry.purchase_price as i64)),
                source_waypoint: opp.purchase_waypoint_symbol.clone(),
            }),
            TransactionGoal::SellTradeGoods(SellTradeGoodsTransactionGoal {
                id: TransactionTicketId::new(),
                good: trade_good,
                target_quantity: ev.units,
                sold_quantity: 0,
                estimated_price_per_unit: Credits(opp.sell_market_trade_good_entry.sell_price as i64),
                min_acceptable_price_per_unit: None,
                destination_waypoint: opp.sell_waypoint_symbol.clone(),
            }),
        ]
    }
}
