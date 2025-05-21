use crate::fleet::construction_fleet::ConstructionFleetAction::{BoostSupplyChain, DeliverConstructionMaterials, TradeProfitably};
use crate::fleet::fleet::FleetAdmiral;
use anyhow::*;
use itertools::Itertools;
use st_domain::budgeting::credits::Credits;
use st_domain::budgeting::treasury_redesign::FinanceTicketDetails::SellTradeGoods;
use st_domain::budgeting::treasury_redesign::{
    ActiveTradeRoute, DeliverConstructionMaterialsTicketDetails, FinanceTicketDetails, FleetBudget, PurchaseTradeGoodsTicketDetails,
    SellTradeGoodsTicketDetails,
};
use st_domain::{
    calc_scored_supply_chain_routes, trading, Cargo, ConstructJumpGateFleetConfig, EvaluatedTradingOpportunity, Fleet, FleetDecisionFacts, FleetId,
    LabelledCoordinate, MarketEntry, MarketTradeGood, ScoredSupplyChainSupportRoute, Ship, ShipPriceInfo, ShipSymbol, SupplyLevel, TradeGoodSymbol,
    TradeGoodType, Waypoint, WaypointSymbol,
};
use std::collections::{HashMap, HashSet};
use std::ops::Not;

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
    ) -> Result<Vec<PotentialConstructionTask>> {
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

        let best_new_trading_opportunities: Vec<EvaluatedTradingOpportunity> =
            trading::find_optimal_trading_routes_exhaustive(&evaluated_trading_opportunities, active_trade_routes);

        let new_tasks = if admiral.ship_purchase_demand.is_empty() {
            println!("Starting construction");
            let best_actions_for_ships = determine_construction_fleet_actions(
                admiral,
                facts,
                &fleet.id,
                latest_market_entries,
                ship_prices,
                &waypoint_map,
                unassigned_ships_of_fleet,
                active_trade_routes,
                fleet_budget,
                &best_new_trading_opportunities,
            )?;

            best_actions_for_ships
                .into_iter()
                .map(|(ship_symbol, task)| PotentialConstructionTask { ship_symbol, task })
                .collect()
        } else {
            create_trading_tickets(&best_new_trading_opportunities)
        };

        Ok(new_tasks)
    }
}

pub fn create_trading_tickets(trading_opportunities_within_budget: &[EvaluatedTradingOpportunity]) -> Vec<PotentialConstructionTask> {
    let mut new_tasks_with_tickets = Vec::new();
    for ev_opp in trading_opportunities_within_budget.iter() {
        let volume = ev_opp
            .trading_opportunity
            .purchase_market_trade_good_entry
            .trade_volume
            .min(
                ev_opp
                    .trading_opportunity
                    .sell_market_trade_good_entry
                    .trade_volume,
            ) as u32;

        let estimated_costs = Credits::from(
            ev_opp
                .trading_opportunity
                .purchase_market_trade_good_entry
                .purchase_price,
        ) * volume;

        new_tasks_with_tickets.push(PotentialConstructionTask {
            ship_symbol: ev_opp.ship_symbol.clone(),
            task: TradeProfitably {
                evaluated_trading_opportunity: ev_opp.clone(),
                estimated_costs,
                from: ev_opp.trading_opportunity.purchase_waypoint_symbol.clone(),
                to: ev_opp.trading_opportunity.sell_waypoint_symbol.clone(),
            },
        });
    }
    new_tasks_with_tickets
}

