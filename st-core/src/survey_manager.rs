use itertools::Itertools;
use st_domain::{MaterializedSupplyChain, Survey, TradeGoodSymbol};
use std::collections::HashMap;
use strum::IntoEnumIterator;

pub(crate) fn pick_best_survey(all_surveys: Vec<Survey>, materialized_supply_chain: &MaterializedSupplyChain) -> Option<(Survey, i32)> {
    let demand_for_raw_materials = materialized_supply_chain.calc_demand_for_raw_materials();

    let scored_raw_products: HashMap<TradeGoodSymbol, i32> = demand_for_raw_materials
        .iter()
        .map(|foo| (foo.trade_good_symbol.clone(), foo.score))
        .collect();

    let scored_surveys: Vec<(Survey, i32)> = all_surveys
        .iter()
        .map(|s| {
            let item_counts: HashMap<TradeGoodSymbol, usize> = s.deposits.iter().map(|d| d.symbol.clone()).counts();

            let survey_score = item_counts
                .iter()
                .map(|(tgs, num_occurrences_in_survey)| scored_raw_products.get(tgs).cloned().unwrap_or(0) * (*num_occurrences_in_survey as i32))
                .sum::<i32>();

            (s.clone(), survey_score)
        })
        .sorted_by_key(|(_, score)| -*score)
        .collect_vec();

    let maybe_best: Option<(Survey, i32)> = scored_surveys.first().cloned();

    maybe_best
}
