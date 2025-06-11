use itertools::Itertools;

use serde::{Deserialize, Serialize};
use st_domain::TradeGoodSymbol;

// IMPORTANT: all these imports are required, dear copy-and-paster
use crate::tables::renderers::*;
use crate::tailwind::TailwindClassesPreset;
use leptos::prelude::*;
use leptos_struct_table::*;

#[derive(Serialize, Deserialize, Clone, Debug, TableRow)]
#[table(impl_vec_data_provider, sortable, classes_provider = "TailwindClassesPreset")]
pub struct TradeGoodsOverviewRow {
    pub label: String,

    #[table(class = "text-right")]
    pub number_trade_goods: usize,

    #[table(renderer = "TradeGoodSymbolListCellRenderer")]
    pub trade_goods: Vec<TradeGoodSymbol>,
}

impl TradeGoodsOverviewRow {
    pub(crate) fn new<'a, I>(label: String, trade_goods: I) -> Self
    where
        I: Iterator<Item = &'a TradeGoodSymbol>,
    {
        let vec = trade_goods.into_iter().cloned().collect_vec();
        Self {
            label,
            number_trade_goods: vec.len(),
            trade_goods: vec,
        }
    }
}