fn determine_construction_fleet_actions(
    admiral: &FleetAdmiral,
    facts: &FleetDecisionFacts,
    my_fleet_id: &FleetId,
    latest_market_entries: &Vec<MarketEntry>,
    ship_prices: &ShipPriceInfo,
    waypoint_map: &HashMap<WaypointSymbol, &Waypoint>,
    unassigned_ships_of_fleet: &[&Ship],
    active_trade_routes: &HashSet<ActiveTradeRoute>,
    fleet_budget: &FleetBudget,
    best_new_trading_opportunities: &[EvaluatedTradingOpportunity],
) -> Result<HashMap<ShipSymbol, ConstructionFleetAction>> {
    let active_trades = admiral
        .treasurer
        .get_fleet_tickets()?
        .get(my_fleet_id)
        .cloned()
        .unwrap_or_default();

    let cargo_sizes = unassigned_ships_of_fleet
        .iter()
        .map(|s| (s.symbol.clone(), s.cargo.capacity))
        .collect::<HashMap<_, _>>();

    let prioritized_actions = if let Some(materialized_supply_chain) = facts.materialized_supply_chain.clone() {
        let required_construction_materials = facts
            .construction_site
            .clone()
            .map(|cs| cs.missing_construction_materials())
            .unwrap_or_default();

        let goods_of_interest: Vec<TradeGoodSymbol> = vec![
            //TradeGoodSymbol::ADVANCED_CIRCUITRY,
            //TradeGoodSymbol::FAB_MATS,
            TradeGoodSymbol::SHIP_PLATING,
            TradeGoodSymbol::SHIP_PARTS,
            TradeGoodSymbol::MICROPROCESSORS,
            TradeGoodSymbol::CLOTHING,
        ];

        let goods_of_interest_in_order = required_construction_materials
            .keys()
            .cloned()
            .chain(goods_of_interest)
            .collect_vec();

        let scored_supply_chain_routes: Vec<ScoredSupplyChainSupportRoute> =
            calc_scored_supply_chain_routes(&materialized_supply_chain, goods_of_interest_in_order);

        let available_capital = fleet_budget.available_capital();

        let market_data: Vec<(WaypointSymbol, Vec<MarketTradeGood>)> = trading::to_trade_goods_with_locations(&latest_market_entries);
        let flattened_market_data: Vec<(MarketTradeGood, WaypointSymbol)> = market_data
            .iter()
            .flat_map(|(wps, mtg_vec)| mtg_vec.iter().map(|mtg| (mtg.clone(), wps.clone())))
            .collect_vec();

        // if the supply-level of the construction materials is sufficiently high, we prioritise them
        let construction_material_deliveries: Vec<ConstructionFleetAction> = flattened_market_data
            .iter()
            .filter(|(mtg, wps)| {
                required_construction_materials.contains_key(&mtg.symbol) && mtg.trade_good_type == TradeGoodType::Export && mtg.supply >= SupplyLevel::High
            })
            .filter(|(mtg, wps)| {
                active_trade_routes
                    .iter()
                    .any(|atr| atr.from == *wps && atr.trade_good == mtg.symbol)
                    .not()
            })
            .map(|(mtg, wps)| {
                let volume = mtg.trade_volume as u32;

                DeliverConstructionMaterials {
                    trade_good_symbol: mtg.symbol.clone(),
                    from: wps.clone(),
                    to: facts.construction_site.clone().unwrap().symbol,
                    units: volume,
                    market_trade_good: mtg.clone(),
                    estimated_costs: Credits::from(mtg.purchase_price) * volume,
                }
            })
            .collect_vec();

        let boosted_trade_routes = scored_supply_chain_routes
            .into_iter()
            .filter(|r| {
                active_trade_routes
                    .iter()
                    .any(|atr| {
                        atr.from == r.tgr.source_location
                            && atr.to == r.tgr.delivery_location
                            && atr.trade_good == r.tgr.trade_good
                            && r.num_allowed_parallel_pickups <= atr.number_ongoing_trades as u32
                    })
                    .not()
            })
            .take(unassigned_ships_of_fleet.len())
            .map(|r| BoostSupplyChain {
                trade_good_symbol: r.tgr.trade_good.clone(),
                from: r.tgr.source_location.clone(),
                to: r.tgr.delivery_location.clone(),
                scored_supply_chain_support_route: r.clone(),
                units: r
                    .tgr
                    .source_market_entry
                    .trade_volume
                    .min(r.tgr.delivery_market_entry.trade_volume) as u32,
                estimated_costs: Credits::from(r.purchase_price) * r.tgr.source_market_entry.trade_volume,
            })
            .collect_vec();

        let profitable_trades = best_new_trading_opportunities
            .iter()
            .take(unassigned_ships_of_fleet.len())
            .cloned()
            .map(|(e)| {
                let volume = e
                    .trading_opportunity
                    .purchase_market_trade_good_entry
                    .trade_volume
                    .min(
                        e.trading_opportunity
                            .sell_market_trade_good_entry
                            .trade_volume,
                    ) as u32;

                let estimated_costs = Credits::from(
                    e.trading_opportunity
                        .purchase_market_trade_good_entry
                        .purchase_price,
                ) * volume;

                TradeProfitably {
                    evaluated_trading_opportunity: e.clone(),
                    estimated_costs,
                    from: e.trading_opportunity.purchase_waypoint_symbol.clone(),
                    to: e.trading_opportunity.sell_waypoint_symbol.clone(),
                }
            })
            .collect_vec();

        let prioritized_actions: Vec<ConstructionFleetAction> = construction_material_deliveries
            .into_iter()
            .chain(boosted_trade_routes)
            .chain(profitable_trades)
            .collect_vec();

        println!("Hello, breakpoint");
        find_best_combination(unassigned_ships_of_fleet, &prioritized_actions, &waypoint_map, fleet_budget)
    } else {
        HashMap::new()
    };

    Ok(prioritized_actions)
}

