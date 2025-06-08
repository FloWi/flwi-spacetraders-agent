use crate::calc_batches_based_on_volume_constraint;
use anyhow::{anyhow, Result};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use st_domain::budgeting::credits::Credits;
use st_domain::budgeting::treasury_redesign::{DeliverCargoContractTicketDetails, PurchaseCargoReason, PurchaseTradeGoodsTicketDetails};
use st_domain::trading::group_markets_by_type;
use st_domain::{
    combine_maps, trading, Contract, ContractEvaluationResult, ContractId, MarketEntry, MarketTradeGood, TradeGoodSymbol, TradeGoodType, WaypointSymbol,
};
use std::collections::HashMap;
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

pub fn calculate_necessary_purchase_tickets_for_contract(
    ship_cargo_size: u32,
    contract: &Contract,
    latest_market_entries: &[MarketEntry],
) -> Result<ContractEvaluationResult> {
    let trading_entries = trading::to_trade_goods_with_locations(latest_market_entries);

    let export_markets: HashMap<TradeGoodSymbol, Vec<(WaypointSymbol, MarketTradeGood)>> = group_markets_by_type(&trading_entries, TradeGoodType::Export);
    let exchange_markets: HashMap<TradeGoodSymbol, Vec<(WaypointSymbol, MarketTradeGood)>> = group_markets_by_type(&trading_entries, TradeGoodType::Exchange);

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
            })
        }

        let delivery_batches = calc_batches_based_on_volume_constraint(quantity, ship_cargo_size);
        for delivery_batch in delivery_batches {
            delivery_tickets.push(DeliverCargoContractTicketDetails {
                waypoint_symbol: deliver.destination_symbol.clone(),
                trade_good: deliver.trade_symbol.clone(),
                quantity: delivery_batch,
                contract_id: contract.id.clone(),
            })
        }
    }

    Ok(ContractEvaluationResult {
        purchase_tickets,
        delivery_tickets,
        contract: contract.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use st_domain::{ActivityLevel, ContractId, ContractTerms, Delivery, MarketData, Payment, SupplyLevel, TradeGood};

    #[test]
    fn test_calculate_necessary_purchase_tickets_for_contract() {
        /*
        Scenario:
        contract is 45 IRON and 35 COPPER
        cargo capacity is 40
        trade_volume for COPPER is 40
        trade_volume for IRON is 20
        */
        let test_market_entries = create_test_market_entries();
        let test_contract = create_test_contract();

        let result = calculate_necessary_purchase_tickets_for_contract(40, &test_contract, &test_market_entries).unwrap();
        let ContractEvaluationResult {
            purchase_tickets,
            delivery_tickets,
            contract,
        } = result.clone();

        let actual_purchase_ticket_quantities = purchase_tickets
            .iter()
            .map(|t| (t.trade_good.clone(), t.quantity.clone()))
            .sorted()
            .collect_vec();

        let expected_purchase_ticket_quantities = vec![
            (TradeGoodSymbol::IRON, 20),
            (TradeGoodSymbol::IRON, 20),
            (TradeGoodSymbol::IRON, 5),
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
            (TradeGoodSymbol::IRON, 40),
            (TradeGoodSymbol::IRON, 5),
            (TradeGoodSymbol::COPPER, 35),
        ]
        .into_iter()
        .sorted()
        .collect_vec();

        assert_eq!(actual_delivery_ticket_quantities, expected_delivery_ticket_quantities);

        // IRON:
        // batch #0: 20 * 60c/unit * 1.02 ^ 0 =           = 20 * 60 = 1_200
        // batch #1: 20 * 60c/unit * 1.02 ^ 1 = 20 * 61.2 = 20 * 62 = 1_240
        // batch #2:  5 * 60c/unit * 1.02 ^ 2 =  5 * 62,4 =  5 * 63 =   315
        //                                                          = 2_755
        // COPPER:
        // batch #0: 35 * 50c/unit * 1.02 ^ 0 = 1_750

        // total:
        //   2_755
        // + 1_750
        // = 4_505
        let actual_estimated_purchase_costs = result.estimated_purchase_costs();

        // purchase_total:  4_505
        // on_accepted:    -1_234
        //               =  3_271

        let actual_required_capital = result.required_capital();

        assert_eq!(actual_estimated_purchase_costs, 4_505.into());
        assert_eq!(actual_required_capital, 3_271.into());
    }

    fn create_test_contract() -> Contract {
        Contract {
            id: ContractId("contract-id-foo".to_string()),
            faction_symbol: "LORDS".to_string(),
            contract_type: "contract_type".to_string(),
            terms: ContractTerms {
                deadline: Default::default(),
                payment: Payment {
                    on_accepted: 1_234,
                    on_fulfilled: 2_345,
                },
                deliver: vec![
                    Delivery {
                        trade_symbol: TradeGoodSymbol::IRON,
                        destination_symbol: WaypointSymbol("X1-CONTRACT-DELIVERY".to_string()),
                        units_required: 45,
                        units_fulfilled: 0,
                    },
                    Delivery {
                        trade_symbol: TradeGoodSymbol::COPPER,
                        destination_symbol: WaypointSymbol("X1-CONTRACT-DELIVERY".to_string()),
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

    fn create_test_market_entries() -> Vec<MarketEntry> {
        let iron_export_1 = create_export_market_entry(
            WaypointSymbol("X1-IRON-ONE".to_string()),
            TradeGoodSymbol::IRON,
            20,
            SupplyLevel::Moderate,
            None,
            60,
            30,
        );

        let iron_export_2 = create_export_market_entry(
            WaypointSymbol("X1-IRON-TWO".to_string()),
            TradeGoodSymbol::IRON,
            20,
            SupplyLevel::Moderate,
            None,
            70,
            35,
        );

        let copper_export_1 = create_export_market_entry(
            WaypointSymbol("X1-COPPER-ONE".to_string()),
            TradeGoodSymbol::COPPER,
            40,
            SupplyLevel::Moderate,
            None,
            50,
            25,
        );

        vec![iron_export_1, iron_export_2, copper_export_1]
    }

    fn create_export_market_entry(
        waypoint_symbol: WaypointSymbol,
        trade_good_symbol: TradeGoodSymbol,
        trade_volume: i32,
        supply_level: SupplyLevel,
        activity_level: Option<ActivityLevel>,
        purchase_price: i32,
        sell_price: i32,
    ) -> MarketEntry {
        MarketEntry {
            waypoint_symbol: waypoint_symbol.clone(),
            market_data: MarketData {
                symbol: waypoint_symbol,
                exports: vec![TradeGood {
                    symbol: trade_good_symbol.clone(),
                    name: trade_good_symbol.to_string(),
                    description: trade_good_symbol.to_string(),
                }],
                imports: vec![],
                exchange: vec![],
                transactions: None,
                trade_goods: Some(vec![MarketTradeGood {
                    symbol: trade_good_symbol,
                    trade_good_type: TradeGoodType::Export,
                    trade_volume,
                    supply: supply_level,
                    activity: activity_level,
                    purchase_price,
                    sell_price,
                }]),
            },
            created_at: Default::default(),
        }
    }
}
