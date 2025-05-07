use leptos::prelude::*;
use leptos::*;
use st_domain::{ActivityLevel, SupplyLevel, TradeGoodSymbol, TradeGoodType, WaypointSymbol};
use thousands::Separable;

// WaypointSymbolCellRenderer (you already had this one)
#[component]
pub fn WaypointSymbolCellRenderer<F>(class: String, value: Signal<WaypointSymbol>, row: RwSignal<F>, index: usize) -> impl IntoView {
    view! { <td class=class>{move || value.get_untracked().symbol_ex_system_symbol()}</td> }
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
        SupplyLevel::Scarce => "text-red-600",       // Bright red for urgency/scarcity
        SupplyLevel::Limited => "text-amber-500",    // Amber/orange for caution
        SupplyLevel::Moderate => "text-yellow-500",  // Yellow for neutral/average
        SupplyLevel::High => "text-blue-500",        // Blue for good supply
        SupplyLevel::Abundant => "text-emerald-600", // Emerald green for abundance
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

    let activity_class = move || {
        value
            .get_untracked()
            .map(|act| match act {
                ActivityLevel::Strong => "text-emerald-600", // Emerald green for excellent activity
                ActivityLevel::Growing => "text-blue-500",   // Blue for positive growth
                ActivityLevel::Weak => "text-amber-500",     // Amber for caution/weak
                ActivityLevel::Restricted => "text-red-600", // Red for restricted/problematic
            })
            .unwrap_or("")
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
    view! { <td class=class>{move || format!("{}c", value.get_untracked().separate_with_commas())}</td> }
}

// TradeVolumeCellRenderer
#[component]
pub fn TradeVolumeCellRenderer<F>(class: String, value: Signal<i32>, row: RwSignal<F>, index: usize) -> impl IntoView {
    view! { <td class=class>{move || format!("{}", value.get_untracked())}</td> }
}

// TradeGoodSymbolCellRenderer
#[component]
pub fn TradeGoodSymbolCellRenderer<F>(class: String, value: Signal<TradeGoodSymbol>, row: RwSignal<F>, index: usize) -> impl IntoView {
    view! { <td class=class>{move || format!("{}", value.get_untracked().to_string())}</td> }
}