fn find_best_combination(
    ships: &[&Ship],
    actions: &[ConstructionFleetAction],
    waypoint_map: &HashMap<WaypointSymbol, &Waypoint>,
    fleet_budget: &FleetBudget,
) -> HashMap<ShipSymbol, ConstructionFleetAction> {
    let actions = ConstructionFleetAction::select_actions_within_budget(actions, fleet_budget.available_capital());

    // If there are no actions, return an empty hashmap
    if actions.is_empty() || ships.is_empty() {
        return HashMap::new();
    }

    if ships.len() == 1 {
        return HashMap::from([(ships[0].symbol.clone(), actions[0].clone())]);
    }

    // if we have evaluated trading opportunities in here, we are currently overriding the ship symbol and therefor invalidating the whole result
    // Let's keep those entries and only pick the best combination of the rest

    let already_assigned_ships = actions
        .iter()
        .filter_map(|a| match a {
            DeliverConstructionMaterials { .. } => None,
            BoostSupplyChain { .. } => None,
            TradeProfitably {
                evaluated_trading_opportunity, ..
            } => Some((evaluated_trading_opportunity.ship_symbol.clone(), a.clone())),
        })
        .collect::<HashMap<_, _>>();

    let actions = actions
        .into_iter()
        .filter(|a| match &a {
            DeliverConstructionMaterials { .. } => true,
            BoostSupplyChain { .. } => true,
            TradeProfitably { .. } => false,
        })
        .collect_vec();

    let ships = ships
        .into_iter()
        .filter(|s| already_assigned_ships.contains_key(&s.symbol).not())
        .collect_vec();

    // We'll track the best score and the corresponding assignment
    let mut best_total_distance = 0u32;
    let mut best_assignment: HashMap<ShipSymbol, ConstructionFleetAction> = HashMap::new();

    // Generate all possible ways to select actions.len() ships from the ships vector
    for ship_combination in ships.iter().combinations(actions.len()) {
        // For each combination of ships, try all permutations of actions
        for action_permutation in actions.iter().permutations(actions.len()) {
            let mut total_distance = 0;
            let mut current_assignment = HashMap::new();

            // Pair each ship with an action and calculate the score
            for (ship, action) in ship_combination.iter().zip(action_permutation.iter()) {
                // Calculate the score for this ship-action pair (you'll need to define this logic)
                let pair_score = calculate_total_distance(ship, action, waypoint_map);
                total_distance += pair_score;

                // Store this assignment
                current_assignment.insert(ship.symbol.clone(), action.adjusted_for_cargo_space(&ship.cargo));
            }

            // Update the best assignment if this one is better
            if total_distance < best_total_distance {
                best_total_distance = total_distance;
                best_assignment = current_assignment;
            }
        }
    }

    let result = best_assignment
        .into_iter()
        .chain(already_assigned_ships)
        .collect();

    result
}

