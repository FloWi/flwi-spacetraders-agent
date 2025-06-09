use crate::{calc_batches_based_on_volume_constraint, get_closest_waypoint};
use anyhow::{anyhow, Result};
use itertools::Either::{Left, Right};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use st_domain::budgeting::credits::Credits;
use st_domain::budgeting::treasury_redesign::FinanceTicketDetails::SellTradeGoods;
use st_domain::budgeting::treasury_redesign::{
    DeliverCargoContractTicketDetails, FinanceTicket, PurchaseCargoReason, PurchaseTradeGoodsTicketDetails, SellTradeGoodsTicketDetails,
};
use st_domain::trading::group_markets_by_type;
use st_domain::{
    combine_maps, trading, Cargo, Contract, ContractEvaluationResult, ContractId, FleetId, Inventory, MarketEntry, MarketTradeGood, ShipSymbol,
    TradeGoodSymbol, TradeGoodType, Waypoint, WaypointSymbol,
};
use std::collections::{HashMap, HashSet};
use std::ops::Not;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct ContractManager {
    contract: Arc<Mutex<Option<Contract>>>,
}

impl ContractManager {
    pub fn new() -> Self {
        Self {
            contract: Arc::new(Mutex::new(None)),
        }
    }
}

pub fn calculate_necessary_tickets_for_contract(
    ship_cargo: &Cargo,
    ship_location: &WaypointSymbol,
    contract: &Contract,
    latest_market_entries: &[MarketEntry],
    waypoints_of_system: &[Waypoint],
) -> Result<ContractEvaluationResult> {
    let ship_cargo_size = ship_cargo.capacity as u32;

    let allow_list: HashSet<TradeGoodSymbol> = contract
        .terms
        .deliver
        .iter()
        .filter_map(|delivery_entry| {
            let still_required_units = delivery_entry.units_required - delivery_entry.units_fulfilled;
            (still_required_units > 0).then_some(delivery_entry.trade_symbol.clone())
        })
        .collect();

    let (cargo_on_allowed_list, cargo_not_on_allowed_list): (Vec<_>, Vec<_>) = ship_cargo
        .inventory
        .iter()
        .partition_map(|inventory_entry| {
            if allow_list.contains(&inventory_entry.symbol) {
                Left(inventory_entry)
            } else {
                Right(inventory_entry)
            }
        });

    let trading_entries = trading::to_trade_goods_with_locations(latest_market_entries);

    let export_markets: HashMap<TradeGoodSymbol, Vec<(WaypointSymbol, MarketTradeGood)>> = group_markets_by_type(&trading_entries, TradeGoodType::Export);
    let exchange_markets: HashMap<TradeGoodSymbol, Vec<(WaypointSymbol, MarketTradeGood)>> = group_markets_by_type(&trading_entries, TradeGoodType::Exchange);
    let import_markets: HashMap<TradeGoodSymbol, Vec<(WaypointSymbol, MarketTradeGood)>> = group_markets_by_type(&trading_entries, TradeGoodType::Import);

    let all_demand_markets: HashMap<TradeGoodSymbol, Vec<(WaypointSymbol, MarketTradeGood)>> = combine_maps(&import_markets, &exchange_markets);
    let all_supply_markets: HashMap<TradeGoodSymbol, Vec<(WaypointSymbol, MarketTradeGood)>> = combine_maps(&export_markets, &exchange_markets);

    let mut purchase_tickets: Vec<PurchaseTradeGoodsTicketDetails> = vec![];
    let mut delivery_tickets: Vec<DeliverCargoContractTicketDetails> = vec![];

    for deliver in contract.terms.deliver.iter() {
        let trade_good = &deliver.trade_symbol;
        let quantity = deliver.units_required - deliver.units_fulfilled;

        let (best_purchase_location, market_entry) = all_supply_markets
            .get(trade_good)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .min_by_key(|(_, mtg)| mtg.purchase_price)
            .ok_or(anyhow!("no purchase location for {} found", trade_good))?;

        let purchase_batches = calc_batches_based_on_volume_constraint(quantity, (market_entry.trade_volume as u32).min(ship_cargo_size));

        for (idx, purchase_batch) in purchase_batches.iter().enumerate() {
            // we assume that every batch gets a bit more expensive
            let increase_factor = 1.02_f64.powi(idx as i32);
            let expected_batch_price_per_unit: Credits = (((market_entry.purchase_price as f64) * increase_factor).ceil() as i64).into();

            let total_batch_price = expected_batch_price_per_unit * *purchase_batch;

            purchase_tickets.push(PurchaseTradeGoodsTicketDetails {
                waypoint_symbol: best_purchase_location.clone(),
                trade_good: trade_good.clone(),
                expected_price_per_unit: expected_batch_price_per_unit,
                quantity: *purchase_batch,
                expected_total_purchase_price: total_batch_price,
                purchase_cargo_reason: Some(PurchaseCargoReason::Contract(contract.id.clone())),
            });
            delivery_tickets.push(DeliverCargoContractTicketDetails {
                waypoint_symbol: deliver.destination_symbol.clone(),
                trade_good: deliver.trade_symbol.clone(),
                quantity: *purchase_batch,
                contract_id: contract.id.clone(),
            })
        }
    }

    let sell_excess_cargo_tickets: Vec<SellTradeGoodsTicketDetails> =
        create_sell_tickets_for_cargo_items(&cargo_not_on_allowed_list, ship_location, waypoints_of_system, &all_demand_markets);

    Ok(ContractEvaluationResult {
        purchase_tickets,
        delivery_tickets,
        contract: contract.clone(),
        sell_excess_cargo_tickets,
    })
}

