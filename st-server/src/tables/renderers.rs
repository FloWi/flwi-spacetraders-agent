use leptos::prelude::*;
use leptos::*;
use st_domain::{ActivityLevel, SupplyLevel, TradeGoodSymbol, TradeGoodType, WaypointSymbol};

// WaypointSymbolCellRenderer (you already had this one)
#[component]
pub fn WaypointSymbolCellRenderer<F>(class: String, value: Signal<WaypointSymbol>, row: RwSignal<F>, index: usize) -> impl IntoView {
    view! { <td class=class>{move || value.get_untracked().0}</td> }
}

// TradeGoodTypeCellRenderer
#[component]
pub fn TradeGoodTypeCellRenderer<F>(class: String, value: Signal<TradeGoodType>, row: RwSignal<F>, index: usize) -> impl IntoView {
    view! { <td class=class>{move || format!("{:?}", value.get_untracked())}</td> }
}

// SupplyLevelCellRenderer
#[component]
pub fn SupplyLevelCellRenderer<F>(class: String, value: Signal<SupplyLevel>, row: RwSignal<F>, index: usize) -> impl IntoView {
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
pub fn ActivityLevelCellRenderer<F>(class: String, value: Signal<Option<ActivityLevel>>, row: RwSignal<F>, index: usize) -> impl IntoView {
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
pub fn ProfitCellRenderer<F>(class: String, value: Signal<u64>, row: RwSignal<F>, index: usize) -> impl IntoView {
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
pub fn PriceCellRenderer<F>(class: String, value: Signal<i32>, row: RwSignal<F>, index: usize) -> impl IntoView {
    view! { <td class=class>{move || format!("{} cr", value.get_untracked())}</td> }
}

// TradeVolumeCellRenderer
#[component]
pub fn TradeVolumeCellRenderer<F>(class: String, value: Signal<i32>, row: RwSignal<F>, index: usize) -> impl IntoView {
    view! { <td class=class>{move || format!("{} units", value.get_untracked())}</td> }
}

// TradeGoodSymbolCellRenderer
#[component]
pub fn TradeGoodSymbolCellRenderer<F>(class: String, value: Signal<TradeGoodSymbol>, row: RwSignal<F>, index: usize) -> impl IntoView {
    view! { <td class=class>{move || format!("{}", value.get_untracked().to_string())}</td> }
}