fn calculate_total_distance(ship: &Ship, action: &ConstructionFleetAction, waypoint_map: &HashMap<WaypointSymbol, &Waypoint>) -> u32 {
    let from = waypoint_map.get(&ship.nav.waypoint_symbol).unwrap();
    let start = waypoint_map.get(&action.purchase_location()).unwrap();
    let end = waypoint_map.get(&action.delivery_location()).unwrap();

    from.distance_to(start) + start.distance_to(end)
}

#[derive(Clone)]
pub enum ConstructionFleetAction {
    DeliverConstructionMaterials {
        trade_good_symbol: TradeGoodSymbol,
        from: WaypointSymbol,
        to: WaypointSymbol,
        units: u32,
        market_trade_good: MarketTradeGood,
        estimated_costs: Credits,
    },
    BoostSupplyChain {
        trade_good_symbol: TradeGoodSymbol,
        from: WaypointSymbol,
        to: WaypointSymbol,
        scored_supply_chain_support_route: ScoredSupplyChainSupportRoute,
        units: u32,
        estimated_costs: Credits,
    },
    TradeProfitably {
        evaluated_trading_opportunity: EvaluatedTradingOpportunity,
        estimated_costs: Credits,
        from: WaypointSymbol,
        to: WaypointSymbol,
    },
}

impl ConstructionFleetAction {
    pub(crate) fn adjusted_for_cargo_space(&self, cargo: &Cargo) -> Self {
        let available_cargo_space = (cargo.capacity - cargo.units) as u32;
        let mut copy = self.clone();
        match &mut copy {
            DeliverConstructionMaterials { units, .. } => {
                *units = (*units).min(available_cargo_space);
            }
            BoostSupplyChain { units, .. } => {
                *units = (*units).min(available_cargo_space);
            }
            TradeProfitably {
                evaluated_trading_opportunity, ..
            } => {
                assert!(
                    evaluated_trading_opportunity.units <= available_cargo_space,
                    "the evaluated trading opportunity should already have the correct units"
                );
            }
        }
        copy
    }
}

impl ConstructionFleetAction {
    pub(crate) fn purchase_location(&self) -> WaypointSymbol {
        match self {
            DeliverConstructionMaterials { from, .. } => from.clone(),
            BoostSupplyChain { from, .. } => from.clone(),
            TradeProfitably { from, .. } => from.clone(),
        }
    }

    pub(crate) fn delivery_location(&self) -> WaypointSymbol {
        match self {
            DeliverConstructionMaterials { to, .. } => to.clone(),
            BoostSupplyChain { to, .. } => to.clone(),
            TradeProfitably { to, .. } => to.clone(),
        }
    }
}

impl ConstructionFleetAction {
    pub fn estimated_costs(&self) -> Credits {
        match self {
            BoostSupplyChain { estimated_costs, .. } => estimated_costs.clone(),
            TradeProfitably { estimated_costs, .. } => estimated_costs.clone(),
            DeliverConstructionMaterials { estimated_costs, .. } => estimated_costs.clone(),
        }
    }

    pub fn select_actions_within_budget(actions: &[ConstructionFleetAction], budget: Credits) -> Vec<ConstructionFleetAction> {
        let mut selected_actions = Vec::new();
        let mut remaining_budget = budget;

        // Go through the prioritized list in order
        for action in actions.iter().cloned() {
            let cost = action.estimated_costs();

            // If this action fits within the remaining budget, select it
            if cost <= remaining_budget {
                remaining_budget -= cost;
                selected_actions.push(action);
            }
            // If too expensive, skip this action and continue to the next one
        }

        selected_actions
    }
}

#[derive(Clone)]
pub struct PotentialConstructionTask {
    pub ship_symbol: ShipSymbol,
    pub task: ConstructionFleetAction,
}

