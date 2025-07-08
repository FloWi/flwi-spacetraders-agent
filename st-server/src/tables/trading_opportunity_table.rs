use serde::{Deserialize, Serialize};
use st_domain::{ActivityLevel, SupplyLevel, TradeGoodType, TradingOpportunity, WaypointSymbol};

// IMPORTANT: all these imports are required, dear copy-and-paster
use crate::tables::renderers::*;
use crate::tailwind::TailwindClassesPreset;
#[allow(unused_variables)]
use leptos::prelude::*;
use leptos_struct_table::*;

#[derive(Serialize, Deserialize, Clone, Debug, TableRow)]
#[table(impl_vec_data_provider, sortable, classes_provider = "TailwindClassesPreset")]
pub struct TradingOpportunityRow {
    pub trade_good_symbol: String,

    // Purchase location info
    #[table(renderer = "WaypointSymbolCellRenderer")]
    pub purchase_waypoint_symbol: WaypointSymbol,

    #[table(renderer = "WaypointSymbolCellRenderer")]
    pub sell_waypoint_symbol: WaypointSymbol,

    #[table(renderer = "TradeGoodTypeCellRenderer")]
    pub purchase_trade_good_type: TradeGoodType,

    #[table(renderer = "TradeGoodTypeCellRenderer")]
    pub sell_trade_good_type: TradeGoodType,

    #[table(renderer = "TradeVolumeCellRenderer")]
    pub purchase_trade_volume: i32,

    #[table(renderer = "TradeVolumeCellRenderer")]
    pub sell_trade_volume: i32,

    #[table(renderer = "SupplyLevelCellRenderer")]
    pub purchase_supply: SupplyLevel,

    #[table(renderer = "SupplyLevelCellRenderer")]
    pub sell_supply: SupplyLevel,

    #[table(renderer = "ActivityLevelCellRenderer")]
    pub purchase_activity: Option<ActivityLevel>,

    #[table(renderer = "ActivityLevelCellRenderer")]
    pub sell_activity: Option<ActivityLevel>,

    #[table(renderer = "PriceCellRenderer", class = "text-right")]
    pub purchase_price: i32,

    // Sell location info
    #[table(renderer = "PriceCellRenderer", class = "text-right")]
    pub sell_price: i32,

    #[table(renderer = "ProfitCellRenderer", class = "text-right")]
    pub profit: u64,

    // Calculated profit (default sort field)
    #[table(renderer = "FloatCellRenderer", class = "text-right")]
    pub profit_per_unit_per_distance: f64,

    #[table(class = "text-right")]
    pub distance: u32,
}

impl From<TradingOpportunity> for TradingOpportunityRow {
    fn from(opportunity: TradingOpportunity) -> Self {
        TradingOpportunityRow {
            purchase_waypoint_symbol: opportunity.purchase_waypoint_symbol,
            trade_good_symbol: opportunity
                .purchase_market_trade_good_entry
                .symbol
                .to_string(),
            purchase_trade_good_type: opportunity.purchase_market_trade_good_entry.trade_good_type,
            purchase_trade_volume: opportunity.purchase_market_trade_good_entry.trade_volume,
            purchase_supply: opportunity.purchase_market_trade_good_entry.supply,
            purchase_activity: opportunity.purchase_market_trade_good_entry.activity,
            purchase_price: opportunity.purchase_market_trade_good_entry.purchase_price,

            sell_waypoint_symbol: opportunity.sell_waypoint_symbol,
            sell_trade_good_type: opportunity.sell_market_trade_good_entry.trade_good_type,
            sell_trade_volume: opportunity.sell_market_trade_good_entry.trade_volume,
            sell_supply: opportunity.sell_market_trade_good_entry.supply,
            sell_activity: opportunity.sell_market_trade_good_entry.activity,
            sell_price: opportunity.sell_market_trade_good_entry.sell_price,

            profit: opportunity.profit_per_unit,
            profit_per_unit_per_distance: opportunity.profit_per_unit_per_distance.0,
            distance: opportunity.direct_distance,
        }
    }
}
