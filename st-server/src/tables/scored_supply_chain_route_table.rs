use itertools::Itertools;
use leptos::prelude::*;
use leptos_struct_table::*;
use serde::{Deserialize, Serialize};
use st_domain::{
    ActivityLevel, MarketTradeGood, ScoredSupplyChainSupportRoute, SupplyLevel, TradeGoodSymbol, TradeGoodType, TradingOpportunity, WaypointSymbol,
};

use crate::tables::renderers::*;

#[derive(Serialize, Deserialize, Clone, Debug, TableRow)]
#[table(impl_vec_data_provider, sortable, classes_provider = "TailwindClassesPreset")]
pub struct ScoredSupplyChainRouteRow {
    // Purchase location info
    #[table(renderer = "TradeGoodSymbolCellRenderer")]
    pub trade_good: TradeGoodSymbol,
    #[table(renderer = "WaypointSymbolCellRenderer")]
    pub purchase_waypoint_symbol: WaypointSymbol,
    #[table(renderer = "WaypointSymbolCellRenderer")]
    pub delivery_waypoint_symbol: WaypointSymbol,
    #[table(renderer = "TradeGoodSymbolCellRenderer")]
    pub producing_trade_good: TradeGoodSymbol,
    pub priorities_of_chains_containing_this_route: String,
    #[table(class = "text-right")]
    pub delivery_market_export_volume: i32,
    #[table(class = "text-right")]
    pub delivery_market_import_volume: i32,
    pub is_import_volume_too_low: bool,
    #[table(renderer = "SupplyLevelCellRenderer")]
    pub supply_level_at_source: SupplyLevel,
    #[table(renderer = "ActivityLevelCellRenderer")]
    pub activity_level_at_source: Option<ActivityLevel>,
    #[table(renderer = "SupplyLevelCellRenderer")]
    pub supply_level_of_import_at_destination: SupplyLevel,
    #[table(renderer = "ActivityLevelCellRenderer")]
    pub activity_level_of_import_at_destination: Option<ActivityLevel>,
    #[table(class = "text-right")]
    pub supply_level_score: i32,
    #[table(class = "text-right")]
    pub activity_level_score: i32,
    #[table(class = "text-right")]
    pub level_score: i32,
    #[table(class = "text-right")]
    pub max_prio_score: u32,
    #[table(renderer = "PriceCellRenderer", class = "text-right")]
    pub purchase_price: i32,
    #[table(renderer = "PriceCellRenderer", class = "text-right")]
    pub sell_price: i32,
    #[table(renderer = "PriceCellRenderer", class = "text-right")]
    pub spread: i32,
    #[table(class = "text-right")]
    pub num_parallel_pickups: u32,
    #[table(class = "text-right")]
    pub score: i32,
}

impl From<ScoredSupplyChainSupportRoute> for ScoredSupplyChainRouteRow {
    fn from(route: ScoredSupplyChainSupportRoute) -> Self {
        ScoredSupplyChainRouteRow {
            trade_good: route.tgr.trade_good.clone(),
            purchase_waypoint_symbol: route.tgr.source_location.clone(),
            delivery_waypoint_symbol: route.tgr.delivery_location.clone(),
            producing_trade_good: route.tgr.producing_trade_good.clone(),
            priorities_of_chains_containing_this_route: route
                .priorities_of_chains_containing_this_route
                .iter()
                .sorted()
                .join(", "),
            //source_market: route.source_market.clone(),
            delivery_market_export_volume: route.delivery_market_export_volume.clone(),
            delivery_market_import_volume: route.delivery_market_import_volume.clone(),
            is_import_volume_too_low: route.is_import_volume_too_low.clone(),
            supply_level_at_source: route.supply_level_at_source.clone(),
            activity_level_at_source: route.activity_level_at_source.clone(),
            supply_level_of_import_at_destination: route.supply_level_of_import_at_destination.clone(),
            activity_level_of_import_at_destination: route.activity_level_of_import_at_destination.clone(),
            supply_level_score: route.supply_level_score.clone(),
            activity_level_score: route.activity_level_score.clone(),
            level_score: route.level_score.clone(),
            max_prio_score: route.max_prio_score.clone(),
            purchase_price: route.purchase_price.clone(),
            sell_price: route.sell_price.clone(),
            spread: route.spread.clone(),
            num_parallel_pickups: route.num_parallel_pickups.clone(),
            score: route.score.clone(),
        }
    }
}
