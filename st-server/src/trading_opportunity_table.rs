use leptos::prelude::*;
use leptos::prelude::{RwSignal, Signal};
use leptos::{component, IntoView};
use leptos_struct_table::*;
use serde::{Deserialize, Serialize};
use st_domain::{ActivityLevel, SupplyLevel, TradeGoodType, TradingOpportunity, WaypointSymbol};

#[derive(Serialize, Deserialize, Clone, Debug, TableRow)]
#[table(
    impl_vec_data_provider,
    sortable,
    classes_provider = "TailwindClassesPreset"
)]
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
            trade_good_symbol: opportunity.purchase_market_trade_good_entry.symbol,
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

            profit: opportunity.profit,
        }
    }
}

// WaypointSymbolCellRenderer (you already had this one)
#[component]
fn WaypointSymbolCellRenderer(
    class: String,
    value: Signal<WaypointSymbol>,
    row: RwSignal<TradingOpportunityRow>,
    index: usize,
) -> impl IntoView {
    view! { <td class=class>{move || value.get_untracked().0}</td> }
}

// TradeGoodTypeCellRenderer
#[component]
fn TradeGoodTypeCellRenderer(
    class: String,
    value: Signal<TradeGoodType>,
    row: RwSignal<TradingOpportunityRow>,
    index: usize,
) -> impl IntoView {
    view! { <td class=class>{move || format!("{:?}", value.get_untracked())}</td> }
}

// SupplyLevelCellRenderer
#[component]
fn SupplyLevelCellRenderer(
    class: String,
    value: Signal<SupplyLevel>,
    row: RwSignal<TradingOpportunityRow>,
    index: usize,
) -> impl IntoView {
    let supply_class = move || match value.get_untracked() {
        SupplyLevel::Scarce => "text-red-600 font-bold",
        SupplyLevel::Limited => "text-orange-500",
        SupplyLevel::Moderate => "text-blue-500",
        SupplyLevel::Abundant => "text-green-500",
        _ => "",
    };

    view! {
        <td class=class>
            <span class=supply_class()>{move || format!("{:?}", value.get_untracked())}</span>
        </td>
    }
}

// ActivityLevelCellRenderer
#[component]
fn ActivityLevelCellRenderer(
    class: String,
    value: Signal<Option<ActivityLevel>>,
    row: RwSignal<TradingOpportunityRow>,
    index: usize,
) -> impl IntoView {
    let activity_text = move || match value.get_untracked() {
        Some(activity) => format!("{:?}", activity),
        None => "N/A".to_string(),
    };

    let activity_class = move || match value.get_untracked() {
        Some(ActivityLevel::Strong) => "text-green-600 font-bold",
        Some(ActivityLevel::Growing) => "text-green-500",
        Some(ActivityLevel::Weak) => "text-red-500",
        Some(ActivityLevel::Restricted) => "text-red-600 font-bold",
        _ => "",
    };

    view! {
        <td class=class>
            <span class=activity_class()>{activity_text()}</span>
        </td>
    }
}

// ProfitCellRenderer
#[component]
fn ProfitCellRenderer(
    class: String,
    value: Signal<u64>,
    row: RwSignal<TradingOpportunityRow>,
    index: usize,
) -> impl IntoView {
    let profit_class = move || {
        let profit = value.get_untracked();
        if profit > 10000 {
            "text-green-600 font-bold"
        } else if profit > 5000 {
            "text-green-500"
        } else if profit > 1000 {
            "text-blue-500"
        } else {
            "text-gray-500"
        }
    };

    view! {
        <td class=class>
            <span class=profit_class()>{move || format!("{} cr", value.get_untracked())}</span>
        </td>
    }
}

// PriceCellRenderer (for purchase_price and sell_price)
#[component]
fn PriceCellRenderer(
    class: String,
    value: Signal<i32>,
    row: RwSignal<TradingOpportunityRow>,
    index: usize,
) -> impl IntoView {
    view! { <td class=class>{move || format!("{} cr", value.get_untracked())}</td> }
}

// TradeVolumeCellRenderer
#[component]
fn TradeVolumeCellRenderer(
    class: String,
    value: Signal<i32>,
    row: RwSignal<TradingOpportunityRow>,
    index: usize,
) -> impl IntoView {
    view! { <td class=class>{move || format!("{} units", value.get_untracked())}</td> }
}
