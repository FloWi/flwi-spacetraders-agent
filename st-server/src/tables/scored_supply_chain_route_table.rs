use leptos::prelude::*;
use leptos_struct_table::*;
use serde::{Deserialize, Serialize};
use st_domain::{ActivityLevel, ScoredSupplyChainSupportRoute, SupplyLevel, TradeGoodSymbol, TradeGoodType, WaypointSymbol};

use crate::tables::renderers::*;
use crate::tailwind::TailwindClassesPreset;

#[derive(Serialize, Deserialize, Clone, Debug, TableRow)]
#[table(impl_vec_data_provider, sortable, classes_provider = "TailwindClassesPreset")]
pub struct ScoredSupplyChainRouteRow {
    #[table(class = "text-right")]
    pub rank: u32,

    // Purchase location info
    #[table(renderer = "TradeGoodSymbolCellRenderer")]
    pub trade_good: TradeGoodSymbol,

    #[table(renderer = "WaypointSymbolCellRenderer", title = "source wp")]
    pub source_waypoint_symbol: WaypointSymbol,

    #[table(renderer = "TradeGoodTypeCellRenderer", title = "source type")]
    pub source_trade_good_type: TradeGoodType,

    #[table(renderer = "SupplyLevelCellRenderer", title = "source sup.")]
    pub source_supply_level: SupplyLevel,

    #[table(class = "text-right", title = "source vol.")]
    pub source_volume: i32,

    #[table(renderer = "ActivityLevelCellRenderer", title = "source act.")]
    pub source_activity: Option<ActivityLevel>,

    #[table(renderer = "WaypointSymbolCellRenderer")]
    pub delivery_waypoint_symbol: WaypointSymbol,

    #[table(renderer = "TradeGoodTypeCellRenderer")]
    pub delivery_trade_good_type: TradeGoodType,

    #[table(renderer = "TradeGoodSymbolCellRenderer")]
    pub producing_trade_good: TradeGoodSymbol,

    // pub priorities_of_chains_containing_this_route: String,
    #[table(class = "text-right", title = "dest. imp. vol.")]
    pub destination_import_volume: i32,

    #[table(renderer = "SupplyLevelCellRenderer", title = "dest. imp. sup.")]
    pub destination_import_supply: SupplyLevel,

    #[table(renderer = "ActivityLevelCellRenderer", title = "dest. imp. act.")]
    pub destination_import_activity: Option<ActivityLevel>,

    #[table(class = "text-right", title = "dest. exp. vol.")]
    pub destination_export_volume: i32,

    #[table(renderer = "SupplyLevelCellRenderer", title = "dest. exp. sup.")]
    pub destination_export_supply: SupplyLevel,

    #[table(renderer = "ActivityLevelCellRenderer", title = "dest. exp. act.")]
    pub destination_export_activity: Option<ActivityLevel>,

    pub is_import_volume_too_low: bool,

    #[table(class = "text-right")]
    pub import_supply_score: i32,

    #[table(class = "text-right")]
    pub activity_score: i32,

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
            source_waypoint_symbol: route.tgr.source_location.clone(),
            delivery_waypoint_symbol: route.tgr.delivery_location.clone(),
            delivery_trade_good_type: route.tgr.delivery_market_entry.trade_good_type.clone(),
            producing_trade_good: route.tgr.producing_trade_good.clone(),
            // priorities_of_chains_containing_this_route: route
            //     .priorities_of_chains_containing_this_route
            //     .iter()
            //     .sorted()
            //     .join(", "),
            //source_market: route.source_market.clone(),
            destination_export_volume: route.delivery_market_export_volume,
            destination_export_supply: route.tgr.producing_market_entry.supply.clone(),
            destination_export_activity: route.tgr.producing_market_entry.activity.clone(),
            destination_import_volume: route.delivery_market_import_volume,
            is_import_volume_too_low: route.is_import_volume_too_low,
            source_supply_level: route.supply_level_at_source.clone(),
            source_volume: route.source_market.trade_volume,
            source_activity: route.activity_level_at_source.clone(),
            destination_import_supply: route.supply_level_of_import_at_destination.clone(),
            destination_import_activity: route.activity_level_of_import_at_destination.clone(),
            import_supply_score: route.import_supply_level_score,
            activity_score: route.import_activity_level_score,
            level_score: route.level_score,
            max_prio_score: route.max_prio_score,
            purchase_price: route.purchase_price,
            sell_price: route.sell_price,
            spread: route.spread,
            num_parallel_pickups: route.num_allowed_parallel_pickups,
            score: route.score,
            source_trade_good_type: route.source_market.trade_good_type.clone(),
            rank: route.rank,
        }
    }
}