pub(crate) fn create_sell_tickets_for_cargo_items(
    inventory_entries_to_sell: &[&Inventory],
    ship_location: &WaypointSymbol,
    waypoints_of_system: &[Waypoint],
    all_demand_markets: &HashMap<TradeGoodSymbol, Vec<(WaypointSymbol, MarketTradeGood)>>,
) -> Vec<SellTradeGoodsTicketDetails> {
    let mut sell_ticket_details = Vec::new();

    let waypoint_map: HashMap<WaypointSymbol, &Waypoint> = waypoints_of_system
        .into_iter()
        .map(|wp| (wp.symbol.clone(), wp))
        .collect();

    for inventory_entry in inventory_entries_to_sell {
        let demand_markets = all_demand_markets
            .get(&inventory_entry.symbol)
            .cloned()
            .unwrap_or_default();
        let waypoint_of_demand_markets = demand_markets.iter().map(|(wps, _)| wps.clone()).collect();
        let maybe_closest = get_closest_waypoint(ship_location, &waypoint_map, waypoint_of_demand_markets);
        if let Some(closest_wp) = maybe_closest {
            let closest_delivery_market_entry = demand_markets
                .iter()
                .find_map(|(wps, mtg)| (wps == &closest_wp.symbol).then_some(mtg.clone()))
                .unwrap();
            let sell_batches = crate::calc_batches_based_on_volume_constraint(inventory_entry.units, closest_delivery_market_entry.trade_volume as u32);
            for (idx, sell_batch) in sell_batches.iter().enumerate() {
                let expected_price_per_unit: Credits = ((closest_delivery_market_entry.sell_price as f64 / 1.02f64.powi(idx as i32)).round() as i64).into();
                let total = expected_price_per_unit * *sell_batch;
                sell_ticket_details.push(SellTradeGoodsTicketDetails {
                    waypoint_symbol: closest_wp.symbol.clone(),
                    trade_good: closest_delivery_market_entry.symbol.clone(),
                    expected_price_per_unit: expected_price_per_unit.into(),
                    quantity: *sell_batch,
                    expected_total_sell_price: total.into(),
                    maybe_matching_purchase_ticket: None,
                })
            }
        }
    }

    sell_ticket_details
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::universe_server::universe_server::InMemoryUniverse;
    use itertools::assert_equal;
    use st_domain::{ActivityLevel, ContractId, ContractTerms, Delivery, MarketData, Payment, SupplyLevel, TradeGood, WaypointType};

    #[test]
    fn test_calculate_necessary_tickets_for_contract_should_create_sell_cargo_tickets() {
        let test_universe = get_in_memory_universe();
        let test_market_entries = get_test_market_entries(&test_universe);
        let test_waypoints = get_test_waypoints(&test_universe);

        let test_contract = create_test_contract();

        let test_cargo_with_microprocessors = create_test_cargo(vec![(TradeGoodSymbol::MICROPROCESSORS, 12)], 40);
        let ship_location = test_waypoints
            .iter()
            .find(|wp| wp.r#type == WaypointType::ENGINEERED_ASTEROID)
            .unwrap();

        let result = calculate_necessary_tickets_for_contract(
            &test_cargo_with_microprocessors,
            &ship_location.symbol,
            &test_contract,
            &test_market_entries,
            &test_waypoints,
        )
        .unwrap();

        assert_eq!(
            result.sell_excess_cargo_tickets,
            vec![SellTradeGoodsTicketDetails {
                waypoint_symbol: WaypointSymbol("X1-AD75-D44".to_string()),
                trade_good: TradeGoodSymbol::MICROPROCESSORS,
                expected_price_per_unit: 3_587.into(),
                quantity: 12,
                expected_total_sell_price: 43_044.into(),
                maybe_matching_purchase_ticket: None
            }]
        );
    }

    #[test]
    fn test_calculate_necessary_tickets_for_contract() {
        /*
        Scenario:
        contract is 95 IRON and 35 COPPER
        cargo capacity is 80
        trade_volume for COPPER is 40
        trade_volume for IRON is 40
        */
        let test_universe = get_in_memory_universe();
        let test_market_entries = get_test_market_entries(&test_universe);
        let test_waypoints = get_test_waypoints(&test_universe);

        let test_contract = create_test_contract();

        let empty_test_cargo = create_test_cargo(vec![], 80);
        let ship_location = test_waypoints
            .iter()
            .find(|wp| wp.r#type == WaypointType::ENGINEERED_ASTEROID)
            .unwrap();

        let result =
            calculate_necessary_tickets_for_contract(&empty_test_cargo, &ship_location.symbol, &test_contract, &test_market_entries, &test_waypoints).unwrap();

        let ContractEvaluationResult {
            purchase_tickets,
            delivery_tickets,
            sell_excess_cargo_tickets,
            contract,
        } = result.clone();

        let actual_purchase_ticket_quantities = purchase_tickets
            .iter()
            .map(|t| (t.trade_good.clone(), t.quantity.clone()))
            .sorted()
            .collect_vec();

        let expected_purchase_ticket_quantities = vec![
            (TradeGoodSymbol::IRON, 60),
            (TradeGoodSymbol::IRON, 35),
            (TradeGoodSymbol::COPPER, 35),
        ]
        .into_iter()
        .sorted()
        .collect_vec();

        assert_eq!(actual_purchase_ticket_quantities, expected_purchase_ticket_quantities,);

        let actual_delivery_ticket_quantities = delivery_tickets
            .iter()
            .map(|t| (t.trade_good.clone(), t.quantity.clone()))
            .sorted()
            .collect_vec();

        let expected_delivery_ticket_quantities = vec![
            (TradeGoodSymbol::IRON, 60),
            (TradeGoodSymbol::IRON, 35),
            (TradeGoodSymbol::COPPER, 35),
        ]
        .into_iter()
        .sorted()
        .collect_vec();

        assert_eq!(actual_delivery_ticket_quantities, expected_delivery_ticket_quantities);
        let actual_estimated_purchase_costs = result.estimated_purchase_costs();

        assert_eq!(sell_excess_cargo_tickets, vec![]);

        let actual_required_capital = result.required_capital();

        assert_eq!(actual_estimated_purchase_costs, 14_795.into());
        assert_eq!(actual_required_capital, 9_795.into());
    }

    fn get_in_memory_universe() -> InMemoryUniverse {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");

        let json_path = std::path::Path::new(manifest_dir)
            .parent()
            .unwrap()
            .join("resources")
            .join("universe_snapshot.json");

        let in_memory_universe = InMemoryUniverse::from_snapshot(json_path).expect("InMemoryUniverse::from_snapshot");
        in_memory_universe
    }

    fn get_test_waypoints(in_memory_universe: &InMemoryUniverse) -> Vec<Waypoint> {
        in_memory_universe.waypoints.values().cloned().collect_vec()
    }

    fn get_test_market_entries(in_memory_universe: &InMemoryUniverse) -> Vec<MarketEntry> {
        in_memory_universe
            .marketplaces
            .iter()
            .map(|(wps, market_data)| MarketEntry {
                waypoint_symbol: wps.clone(),
                market_data: market_data.clone(),
                created_at: Default::default(),
            })
            .collect_vec()
    }

    fn create_test_cargo(trade_good_quantities: Vec<(TradeGoodSymbol, u32)>, capacity: u32) -> Cargo {
        let units = trade_good_quantities
            .iter()
            .map(|(_, quantity)| *quantity)
            .sum::<u32>();
        Cargo {
            capacity: capacity as i32,
            units: units as i32,
            inventory: trade_good_quantities
                .iter()
                .map(|(tg, qty)| Inventory {
                    symbol: tg.clone(),
                    units: *qty,
                })
                .collect_vec(),
        }
    }

    fn create_test_contract() -> Contract {
        // costs: 14_795
        Contract {
            id: ContractId("contract-id-foo".to_string()),
            faction_symbol: "LORDS".to_string(),
            contract_type: "contract_type".to_string(),
            terms: ContractTerms {
                deadline: Default::default(),
                payment: Payment {
                    on_accepted: 5_000,
                    on_fulfilled: 15_000,
                },
                deliver: vec![
                    Delivery {
                        trade_symbol: TradeGoodSymbol::IRON,
                        destination_symbol: WaypointSymbol("X1-AD75-H52".to_string()),
                        units_required: 95,
                        units_fulfilled: 0,
                    },
                    Delivery {
                        trade_symbol: TradeGoodSymbol::COPPER,
                        destination_symbol: WaypointSymbol("X1-AD75-H51".to_string()),
                        units_required: 35,
                        units_fulfilled: 0,
                    },
                ],
            },
            accepted: false,
            fulfilled: false,
            deadline_to_accept: Default::default(),
        }
    }
}