impl PotentialConstructionTask {
    pub fn create_purchase_ticket_details(&self) -> PurchaseTradeGoodsTicketDetails {
        match &self.task {
            DeliverConstructionMaterials {
                trade_good_symbol,
                from,
                units,
                market_trade_good,
                estimated_costs,
                ..
            } => PurchaseTradeGoodsTicketDetails {
                waypoint_symbol: from.clone(),
                trade_good: trade_good_symbol.clone(),
                expected_price_per_unit: market_trade_good.purchase_price.into(),
                quantity: *units,
                expected_total_purchase_price: *estimated_costs,
            },
            BoostSupplyChain {
                trade_good_symbol,
                from,
                scored_supply_chain_support_route,
                units,
                estimated_costs,
                ..
            } => PurchaseTradeGoodsTicketDetails {
                waypoint_symbol: from.clone(),
                trade_good: trade_good_symbol.clone(),
                expected_price_per_unit: scored_supply_chain_support_route.purchase_price.into(),
                quantity: *units,
                expected_total_purchase_price: *estimated_costs,
            },
            TradeProfitably {
                evaluated_trading_opportunity: e,
                estimated_costs,
                ..
            } => PurchaseTradeGoodsTicketDetails {
                waypoint_symbol: e.trading_opportunity.purchase_waypoint_symbol.clone(),
                trade_good: e
                    .trading_opportunity
                    .purchase_market_trade_good_entry
                    .symbol
                    .clone(),
                expected_price_per_unit: e
                    .trading_opportunity
                    .purchase_market_trade_good_entry
                    .purchase_price
                    .into(),
                quantity: e.units,
                expected_total_purchase_price: *estimated_costs,
            },
        }
    }

    pub fn create_sell_or_deliver_ticket_details(&self) -> FinanceTicketDetails {
        match &self.task {
            DeliverConstructionMaterials {
                trade_good_symbol,
                from,
                units,
                market_trade_good,
                estimated_costs,
                ..
            } => {
                let delivery_details = DeliverConstructionMaterialsTicketDetails {
                    waypoint_symbol: from.clone(),
                    trade_good: trade_good_symbol.clone(),
                    quantity: *units,
                    maybe_matching_purchase_ticket: None, // will be set after we created the actual purchase ticket
                };

                FinanceTicketDetails::SupplyConstructionSite(delivery_details)
            }
            BoostSupplyChain {
                trade_good_symbol,
                from,
                to,
                scored_supply_chain_support_route,
                units,
                estimated_costs,
            } => {
                let sell_price = scored_supply_chain_support_route
                    .tgr
                    .delivery_market_entry
                    .sell_price
                    .into();
                let sell_details = SellTradeGoodsTicketDetails {
                    waypoint_symbol: to.clone(),
                    trade_good: trade_good_symbol.clone(),
                    expected_price_per_unit: sell_price,
                    quantity: *units,
                    expected_total_sell_price: sell_price * *units,
                    maybe_matching_purchase_ticket: None, // will be set later
                };

                SellTradeGoods(sell_details)
            }
            TradeProfitably {
                evaluated_trading_opportunity: e,
                ..
            } => {
                let sell_details = SellTradeGoodsTicketDetails {
                    waypoint_symbol: e.trading_opportunity.sell_waypoint_symbol.clone(),
                    trade_good: e
                        .trading_opportunity
                        .sell_market_trade_good_entry
                        .symbol
                        .clone(),
                    expected_price_per_unit: e
                        .trading_opportunity
                        .sell_market_trade_good_entry
                        .sell_price
                        .into(),
                    quantity: e.units,
                    expected_total_sell_price: Credits::from(
                        e.trading_opportunity
                            .sell_market_trade_good_entry
                            .sell_price,
                    ) * e.units,
                    maybe_matching_purchase_ticket: None, // will be set after we created the actual purchase ticket
                };

                SellTradeGoods(sell_details)
            }
        }
    }
}
