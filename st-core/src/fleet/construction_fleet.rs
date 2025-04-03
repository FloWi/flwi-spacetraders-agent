use crate::fleet::fleet::FleetAdmiral;
use anyhow::*;
use itertools::Itertools;
use st_domain::{
    trading, ConstructJumpGateFleetConfig, EvaluatedTradingOpportunity, Fleet, FleetDecisionFacts, LabelledCoordinate, PurchaseShipTicketDetails, Ship,
    ShipSymbol, ShipTask, TicketId, TransactionTicketId, Waypoint,
};
use st_store::shipyard_bmc::ShipyardBmc;
use st_store::{Ctx, DbModelManager, MarketBmc, SystemBmc};
use std::collections::{HashMap, HashSet};
use std::ops::Not;
use uuid::Uuid;

pub struct ConstructJumpGateFleet;

impl ConstructJumpGateFleet {
    pub async fn compute_ship_tasks(
        admiral: &mut FleetAdmiral,
        cfg: &ConstructJumpGateFleetConfig,
        fleet: &Fleet,
        facts: &FleetDecisionFacts,
        mm: &DbModelManager,
    ) -> Result<HashMap<ShipSymbol, ShipTask>> {
        let ships: Vec<&Ship> = admiral.get_ships_of_fleet(fleet);

        let budget: u64 = admiral.get_total_budget_for_fleet(fleet);
        let my_ships = admiral.get_ships_of_fleet(fleet);
        let ship_tasks: Vec<(ShipSymbol, ShipTask)> = admiral.get_ship_tasks_of_fleet(fleet);

        let allocated_budget: u64 = admiral.get_allocated_budget_of_fleet(fleet);

        let ships_with_tasks = ship_tasks.iter().map(|(ss, _)| ss.clone()).collect::<HashSet<_>>();
        let unassigned_ships = ships.into_iter().filter(|s| ships_with_tasks.contains(&s.symbol).not()).collect_vec();

        // all ships are traders (command frigate + 4 haulers --> 200k each => 1M total)
        let reserved_for_trading = (50_000 * unassigned_ships.len()) as i64;
        let rest_budget = (budget as i64) - allocated_budget as i64;

        let budget_for_ship_purchase = rest_budget - reserved_for_trading;

        // if we have enough budget for purchasing construction material, we do so

        let waypoints = SystemBmc::get_waypoints_of_system(&Ctx::Anonymous, mm, &cfg.system_symbol).await?;
        let waypoint_map = waypoints.iter().map(|wp| (wp.symbol.clone(), wp)).collect::<HashMap<_, _>>();

        let maybe_next_ship_to_purchase = admiral.get_next_ship_purchase();

        let maybe_ship_purchase_location = match maybe_next_ship_to_purchase.clone() {
            None => None,
            Some(ship_type) => {
                let ship_prices = ShipyardBmc::get_latest_ship_prices(&Ctx::Anonymous, mm, &cfg.system_symbol).await?;
                let maybe_ship_purchase_location = ship_prices
                    .price_infos
                    .iter()
                    .flat_map(|(wps, shipyard_ships)| {
                        shipyard_ships.iter().filter_map(|shipyard_ship| (shipyard_ship.r#type == ship_type).then_some((wps.clone(), shipyard_ship.clone())))
                    })
                    .sorted_by_key(|(_, s)| s.purchase_price as i64)
                    .filter(|(_, s)| s.purchase_price as i64 <= budget_for_ship_purchase)
                    .take(1)
                    .next();

                maybe_ship_purchase_location
            }
        };

        let maybe_ship_purchase_ticket_details = maybe_ship_purchase_location.clone().and_then(|(wps, s)| {
            let shipyard_waypoint = waypoint_map.get(&wps.clone()).expect("Waypoint of shipyard");

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
                    assigned_fleet_id: fleet.id.clone(),
                    is_complete: false,
                }),
            }
        });

        let unassigned_ships = unassigned_ships
            .into_iter()
            .filter(|s| {
                let is_ship_assigned_for_ship_purchase = maybe_ship_purchase_ticket_details.clone().map(|t| t.ship_symbol == s.symbol).unwrap_or(false);
                is_ship_assigned_for_ship_purchase.not()
            })
            .collect_vec();

        let unassigned_ships_symbols = unassigned_ships.iter().map(|s| s.symbol.clone()).collect_vec();

        let latest_market_data = MarketBmc::get_latest_market_data_for_system(&Ctx::Anonymous, mm, &cfg.system_symbol).await?;
        let market_data = trading::to_trade_goods_with_locations(&latest_market_data);
        let trading_opportunities = trading::find_trading_opportunities(&market_data, &waypoint_map);
        let evaluated_trading_opportunities: Vec<EvaluatedTradingOpportunity> =
            trading::evaluate_trading_opportunities(&unassigned_ships, &waypoint_map, trading_opportunities);

        // FIXME: get currently active trades
        let active_trades_of_goods: Vec<EvaluatedTradingOpportunity> = Vec::new();
        let trades_for_ships = trading::find_optimal_trading_routes_exhaustive(&evaluated_trading_opportunities, &active_trades_of_goods);

        // agent has 175_000
        // fleet has budget of 175_000
        // currently allocated are 75_000
        // one idle trader
        // needs probe for 20k
        // required_for_trading = 50k * 1 = 50k
        // rest_budget = 175k - 75k = 75k
        // can_purchase_ship = rest_budget - ship_price > required_for_trading ==> true

        dbg!(budget);
        dbg!(ship_tasks);
        dbg!(allocated_budget);
        dbg!(ships_with_tasks);
        dbg!(unassigned_ships_symbols);
        dbg!(reserved_for_trading);
        dbg!(rest_budget);
        //dbg!(evaluated_trading_opportunities);
        dbg!(&trades_for_ships);
        dbg!(&admiral.fleet_phase);

        dbg!(&maybe_next_ship_to_purchase);
        dbg!(&reserved_for_trading);
        dbg!(&rest_budget);
        dbg!(&budget_for_ship_purchase);
        dbg!(&maybe_ship_purchase_location);
        dbg!(&maybe_ship_purchase_ticket_details);

        admiral.assign_trading_tickets_if_possible(&trades_for_ships);

        if let Some(ticket_details) = maybe_ship_purchase_ticket_details {
            admiral.assign_ship_purchase_ticket_if_possible(&ticket_details);
        }

        println!("Assigned trades to ships: \n{}", serde_json::to_string_pretty(&admiral.active_trades)?);

        // TradingManager::acquire_trading_tickets(trading_tickets, admiral);

        Ok(Default::default())
    }
}

struct TradingManager;
