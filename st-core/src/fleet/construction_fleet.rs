use crate::calc_batches_based_on_volume_constraint;
use crate::fleet::construction_fleet::ConstructionFleetAction::{BoostSupplyChain, DeliverConstructionMaterials, TradeProfitably};
use crate::fleet::fleet::FleetAdmiral;
use anyhow::*;
use itertools::Itertools;
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use st_domain::budgeting::credits::Credits;
use st_domain::budgeting::treasury_redesign::FinanceTicketDetails::SellTradeGoods;
use st_domain::budgeting::treasury_redesign::{
    ActiveTradeRoute, DeliverConstructionMaterialsTicketDetails, FinanceTicketDetails, FleetBudget, LedgerEntry, PurchaseCargoReason,
    PurchaseTradeGoodsTicketDetails, SellTradeGoodsTicketDetails,
};
use st_domain::{
    calc_scored_supply_chain_routes, trading, ActivityLevel, ConstructJumpGateFleetConfig, Construction, EvaluatedTradingOpportunity, Fleet, FleetId,
    FleetPhase, FleetTask, FleetTaskCompletion, Inventory, LabelledCoordinate, MarketEntry, MarketTradeGood, MaterializedSupplyChain,
    ScoredSupplyChainSupportRoute, Ship, ShipPriceInfo, ShipSymbol, ShipTask, ShipType, StationaryProbeLocation, SupplyLevel, TicketId, TradeGoodSymbol,
    TradeGoodType, Waypoint, WaypointSymbol,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::Not;
use tracing::{debug, error, event};
use tracing_core::Level;

pub struct ConstructJumpGateFleet;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct DebugNoNewTaskFacts {
    fleet_budget: FleetBudget,
    active_trade_routes: HashSet<ActiveTradeRoute>,
    waypoints: Vec<Waypoint>,
    ship_prices: ShipPriceInfo,
    latest_market_entries: Vec<MarketEntry>,
    maybe_materialized_supply_chain: Option<MaterializedSupplyChain>,
    maybe_construction_site: Option<Construction>,
    fleet: Fleet,
    cfg: ConstructJumpGateFleetConfig,
    admiral_ship_purchase_demand: VecDeque<(ShipType, FleetTask)>,
    admiral_treasurer_ledger_entries: VecDeque<LedgerEntry>,
    admiral_stationary_probe_locations: Vec<StationaryProbeLocation>,
    admiral_active_trade_ids: HashMap<ShipSymbol, TicketId>,
    admiral_fleet_phase: FleetPhase,
    admiral_ship_fleet_assignment: HashMap<ShipSymbol, FleetId>,
    admiral_fleet_tasks: HashMap<FleetId, Vec<FleetTask>>,
    admiral_ship_tasks: HashMap<ShipSymbol, ShipTask>,
    admiral_all_ships: HashMap<ShipSymbol, Ship>,
    admiral_fleets: HashMap<FleetId, Fleet>,
    admiral_completed_fleet_tasks: Vec<FleetTaskCompletion>,
    unassigned_ships_of_fleet: Vec<Ship>,
    fleet_ships: Vec<Ship>,
}

pub struct NewTasksResultForConstructionFleet {
    pub new_potential_construction_tasks: Vec<PotentialConstructionTask>,
    pub unassigned_ships_with_existing_tickets: HashSet<ShipSymbol>,
    pub deliver_tasks_from_existing_cargo: HashMap<ShipSymbol, Vec<CargoDeliveryAction>>,
}

impl ConstructJumpGateFleet {
    pub async fn compute_ship_tasks(
        admiral: &FleetAdmiral,
        cfg: &ConstructJumpGateFleetConfig,
        fleet: &Fleet,
        maybe_construction_site: &Option<Construction>,
        latest_market_entries: &Vec<MarketEntry>,
        ship_prices: &ShipPriceInfo,
        waypoints: &Vec<Waypoint>,
        unassigned_ships_of_fleet: &[&Ship],
        active_trade_routes: &HashSet<ActiveTradeRoute>,
        fleet_budget: &FleetBudget,
        blocked_budget_for_contracts: Credits,
    ) -> Result<NewTasksResultForConstructionFleet> {
        let fleet_ships: Vec<&Ship> = admiral.get_ships_of_fleet(fleet);

        if unassigned_ships_of_fleet.is_empty() {
            return Ok(NewTasksResultForConstructionFleet {
                new_potential_construction_tasks: vec![],
                unassigned_ships_with_existing_tickets: Default::default(),
                deliver_tasks_from_existing_cargo: Default::default(),
            });
        }

        // don't create new tickets for ships that already have tickets
        // for some reason the execution failed. We just try again.
        let mut unassigned_ships_with_existing_tickets: HashSet<ShipSymbol> = HashSet::new();
        for s in unassigned_ships_of_fleet.iter() {
            let existing_tickets = admiral
                .treasurer
                .get_active_tickets_for_ship(&s.symbol)
                .await?;
            let has_tickets = existing_tickets.is_empty().not();
            if has_tickets {
                unassigned_ships_with_existing_tickets.insert(s.symbol.clone());
            }
        }

        // only check for the ships without trading ticket
        let unassigned_ships_without_existing_tickets: Vec<&Ship> = unassigned_ships_of_fleet
            .iter()
            .filter(|s| {
                unassigned_ships_with_existing_tickets
                    .contains(&s.symbol)
                    .not()
            })
            .cloned()
            .collect_vec();

        let ships_without_ticket_that_have_cargo = unassigned_ships_without_existing_tickets
            .iter()
            .filter(|s| s.cargo.units > 0)
            .cloned()
            .collect_vec();

        // only check for the ships without trading ticket
        let unassigned_ships_to_check: Vec<&Ship> = unassigned_ships_without_existing_tickets
            .iter()
            .filter(|s| {
                ships_without_ticket_that_have_cargo
                    .iter()
                    .any(|ship_with_cargo| s.symbol == ship_with_cargo.symbol)
                    .not()
            })
            .cloned()
            .collect_vec();

        let waypoint_map: HashMap<WaypointSymbol, &Waypoint> = waypoints
            .iter()
            .map(|wp| (wp.symbol.clone(), wp))
            .collect::<HashMap<_, _>>();

        let maybe_materialized_supply_chain = admiral
            .materialized_supply_chain_manager
            .get_materialized_supply_chain_for_system(cfg.system_symbol.clone());

        let no_go_trades = maybe_materialized_supply_chain
            .clone()
            .map(|msc| msc.no_go_trades)
            .unwrap_or_default();

        let market_data: Vec<(WaypointSymbol, Vec<MarketTradeGood>)> = trading::to_trade_goods_with_locations(latest_market_entries);
        let trading_opportunities = trading::find_trading_opportunities_sorted_by_profit_per_distance_unit(&market_data, &waypoint_map, &no_go_trades);

        let available_capital = fleet_budget.available_capital() - blocked_budget_for_contracts;

        let evaluated_trading_opportunities =
            trading::evaluate_trading_opportunities(&unassigned_ships_to_check, &waypoint_map, &trading_opportunities, available_capital.0);

        let best_new_trading_opportunities: Vec<EvaluatedTradingOpportunity> =
            trading::find_optimal_trading_routes_exhaustive(&evaluated_trading_opportunities, active_trade_routes);

        if ships_without_ticket_that_have_cargo.is_empty().not() {
            println!("We found {} ships without ticket, but with cargo", ships_without_ticket_that_have_cargo.len());

            // print debug facts for local testing
            /*
            let debug_facts = DebugNoNewTaskFacts {
                admiral_completed_fleet_tasks: admiral.completed_fleet_tasks.clone(),
                admiral_fleets: admiral.fleets.clone(),
                admiral_all_ships: admiral.all_ships.clone(),
                admiral_ship_tasks: admiral.ship_tasks.clone(),
                admiral_fleet_tasks: admiral.fleet_tasks.clone(),
                admiral_ship_fleet_assignment: admiral.ship_fleet_assignment.clone(),
                admiral_fleet_phase: admiral.fleet_phase.clone(),
                admiral_active_trade_ids: admiral.active_trade_ids.clone(),
                admiral_stationary_probe_locations: admiral.stationary_probe_locations.clone(),
                admiral_treasurer_ledger_entries: admiral.treasurer.get_ledger_entries().await?,
                admiral_ship_purchase_demand: admiral.ship_purchase_demand.clone(),
                cfg: cfg.clone(),
                fleet: fleet.clone(),
                maybe_materialized_supply_chain: maybe_materialized_supply_chain.clone(),
                maybe_construction_site: maybe_construction_site.clone(),
                latest_market_entries: latest_market_entries.clone(),
                ship_prices: ship_prices.clone(),
                waypoints: waypoints.clone(),
                unassigned_ships_of_fleet: unassigned_ships_without_existing_tickets
                    .iter()
                    .cloned()
                    .cloned()
                    .collect_vec(),
                active_trade_routes: active_trade_routes.clone(),
                fleet_budget: fleet_budget.clone(),
                fleet_ships: fleet_ships.iter().cloned().cloned().collect_vec(),
            };

            event!(
                Level::ERROR,
                message = "ships_without_ticket_but_have_cargo is not empty",
                debug_facts = serde_json::to_string(&debug_facts)?
            );
            */
        }

        let has_some_trades = fleet_ships.len() > 2;

        let new_tasks: Vec<PotentialConstructionTask> = if has_some_trades {
            let best_actions_for_ships = determine_construction_fleet_actions(
                admiral,
                &fleet.id,
                latest_market_entries,
                &maybe_materialized_supply_chain,
                maybe_construction_site,
                &waypoint_map,
                &unassigned_ships_to_check,
                active_trade_routes,
                fleet_budget,
                &best_new_trading_opportunities,
            )
            .await?;

            best_actions_for_ships
                .into_iter()
                .map(|(ship_symbol, task)| PotentialConstructionTask { ship_symbol, task })
                .collect()
        } else {
            create_trading_tickets(&best_new_trading_opportunities)
        };

        if new_tasks.is_empty() && unassigned_ships_to_check.is_empty().not() {
            let debug_facts = DebugNoNewTaskFacts {
                admiral_completed_fleet_tasks: admiral.completed_fleet_tasks.clone(),
                admiral_fleets: admiral.fleets.clone(),
                admiral_all_ships: admiral.all_ships.clone(),
                admiral_ship_tasks: admiral.ship_tasks.clone(),
                admiral_fleet_tasks: admiral.fleet_tasks.clone(),
                admiral_ship_fleet_assignment: admiral.ship_fleet_assignment.clone(),
                admiral_fleet_phase: admiral.fleet_phase.clone(),
                admiral_active_trade_ids: admiral.active_trade_ids.clone(),
                admiral_stationary_probe_locations: admiral.stationary_probe_locations.clone(),
                admiral_treasurer_ledger_entries: admiral.treasurer.get_ledger_entries().await?,
                admiral_ship_purchase_demand: admiral.ship_purchase_demand.clone(),
                cfg: cfg.clone(),
                fleet: fleet.clone(),
                maybe_materialized_supply_chain: maybe_materialized_supply_chain.clone(),
                maybe_construction_site: maybe_construction_site.clone(),
                latest_market_entries: latest_market_entries.clone(),
                ship_prices: ship_prices.clone(),
                waypoints: waypoints.clone(),
                unassigned_ships_of_fleet: unassigned_ships_to_check
                    .iter()
                    .cloned()
                    .cloned()
                    .collect_vec(),
                active_trade_routes: active_trade_routes.clone(),
                fleet_budget: fleet_budget.clone(),
                fleet_ships: fleet_ships.iter().cloned().cloned().collect_vec(),
            };

            event!(
                Level::ERROR,
                message = "ConstructJumpGateFleet didn't find new task.",
                debug_facts = serde_json::to_string(&debug_facts)?
            );
        }

        let deliver_cargo_tasks_of_ships_without_ticket = create_tasks_for_ships_with_cargo(
            &ships_without_ticket_that_have_cargo,
            latest_market_entries,
            &waypoint_map,
            maybe_construction_site,
        );

        Ok(NewTasksResultForConstructionFleet {
            new_potential_construction_tasks: new_tasks,
            unassigned_ships_with_existing_tickets,
            deliver_tasks_from_existing_cargo: deliver_cargo_tasks_of_ships_without_ticket,
        })
    }
}

fn create_tasks_for_ships_with_cargo(
    ships_with_cargo: &Vec<&Ship>,
    latest_market_entries: &Vec<MarketEntry>,
    waypoint_map: &HashMap<WaypointSymbol, &Waypoint>,
    maybe_construction_site: &Option<Construction>,
) -> HashMap<ShipSymbol, Vec<CargoDeliveryAction>> {
    ships_with_cargo
        .iter()
        .map(|&ship| {
            (
                ship.symbol.clone(),
                create_delivery_tasks_for_ship_with_cargo(ship, latest_market_entries, waypoint_map, maybe_construction_site),
            )
        })
        .collect()
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

        if volume == 0 {
            debug!("Skipped creating a ticket for trading opportunity with a volume of 0: '{ev_opp:?}'.");
            continue;
        }

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

async fn determine_construction_fleet_actions(
    admiral: &FleetAdmiral,
    my_fleet_id: &FleetId,
    latest_market_entries: &Vec<MarketEntry>,
    maybe_materialized_supply_chain: &Option<MaterializedSupplyChain>,
    maybe_construction_site: &Option<Construction>,
    waypoint_map: &HashMap<WaypointSymbol, &Waypoint>,
    unassigned_ships_of_fleet: &[&Ship],
    active_trade_routes: &HashSet<ActiveTradeRoute>,
    fleet_budget: &FleetBudget,
    best_new_trading_opportunities: &[EvaluatedTradingOpportunity],
) -> Result<HashMap<ShipSymbol, ConstructionFleetAction>> {
    let profitable_trading_actions = best_new_trading_opportunities
        .iter()
        .take(unassigned_ships_of_fleet.len())
        .cloned()
        .map(|e| {
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

            (
                e.ship_symbol.clone(),
                TradeProfitably {
                    evaluated_trading_opportunity: e.clone(),
                    estimated_costs,
                    from: e.trading_opportunity.purchase_waypoint_symbol.clone(),
                    to: e.trading_opportunity.sell_waypoint_symbol.clone(),
                },
            )
        })
        .collect::<HashMap<_, _>>();

    let num_of_traders = admiral
        .get_ships_of_fleet_id(my_fleet_id)
        .iter()
        .filter(|s| s.cargo.capacity > 0)
        .count() as u32;
    let budget_per_trader: Credits = 75_000.into();
    let budget_required_for_trading = budget_per_trader * num_of_traders;

    let is_low_on_cash = fleet_budget.available_capital() < budget_required_for_trading;

    let prioritized_actions = if let Some(materialized_supply_chain) = maybe_materialized_supply_chain.clone() {
        let required_construction_materials = maybe_construction_site
            .clone()
            .map(|cs| cs.missing_construction_materials())
            .unwrap_or_default();

        let priority_map_of_construction_materials = HashMap::from([(TradeGoodSymbol::FAB_MATS, 1), (TradeGoodSymbol::ADVANCED_CIRCUITRY, 2)]);
        let budget_limits_for_construction_materials = HashMap::from([
            (TradeGoodSymbol::FAB_MATS, 125_000),
            (TradeGoodSymbol::ADVANCED_CIRCUITRY, 750_000),
        ]);

        let goods_of_interest = materialized_supply_chain.goods_of_interest.clone();

        let goods_of_interest_in_order = required_construction_materials
            .keys()
            .cloned()
            .sorted_by_key(|trade_good_symbol| {
                priority_map_of_construction_materials
                    .get(trade_good_symbol)
                    .cloned()
                    .unwrap_or_default()
            })
            .chain(goods_of_interest)
            .collect_vec();

        let scored_supply_chain_routes: Vec<ScoredSupplyChainSupportRoute> =
            calc_scored_supply_chain_routes(&materialized_supply_chain, goods_of_interest_in_order);

        let available_capital = fleet_budget.available_capital();

        let market_data: Vec<(WaypointSymbol, Vec<MarketTradeGood>)> = trading::to_trade_goods_with_locations(latest_market_entries);
        let flattened_market_data: Vec<(MarketTradeGood, WaypointSymbol)> = market_data
            .iter()
            .flat_map(|(wps, mtg_vec)| mtg_vec.iter().map(|mtg| (mtg.clone(), wps.clone())))
            .collect_vec();

        // if the supply-level of the construction materials is sufficiently high, we prioritise them
        let export_markets_of_construction_materials = flattened_market_data
            .iter()
            .filter_map(|(mtg, wps)| match required_construction_materials.get(&mtg.symbol) {
                None => None,
                Some(qty_missing) => (mtg.trade_good_type == TradeGoodType::Export).then_some((mtg.clone(), wps.clone(), *qty_missing)),
            })
            .collect_vec();

        let construction_material_deliveries: Vec<ConstructionFleetAction> = export_markets_of_construction_materials
            .iter()
            .filter(|(mtg, wps, _)| {
                // trying whyando's condition
                let should_buy = match mtg.activity.as_ref().unwrap() {
                    ActivityLevel::Strong => mtg.supply >= SupplyLevel::High,
                    _ => mtg.supply >= SupplyLevel::Moderate,
                };
                let budget_limit_for_construction_material: Credits = (*budget_limits_for_construction_materials
                    .get(&mtg.symbol)
                    .unwrap_or(&100_000_000))
                .into();
                let has_met_budget_requirement = available_capital > budget_limit_for_construction_material;
                let no_ongoing_delivery = active_trade_routes
                    .iter()
                    .any(|atr| atr.from == *wps && atr.trade_good == mtg.symbol)
                    .not();

                should_buy && has_met_budget_requirement && no_ongoing_delivery
            })
            .map(|(mtg, wps, qty_missing)| {
                // Don't deliver more than necessary
                let volume = (mtg.trade_volume as u32).min(*qty_missing);

                DeliverConstructionMaterials {
                    trade_good_symbol: mtg.symbol.clone(),
                    from: wps.clone(),
                    to: maybe_construction_site.clone().unwrap().symbol,
                    units: volume,
                    market_trade_good: mtg.clone(),
                    estimated_costs: Credits::from(mtg.purchase_price) * volume,
                }
            })
            .collect_vec();

        let boosted_trade_routes = scored_supply_chain_routes
            .into_iter()
            .filter(|r| r.score > 0)
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

        if is_low_on_cash {
            profitable_trading_actions
        } else {
            let prioritized_actions = construction_material_deliveries
                .into_iter()
                .chain(boosted_trade_routes)
                .chain(profitable_trading_actions.values().cloned())
                .collect_vec();
            find_best_combination(unassigned_ships_of_fleet, &prioritized_actions, waypoint_map, fleet_budget)
        }
    } else {
        event!(
            Level::WARN,
            "materialized supply chain is None - using fallback of profitable_trading_actions as new tasks"
        );
        find_best_combination(
            unassigned_ships_of_fleet,
            &profitable_trading_actions.values().cloned().collect_vec(),
            waypoint_map,
            fleet_budget,
        )
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
        let ship = ships[0];
        if ship.cargo.capacity < actions[0].units() as i32 {
            println!("cargo doesn't fit - adjusting for cargo");
        }
        let action_adjusted_for_cargo = actions[0].adjusted_for_cargo_space(ship);
        return HashMap::from([(ship.symbol.clone(), action_adjusted_for_cargo)]);
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
        .iter()
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

                let cargo_adjusted_action = action.adjusted_for_cargo_space(ship);
                if cargo_adjusted_action.units() > ship.cargo.capacity as u32 {
                    println!("cargo doesn't fit");
                }
                current_assignment.insert(ship.symbol.clone(), cargo_adjusted_action);
            }

            // Update the best assignment if this one is better
            if total_distance < best_total_distance {
                best_total_distance = total_distance;
                best_assignment = current_assignment;
            }
        }
    }

    let result: HashMap<ShipSymbol, ConstructionFleetAction> = best_assignment
        .into_iter()
        .chain(already_assigned_ships)
        .collect();

    if result
        .iter()
        .find_map(|(ss, action)| {
            if let Some(ship) = ships.iter().find(|ship| ship.symbol == *ss) {
                if action.units() > ship.cargo.capacity as u32 {
                    Some((ship, action.clone()))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .is_some()
    {
        println!("cargo doesn't fit");
    }

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

#[derive(Clone)]
pub enum CargoDeliveryAction {
    SellOffCargoInventory {
        trade_good_symbol: TradeGoodSymbol,
        units: u32,
        to: WaypointSymbol,
        delivery_market_entry: MarketTradeGood,
    },
    DeliverConstructionMaterialsFromCargo {
        trade_good_symbol: TradeGoodSymbol,
        units: u32,
        to: WaypointSymbol,
    },
}

impl ConstructionFleetAction {
    pub(crate) fn adjusted_for_cargo_space(&self, ship: &Ship) -> Self {
        let available_cargo_space = (ship.cargo.capacity - ship.cargo.units) as u32;
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
        if copy.units() > available_cargo_space {
            eprintln!("Cargo doesn't fit - even it you squeeze");
        }
        copy
    }

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

    fn units(&self) -> u32 {
        match self {
            DeliverConstructionMaterials { units, .. } => *units,
            BoostSupplyChain { units, .. } => *units,
            TradeProfitably {
                evaluated_trading_opportunity, ..
            } => evaluated_trading_opportunity.units,
        }
    }
}

impl ConstructionFleetAction {
    pub fn estimated_costs(&self) -> Credits {
        match self {
            BoostSupplyChain { estimated_costs, .. } => *estimated_costs,
            TradeProfitably { estimated_costs, .. } => *estimated_costs,
            DeliverConstructionMaterials { estimated_costs, .. } => *estimated_costs,
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
                purchase_cargo_reason: Some(PurchaseCargoReason::Construction),
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
                purchase_cargo_reason: Some(PurchaseCargoReason::BoostSupplyChain),
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
                purchase_cargo_reason: Some(PurchaseCargoReason::TradeProfitably),
            },
        }
    }

    pub fn create_sell_or_deliver_ticket_details(&self) -> FinanceTicketDetails {
        match &self.task {
            DeliverConstructionMaterials {
                trade_good_symbol, to, units, ..
            } => {
                let delivery_details = DeliverConstructionMaterialsTicketDetails {
                    waypoint_symbol: to.clone(),
                    trade_good: trade_good_symbol.clone(),
                    quantity: *units,
                    maybe_matching_purchase_ticket: None, // will be set after we created the actual purchase ticket
                };

                FinanceTicketDetails::SupplyConstructionSite(delivery_details)
            }
            BoostSupplyChain {
                trade_good_symbol,
                to,
                scored_supply_chain_support_route,
                units,
                ..
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

fn create_delivery_tasks_for_ship_with_cargo(
    ship_with_cargo: &Ship,
    latest_market_entries: &Vec<MarketEntry>,
    waypoint_map: &HashMap<WaypointSymbol, &Waypoint>,
    maybe_construction_site: &Option<Construction>,
) -> Vec<CargoDeliveryAction> {
    let construction_site = maybe_construction_site.clone().unwrap();
    let missing_construction_materials = construction_site.missing_construction_materials();

    let mut actions: Vec<CargoDeliveryAction> = Vec::new();
    for inventory_entry in ship_with_cargo.cargo.inventory.clone() {
        match missing_construction_materials.get(&inventory_entry.symbol) {
            // inventory entry is construction material
            Some(required_units_for_construction) => {
                let units_to_deliver = *required_units_for_construction.min(&inventory_entry.units);
                actions.push(CargoDeliveryAction::DeliverConstructionMaterialsFromCargo {
                    trade_good_symbol: inventory_entry.symbol.clone(),
                    units: units_to_deliver,
                    to: construction_site.symbol.clone(),
                })
            }
            None => {
                // inventory entry is just normal trading cargo
                if let Some((selling_location_wps, selling_market_entry)) =
                    find_best_selling_location_for_inventory_entry(&inventory_entry, &ship_with_cargo.nav.waypoint_symbol, latest_market_entries, waypoint_map)
                {
                    let batches = calc_batches_based_on_volume_constraint(inventory_entry.units, selling_market_entry.trade_volume as u32);
                    for batch in batches {
                        actions.push(CargoDeliveryAction::SellOffCargoInventory {
                            trade_good_symbol: inventory_entry.symbol.clone(),
                            units: batch,
                            to: selling_location_wps.clone(),
                            delivery_market_entry: selling_market_entry.clone(),
                        })
                    }
                } else {
                    error!("No selling location found for inventory_entry {inventory_entry:?}");
                }
            }
        }
    }

    actions
}

fn find_best_selling_location_for_inventory_entry(
    inventory_entry: &Inventory,
    ship_location: &WaypointSymbol,
    latest_market_entries: &Vec<MarketEntry>,
    waypoint_map: &HashMap<WaypointSymbol, &Waypoint>,
) -> Option<(WaypointSymbol, MarketTradeGood)> {
    let ship_location_waypoint = waypoint_map.get(ship_location).unwrap();

    let maybe_best = trading::to_trade_goods_with_locations(latest_market_entries)
        .into_iter()
        .flat_map(|(wps, entries_for_waypoint)| {
            let market_waypoint = waypoint_map.get(&wps).unwrap();
            entries_for_waypoint
                .iter()
                .filter(|&mtg| {
                    mtg.symbol == inventory_entry.symbol && (mtg.trade_good_type == TradeGoodType::Import || mtg.trade_good_type == TradeGoodType::Exchange)
                })
                .cloned()
                .map(|mtg| {
                    let distance = ship_location_waypoint.distance_to(market_waypoint);
                    let distance = if distance < 1 { 1 } else { distance };
                    let profit_per_distance_unit = OrderedFloat(mtg.purchase_price as f64 * inventory_entry.units as f64 / distance as f64);
                    (wps.clone(), mtg, profit_per_distance_unit)
                })
                .collect_vec()
        })
        .max_by_key(|(_, _, profit_per_distance_unit)| *profit_per_distance_unit)
        .map(|(sell_wps, mtg, _)| (sell_wps, mtg.clone()));

    maybe_best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::materialized_supply_chain_manager::MaterializedSupplyChainManager;
    use st_domain::budgeting::test_sync_ledger::create_test_ledger_setup;
    use st_domain::budgeting::treasury_redesign::{ImprovedTreasurer, ThreadSafeTreasurer};
    use tokio::test;

    #[test]
    async fn test_compute_new_tasks_from_broken_runtime_state() -> Result<()> {
        let json_str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/broken-construction-fleet-details.json"));

        let NewTasksResultForConstructionFleet {
            new_potential_construction_tasks: actual_tasks,
            unassigned_ships_with_existing_tickets,
            ..
        } = compute_tasks_from_snapshot_file(json_str).await?;

        assert!(actual_tasks.is_empty().not(), "Should have found some tasks");
        Ok(())
    }

    #[test]
    async fn test_compute_new_tasks_from_broken_runtime_state_2() -> Result<()> {
        let json_str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/broken-again-details.json"));

        let NewTasksResultForConstructionFleet {
            new_potential_construction_tasks: actual_tasks,
            unassigned_ships_with_existing_tickets,
            ..
        } = compute_tasks_from_snapshot_file(json_str).await?;

        assert!(actual_tasks.is_empty().not(), "Should have found some tasks");
        Ok(())
    }

    #[test]
    async fn test_debug_ship_purchases() -> Result<()> {
        let json_str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/broken-again-details.json"));
        let input = serde_json::from_str::<DebugNoNewTaskFacts>(json_str)?;

        let mut treasurer = ImprovedTreasurer::new();

        let construction_fleet_id = FleetId(2);
        for entry in input.admiral_treasurer_ledger_entries.iter().cloned() {
            let maybe_budget_before = treasurer
                .get_fleet_budgets()?
                .get(&construction_fleet_id)
                .cloned();

            treasurer.process_ledger_entry(entry.clone())?;

            let maybe_budget_after = treasurer
                .get_fleet_budgets()?
                .get(&construction_fleet_id)
                .cloned();

            if let Some(budget_before) = maybe_budget_before {
                let available_capital_before = budget_before.available_capital();
                if let Some(budget_after) = maybe_budget_after {
                    let available_capital_after = budget_after.available_capital();
                    if available_capital_after < 4_000.into() {
                        println!(
                            r#"
================================================================================================================
available_capital_before {available_capital_before}.
available_capital_after {available_capital_after} < 4_000c.


{budget_before:?}

{entry:?}

{budget_after:?}
                    "#
                        )
                    }
                }
            }
        }

        Ok(())
    }

    async fn compute_tasks_from_snapshot_file(json_str: &str) -> Result<NewTasksResultForConstructionFleet, Error> {
        let input = serde_json::from_str::<DebugNoNewTaskFacts>(json_str)?;

        let (test_ledger_archiver, ledger_archiving_task_sender) = create_test_ledger_setup().await;

        let treasurer = ThreadSafeTreasurer::from_replayed_ledger_log(
            input
                .admiral_treasurer_ledger_entries
                .iter()
                .cloned()
                .collect_vec(),
            ledger_archiving_task_sender,
        );

        let active_tickets = treasurer.get_active_tickets().await?;

        let materialized_supply_chain_manager = MaterializedSupplyChainManager::new();
        if let Some(msc) = &input.maybe_materialized_supply_chain {
            materialized_supply_chain_manager.register_materialized_supply_chain(msc.system_symbol.clone(), msc.clone())?;
        }
        let admiral = FleetAdmiral {
            completed_fleet_tasks: input.admiral_completed_fleet_tasks.clone(),
            fleets: input.admiral_fleets.clone(),
            all_ships: input.admiral_all_ships.clone(),
            ship_tasks: input.admiral_ship_tasks.clone(),
            fleet_tasks: input.admiral_fleet_tasks.clone(),
            ship_fleet_assignment: input.admiral_ship_fleet_assignment.clone(),
            fleet_phase: input.admiral_fleet_phase.clone(),
            active_trade_ids: input.admiral_active_trade_ids.clone(),
            stationary_probe_locations: input.admiral_stationary_probe_locations.clone(),
            treasurer: treasurer.clone(),
            materialized_supply_chain_manager,
            ship_purchase_demand: input.admiral_ship_purchase_demand.clone(),
        };

        let actual_tasks = ConstructJumpGateFleet::compute_ship_tasks(
            &admiral,
            &input.cfg,
            &input.fleet,
            &input.maybe_construction_site,
            &input.latest_market_entries,
            &input.ship_prices,
            &input.waypoints,
            &input.unassigned_ships_of_fleet.iter().collect_vec(),
            &input.active_trade_routes,
            &input.fleet_budget,
            0.into(),
        )
        .await?;
        Ok(actual_tasks)
    }
}
