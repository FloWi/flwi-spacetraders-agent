use crate::fleet::fleet::FleetAdmiral;
use anyhow::*;
use itertools::Itertools;
use st_domain::{
    trading, ConstructJumpGateFleetConfig, EvaluatedTradingOpportunity, Fleet, FleetDecisionFacts, LabelledCoordinate, MarketEntry, PurchaseGoodTicketDetails,
    PurchaseShipTicketDetails, SellGoodTicketDetails, Ship, ShipPriceInfo, ShipSymbol, ShipTask, TicketId, TradeTicket, TransactionTicketId, Waypoint,
    WaypointSymbol,
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

        Ok(Vec::new())
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
            first_purchase_location: opp.trading_opportunity.purchase_waypoint_symbol.clone(),
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
        first_purchase_location: details.waypoint_symbol.clone(),
        ship_task: ShipTask::Trade { ticket_id: ticket.ticket_id() },
    }
}

pub struct PotentialTradingTask {
    pub ship_symbol: ShipSymbol,
    pub trade_ticket: TradeTicket,
    pub ship_task: ShipTask,
    pub first_purchase_location: WaypointSymbol,
}
