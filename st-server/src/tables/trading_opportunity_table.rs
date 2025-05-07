use leptos::prelude::*;
use leptos_struct_table::*;
use serde::{Deserialize, Serialize};
use st_domain::{ActivityLevel, SupplyLevel, TradeGoodType, TradingOpportunity, WaypointSymbol};

use crate::tables::renderers::*;

#[derive(Serialize, Deserialize, Clone, Debug, TableRow)]
#[table(impl_vec_data_provider, sortable, classes_provider = "TailwindClassesPreset")]
pub struct TradingOpportunityRow {
    // Purchase location info
    #[table(renderer = "WaypointSymbolCellRenderer")]
    pub purchase_waypoint_symbol: WaypointSymbol,

    pub trade_good_symbol: String,

    #[table(renderer = "TradeGoodTypeCellRenderer")]
    pub purchase_trade_good_type: TradeGoodType,

    #[table(renderer = "TradeVolumeCellRenderer")]
    pub purchase_trade_volume: i32,

    #[table(renderer = "SupplyLevelCellRenderer")]
    pub purchase_supply: SupplyLevel,

    #[table(renderer = "ActivityLevelCellRenderer")]
    pub purchase_activity: Option<ActivityLevel>,

    #[table(renderer = "PriceCellRenderer")]
    pub purchase_price: i32,

    // Calculated profit (default sort field)
    #[table(renderer = "ProfitCellRenderer")]
    pub profit: u64,

    // Sell location info
    #[table(renderer = "WaypointSymbolCellRenderer")]
    pub sell_waypoint_symbol: WaypointSymbol,

    #[table(renderer = "TradeGoodTypeCellRenderer")]
    pub sell_trade_good_type: TradeGoodType,

    #[table(renderer = "TradeVolumeCellRenderer")]
    pub sell_trade_volume: i32,

    #[table(renderer = "SupplyLevelCellRenderer")]
    pub sell_supply: SupplyLevel,

    #[table(renderer = "ActivityLevelCellRenderer")]
    pub sell_activity: Option<ActivityLevel>,

    #[table(renderer = "PriceCellRenderer")]
    pub sell_price: i32,
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
        }
    }
}
