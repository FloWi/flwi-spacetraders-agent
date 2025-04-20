use crate::fleet::fleet::FleetAdmiral;
use anyhow::*;
use itertools::Itertools;
use st_domain::{
    trading, ConstructJumpGateFleetConfig, EvaluatedTradingOpportunity, Fleet, FleetDecisionFacts, LabelledCoordinate, MarketEntry, PurchaseGoodTicketDetails,
    PurchaseShipTicketDetails, SellGoodTicketDetails, Ship, ShipPriceInfo, ShipSymbol, ShipTask, TicketId, TradeTicket, TransactionTicketId, Waypoint,
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
        latest_market_data: &Vec<MarketEntry>,
        ship_prices: &ShipPriceInfo,
        waypoints: &Vec<Waypoint>,
    ) -> Result<Vec<PotentialTradingTask>> {
        let fleet_ships: Vec<&Ship> = admiral.get_ships_of_fleet(fleet);
        let fleet_ship_symbols = fleet_ships.iter().map(|&s| s.symbol.clone()).collect_vec();
        let budget: u64 = admiral.get_total_budget_for_fleet(fleet);
        let ship_tasks: Vec<(ShipSymbol, ShipTask)> = admiral.get_ship_tasks_of_fleet(fleet);

        let allocated_budget: u64 = admiral.get_allocated_budget_of_fleet(fleet);

        let ships_with_tasks = ship_tasks.iter().map(|(ss, _)| ss.clone()).collect::<HashSet<_>>();

        let unassigned_ships = fleet_ships.into_iter().filter(|s| ships_with_tasks.contains(&s.symbol).not()).collect_vec();
        let initial_unassigned_ships = unassigned_ships.iter().map(|s| s.symbol.clone()).collect_vec();

        // all ships are traders (command frigate + 4 haulers --> 200k each => 1M total)
        let reserved_for_trading = (50_000 * unassigned_ships.len()) as i64;

        let not_allocated_budget = (budget as i64) - allocated_budget as i64;

        let trading_budget = reserved_for_trading.min(not_allocated_budget);

        let budget_for_ship_purchase = not_allocated_budget - trading_budget;

        // if we have enough budget for purchasing construction material, we do so

        let waypoint_map = waypoints.iter().map(|wp| (wp.symbol.clone(), wp)).collect::<HashMap<_, _>>();

        let maybe_next_ship_to_purchase = admiral.get_next_ship_purchase();

        let maybe_ship_purchase_location = match maybe_next_ship_to_purchase.clone() {
            None => None,
            Some((ship_type, assigned_fleet_task)) => {
                let maybe_ship_purchase_location = ship_prices
                    .price_infos
                    .iter()
                    .flat_map(|(wps, shipyard_ships)| {
                        shipyard_ships.iter().filter_map(|shipyard_ship| {
                            (shipyard_ship.r#type == ship_type).then_some((wps.clone(), shipyard_ship.clone(), assigned_fleet_task.clone()))
                        })
                    })
                    .sorted_by_key(|(_, s, _)| s.purchase_price as i64)
                    .filter(|(_, s, _)| s.purchase_price as i64 <= budget_for_ship_purchase)
                    .take(1)
                    .next();

                maybe_ship_purchase_location
            }
        };

        let maybe_ship_purchase_ticket_details: Option<PurchaseShipTicketDetails> =
            maybe_ship_purchase_location.clone().and_then(|(wps, s, assigned_fleet_task)| {
                let shipyard_waypoint = waypoint_map.get(&wps.clone()).expect("Waypoint of shipyard");

                let assigned_fleet_id = admiral
                    .fleet_tasks
                    .iter()
                    .find_map(|(fleet_id, tasks)| (tasks.iter().any(|t| t == &assigned_fleet_task)).then_some(fleet_id.clone()))
                    .expect("ship purchase should have an assigned_fleet_id");

                let maybe_closest_ship: Option<(ShipSymbol, u32)> = unassigned_ships
                    .iter()
                    .map(|s| {
                        let ship_wp = waypoint_map.get(&s.nav.waypoint_symbol).expect("Ship Waypoint");
                        let distance_to_shipyard = ship_wp.distance_to(shipyard_waypoint);
                        (s.symbol.clone(), distance_to_shipyard)
                    })
                    .sorted_by_key(|(_, distance)| *distance)
                    .next();

                match maybe_closest_ship {
                    None => None,
                    Some((closest_ship_symbol, _distance)) => Some(PurchaseShipTicketDetails {
                        id: TransactionTicketId(Uuid::new_v4()),
                        ship_symbol: closest_ship_symbol,
                        waypoint_symbol: wps,
                        ship_type: s.r#type,
                        price: s.purchase_price as u64,
                        allocated_credits: s.purchase_price as u64,
                        assigned_fleet_id,
                        is_complete: false,
                    }),
                }
            });

        // TODO: allow more trading budget if now ship gets purchased
        let _budget_used_for_ship_purchase = maybe_ship_purchase_ticket_details.clone().map(|t| t.allocated_credits).unwrap_or(0);
        let budget_for_trading = if reserved_for_trading < 0 {
            0
        } else {
            reserved_for_trading as u64
        };

        let still_unassigned_ships = unassigned_ships
            .into_iter()
            .filter(|s| {
                let is_ship_assigned_for_ship_purchase = maybe_ship_purchase_ticket_details.clone().map(|t| t.ship_symbol == s.symbol).unwrap_or(false);
                is_ship_assigned_for_ship_purchase.not()
            })
            .collect_vec();

        let still_unassigned_ships_symbols = still_unassigned_ships.iter().map(|s| s.symbol.clone()).collect_vec();

        let market_data = trading::to_trade_goods_with_locations(latest_market_data);
        let trading_opportunities = trading::find_trading_opportunities(&market_data, &waypoint_map);
        let evaluated_trading_opportunities: Vec<EvaluatedTradingOpportunity> =
            trading::evaluate_trading_opportunities(&still_unassigned_ships, &waypoint_map, trading_opportunities, trading_budget);

        // FIXME: get currently active trades
        let active_trades_of_goods: Vec<EvaluatedTradingOpportunity> = Vec::new();
        let trades_for_ships: Vec<EvaluatedTradingOpportunity> =
            trading::find_optimal_trading_routes_exhaustive(&evaluated_trading_opportunities, &active_trades_of_goods);

        // agent has 175_000
        // fleet has budget of 175_000
        // currently allocated are 75_000
        // one idle trader
        // needs probe for 20k
        // required_for_trading = 50k * 1 = 50k
        // rest_budget = 175k - 75k = 75k
        // can_purchase_ship = rest_budget - ship_price > required_for_trading ==> true

        let trading_tasks_with_trading_tickets = create_trading_tickets(&trades_for_ships);
        let ship_purchase_tasks_with_trading_ticket = match maybe_ship_purchase_ticket_details {
            Some(ticket_details) => vec![create_ship_purchase_ticket(ticket_details)],
            None => vec![],
        };

        // ship purchases first
        let tasks_with_tickets = ship_purchase_tasks_with_trading_ticket
            .into_iter()
            .chain(trading_tasks_with_trading_tickets)
            .unique_by(|ptt| ptt.ship_symbol.clone())
            .collect_vec();

        if tasks_with_tickets.is_empty() {
            // dbg!(&budget);
            // dbg!(&fleet_ship_symbols);
            // dbg!(&ship_tasks);
            // dbg!(&allocated_budget);
            // dbg!(&ships_with_tasks);
            // dbg!(&still_unassigned_ships_symbols);
            // dbg!(&reserved_for_trading);
            // dbg!(&not_allocated_budget);
            // dbg!(evaluated_trading_opportunities);
            // dbg!(&trades_for_ships);
            // dbg!(&admiral.fleet_phase);
            //
            // dbg!(&maybe_next_ship_to_purchase);
            // dbg!(&reserved_for_trading);
            // dbg!(&not_allocated_budget);
            // dbg!(&budget_for_ship_purchase);
            // dbg!(&maybe_ship_purchase_location);
            Ok(tasks_with_tickets)
        } else {
            Ok(tasks_with_tickets)
        }
    }
}

