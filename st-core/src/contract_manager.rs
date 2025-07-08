use crate::{calc_batches_based_on_volume_constraint, get_closest_waypoint};
use anyhow::{anyhow, Result};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use st_domain::budgeting::credits::Credits;
use st_domain::budgeting::treasury_redesign::{
    DeliverCargoContractTicketDetails, PurchaseCargoReason, PurchaseTradeGoodsTicketDetails, SellTradeGoodsTicketDetails,
};
use st_domain::trading::group_markets_by_type;
use st_domain::{
    combine_maps, trading, Cargo, Contract, ContractEvaluationResult, MarketEntry, MarketTradeGood,
    TradeGoodSymbol, TradeGoodType, Waypoint, WaypointSymbol,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct ContractManager {
    contract: Arc<Mutex<Option<Contract>>>,
}

impl Default for ContractManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ContractManager {
    pub fn new() -> Self {
        Self {
            contract: Arc::new(Mutex::new(None)),
        }
    }
}

#[derive(PartialEq, Debug, Clone, Serialize, Deserialize)]
struct SplitCargoForContractUsageResult {
    excess: HashMap<TradeGoodSymbol, u32>,
    usable_for_contract: HashMap<TradeGoodSymbol, (u32, WaypointSymbol)>,
    still_open_contract_entries: HashMap<TradeGoodSymbol, (u32, WaypointSymbol)>,
}

fn split_cargo_into_usable_and_excess_entries_for_contract_usage(ship_cargo: &Cargo, contract: &Contract) -> SplitCargoForContractUsageResult {
    // cargo: 45x iron, 35x microprocessors
    // contract demands: 35x iron and 25x copper

    // expected:
    // usable: 35x iron
    // excess: 10x iron and 35x microprocessors
    let mut excess: HashMap<TradeGoodSymbol, u32> = HashMap::new();
    let mut usable_for_contract: HashMap<TradeGoodSymbol, (u32, WaypointSymbol)> = HashMap::new();

    for inventory_entry in ship_cargo.inventory.iter().cloned() {
        if let Some(delivery_entry) = contract
            .terms
            .deliver
            .iter()
            .find(|delivery_entry| delivery_entry.trade_symbol == inventory_entry.symbol)
        {
            let open_quantity = delivery_entry.units_required - delivery_entry.units_fulfilled;
            let usable_quantity = open_quantity.min(inventory_entry.units);
            if usable_quantity >= inventory_entry.units {
                // we used all up
                usable_for_contract.insert(inventory_entry.symbol, (inventory_entry.units, delivery_entry.destination_symbol.clone()));
            } else {
                // we have excess
                let excess_amount = inventory_entry.units - usable_quantity;
                usable_for_contract.insert(inventory_entry.symbol.clone(), (usable_quantity, delivery_entry.destination_symbol.clone()));
                excess.insert(inventory_entry.symbol, excess_amount);
            }
        } else {
            // cargo item not found in contract terms - can't be used for contract
            excess.insert(inventory_entry.symbol.clone(), inventory_entry.units);
        }
    }

    let still_open_contract_entries: HashMap<TradeGoodSymbol, (u32, WaypointSymbol)> = contract
        .terms
        .deliver
        .iter()
        .filter_map(|delivery_entry| {
            let open_quantity = delivery_entry.units_required - delivery_entry.units_fulfilled;
            let provided_from_cargo_quantity = usable_for_contract
                .get(&delivery_entry.trade_symbol)
                .map(|(quantity, _)| *quantity)
                .unwrap_or_default();
            let still_open_quantity = open_quantity - provided_from_cargo_quantity;
            (still_open_quantity > 0).then_some((
                delivery_entry.trade_symbol.clone(),
                (still_open_quantity, delivery_entry.destination_symbol.clone()),
            ))
        })
        .collect();

    SplitCargoForContractUsageResult {
        excess,
        usable_for_contract,
        still_open_contract_entries,
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

    let SplitCargoForContractUsageResult {
        excess,
        usable_for_contract,
        still_open_contract_entries,
    } = split_cargo_into_usable_and_excess_entries_for_contract_usage(ship_cargo, contract);

    let trading_entries = trading::to_trade_goods_with_locations(latest_market_entries);

    let export_markets: HashMap<TradeGoodSymbol, Vec<(WaypointSymbol, MarketTradeGood)>> = group_markets_by_type(&trading_entries, TradeGoodType::Export);
    let exchange_markets: HashMap<TradeGoodSymbol, Vec<(WaypointSymbol, MarketTradeGood)>> = group_markets_by_type(&trading_entries, TradeGoodType::Exchange);
    let import_markets: HashMap<TradeGoodSymbol, Vec<(WaypointSymbol, MarketTradeGood)>> = group_markets_by_type(&trading_entries, TradeGoodType::Import);

    let all_demand_markets: HashMap<TradeGoodSymbol, Vec<(WaypointSymbol, MarketTradeGood)>> = combine_maps(&import_markets, &exchange_markets);
    let all_supply_markets: HashMap<TradeGoodSymbol, Vec<(WaypointSymbol, MarketTradeGood)>> = combine_maps(&export_markets, &exchange_markets);

    let mut purchase_tickets: Vec<PurchaseTradeGoodsTicketDetails> = vec![];
    let mut delivery_tickets: Vec<DeliverCargoContractTicketDetails> = vec![];

    for (trade_symbol, (quantity, destination_wps)) in still_open_contract_entries.iter() {
        let (best_purchase_location, market_entry) = all_supply_markets
            .get(trade_symbol)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .min_by_key(|(_, mtg)| mtg.purchase_price)
            .ok_or(anyhow!("no purchase location for {} found", trade_symbol))?;

        let purchase_batches = calc_batches_based_on_volume_constraint(*quantity, (market_entry.trade_volume as u32).min(ship_cargo_size));

        for (idx, purchase_batch) in purchase_batches.iter().enumerate() {
            // we assume that every batch gets a bit more expensive
            let increase_factor = 1.02_f64.powi(idx as i32);
            let expected_batch_price_per_unit: Credits = (((market_entry.purchase_price as f64) * increase_factor).ceil() as i64).into();

            let total_batch_price = expected_batch_price_per_unit * *purchase_batch;

            purchase_tickets.push(PurchaseTradeGoodsTicketDetails {
                waypoint_symbol: best_purchase_location.clone(),
                trade_good: trade_symbol.clone(),
                expected_price_per_unit: expected_batch_price_per_unit,
                quantity: *purchase_batch,
                expected_total_purchase_price: total_batch_price,
                purchase_cargo_reason: Some(PurchaseCargoReason::Contract(contract.id.clone())),
            });
            delivery_tickets.push(DeliverCargoContractTicketDetails {
                waypoint_symbol: destination_wps.clone(),
                trade_good: trade_symbol.clone(),
                quantity: *purchase_batch,
                contract_id: contract.id.clone(),
            })
        }
    }

    for (trade_symbol, (quantity, destination_wps)) in usable_for_contract {
        delivery_tickets.push(DeliverCargoContractTicketDetails {
            waypoint_symbol: destination_wps.clone(),
            trade_good: trade_symbol.clone(),
            quantity,
            contract_id: contract.id.clone(),
        })
    }

    let sell_excess_cargo_tickets: Vec<SellTradeGoodsTicketDetails> =
        create_sell_tickets_for_cargo_items(&excess, ship_location, waypoints_of_system, &all_demand_markets);

    Ok(ContractEvaluationResult {
        purchase_tickets,
        delivery_tickets,
        contract: contract.clone(),
        sell_excess_cargo_tickets,
    })
}

pub(crate) fn create_sell_tickets_for_cargo_items(
    inventory_entries_to_sell: &HashMap<TradeGoodSymbol, u32>,
    ship_location: &WaypointSymbol,
    waypoints_of_system: &[Waypoint],
    all_demand_markets: &HashMap<TradeGoodSymbol, Vec<(WaypointSymbol, MarketTradeGood)>>,
) -> Vec<SellTradeGoodsTicketDetails> {
    let mut sell_ticket_details = Vec::new();

    let waypoint_map: HashMap<WaypointSymbol, &Waypoint> = waypoints_of_system
        .iter()
        .map(|wp| (wp.symbol.clone(), wp))
        .collect();

    for (trade_good_to_sell, quantity) in inventory_entries_to_sell {
        let demand_markets = all_demand_markets
            .get(trade_good_to_sell)
            .cloned()
            .unwrap_or_default();
        let waypoint_of_demand_markets = demand_markets.iter().map(|(wps, _)| wps.clone()).collect();
        let maybe_closest = get_closest_waypoint(ship_location, &waypoint_map, waypoint_of_demand_markets);
        if let Some(closest_wp) = maybe_closest {
            let closest_delivery_market_entry = demand_markets
                .iter()
                .find_map(|(wps, mtg)| (wps == &closest_wp.symbol).then_some(mtg.clone()))
                .unwrap();
            let sell_batches = crate::calc_batches_based_on_volume_constraint(*quantity, closest_delivery_market_entry.trade_volume as u32);
            for (idx, sell_batch) in sell_batches.iter().enumerate() {
                let expected_price_per_unit: Credits = ((closest_delivery_market_entry.sell_price as f64 / 1.02f64.powi(idx as i32)).round() as i64).into();
                let total = expected_price_per_unit * *sell_batch;
                sell_ticket_details.push(SellTradeGoodsTicketDetails {
                    waypoint_symbol: closest_wp.symbol.clone(),
                    trade_good: closest_delivery_market_entry.symbol.clone(),
                    expected_price_per_unit,
                    quantity: *sell_batch,
                    expected_total_sell_price: total,
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
    fn test_calculate_necessary_tickets_for_contract_should_use_existing_cargo_if_possible() {
        let test_universe = get_in_memory_universe();
        let test_market_entries = get_test_market_entries(&test_universe);
        let test_waypoints = get_test_waypoints(&test_universe);

        let test_contract = create_test_contract();

        // 95x IRON contract
        // cargo capacity 80
        // 25x IRON already in inventory
        // --> only create purchase ticket for 70x IRON (60 and 10 due to trade volume)
        let test_cargo_with_microprocessors = create_test_cargo(vec![(TradeGoodSymbol::IRON, 25)], 80);
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

        let actual_purchase_ticket_quantities = result
            .purchase_tickets
            .iter()
            .map(|t| (t.trade_good.clone(), t.quantity.clone()))
            .sorted()
            .collect_vec();

        let expected_purchase_ticket_quantities = vec![
            (TradeGoodSymbol::IRON, 60),
            (TradeGoodSymbol::IRON, 10),
            (TradeGoodSymbol::COPPER, 35),
        ]
        .into_iter()
        .sorted()
        .collect_vec();

        assert_eq!(actual_purchase_ticket_quantities, expected_purchase_ticket_quantities);

        let actual_delivery_ticket_quantities = result
            .delivery_tickets
            .iter()
            .map(|t| (t.trade_good.clone(), t.quantity.clone()))
            .sorted()
            .collect_vec();

        let expected_delivery_ticket_quantities = vec![
            (TradeGoodSymbol::IRON, 60),
            (TradeGoodSymbol::IRON, 10),
            (TradeGoodSymbol::IRON, 25),
            (TradeGoodSymbol::COPPER, 35),
        ]
        .into_iter()
        .sorted()
        .collect_vec();

        assert_eq!(actual_delivery_ticket_quantities, expected_delivery_ticket_quantities);
    }

    #[test]
    fn test_split_cargo_into_usable_and_excess_entries_for_contract_usage() {
        //            cargo: 105x iron, 35x microprocessors
        // contract demands: 95x iron and 35x copper

        // expected:
        // usable: 95x iron
        // excess: 10x iron and 35x microprocessors

        let test_contract = create_test_contract();
        let test_cargo = create_test_cargo(vec![(TradeGoodSymbol::IRON, 105), (TradeGoodSymbol::MICROPROCESSORS, 35)], 160);

        let actual = split_cargo_into_usable_and_excess_entries_for_contract_usage(&test_cargo, &test_contract);
        let expected = SplitCargoForContractUsageResult {
            excess: HashMap::from([(TradeGoodSymbol::MICROPROCESSORS, 35), (TradeGoodSymbol::IRON, 10)]),
            usable_for_contract: HashMap::from([(TradeGoodSymbol::IRON, (95, WaypointSymbol("X1-AD75-I1".to_string())))]),
            still_open_contract_entries: HashMap::from([(TradeGoodSymbol::COPPER, (35, WaypointSymbol("X1-AD75-C1".to_string())))]),
        };

        assert_eq!(actual, expected);
    }

    #[test]
    fn test_split_cargo_into_usable_and_excess_entries_for_contract_usage_2() {
        //            cargo: 85x iron, 35x microprocessors
        // contract demands: 95x iron and 35x copper

        // expected:
        // usable: 85x iron (all of cargo)
        // excess: 35x microprocessors

        let test_contract = create_test_contract();
        let test_cargo = create_test_cargo(vec![(TradeGoodSymbol::IRON, 85), (TradeGoodSymbol::MICROPROCESSORS, 35)], 160);

        let actual = split_cargo_into_usable_and_excess_entries_for_contract_usage(&test_cargo, &test_contract);
        let expected = SplitCargoForContractUsageResult {
            excess: HashMap::from([(TradeGoodSymbol::MICROPROCESSORS, 35)]),
            usable_for_contract: HashMap::from([(TradeGoodSymbol::IRON, (85, WaypointSymbol("X1-AD75-I1".to_string())))]),
            still_open_contract_entries: HashMap::from([
                (TradeGoodSymbol::IRON, (10, WaypointSymbol("X1-AD75-I1".to_string()))),
                (TradeGoodSymbol::COPPER, (35, WaypointSymbol("X1-AD75-C1".to_string()))),
            ]),
        };

        assert_eq!(actual, expected);
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
                        destination_symbol: WaypointSymbol("X1-AD75-I1".to_string()),
                        units_required: 95,
                        units_fulfilled: 0,
                    },
                    Delivery {
                        trade_symbol: TradeGoodSymbol::COPPER,
                        destination_symbol: WaypointSymbol("X1-AD75-C1".to_string()),
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