pub fn create_trading_tickets(trading_opportunities_within_budget: &[EvaluatedTradingOpportunity]) -> Vec<PotentialTradingTask> {
    let mut new_tasks_with_tickets = Vec::new();
    for opp in trading_opportunities_within_budget.iter() {
        let ticket = TradeTicket::TradeCargo {
            ticket_id: TicketId::new(),
            purchase_completion_status: vec![(PurchaseGoodTicketDetails::from_trading_opportunity(opp), false)],
            sale_completion_status: vec![(SellGoodTicketDetails::from_trading_opportunity(opp), false)],
            evaluation_result: vec![opp.clone()],
        };
        new_tasks_with_tickets.push(PotentialTradingTask {
            ship_symbol: opp.ship_symbol.clone(),
            trade_ticket: ticket.clone(),
            ship_task: ShipTask::Trade { ticket_id: ticket.ticket_id() },
        });
    }
    new_tasks_with_tickets
}

pub fn create_ship_purchase_ticket(details: PurchaseShipTicketDetails) -> PotentialTradingTask {
    let ticket = TradeTicket::PurchaseShipTicket {
        ticket_id: TicketId::new(),
        details: details.clone(),
    };
    PotentialTradingTask {
        ship_symbol: details.ship_symbol.clone(),
        trade_ticket: ticket.clone(),
        ship_task: ShipTask::Trade { ticket_id: ticket.ticket_id() },
    }
}

pub struct PotentialTradingTask {
    pub ship_symbol: ShipSymbol,
    pub trade_ticket: TradeTicket,
    pub ship_task: ShipTask,
}
