use itertools::Itertools;
use leptos::prelude::*;
use st_domain::budgeting::credits::Credits;
use st_domain::{ActivityLevel, SupplyLevel, TradeGoodSymbol, TradeGoodType, WaypointSymbol};
use thousands::Separable;

// WaypointSymbolCellRenderer (you already had this one)
#[component]
pub fn WaypointSymbolCellRenderer<F: 'static>(
    class: String,
    value: Signal<WaypointSymbol>,
    #[allow(unused_variables)] row: RwSignal<F>,
    #[allow(unused_variables)] index: usize,
) -> impl IntoView {
    view! { <td class=class>{move || value.get_untracked().symbol_ex_system_symbol()}</td> }
}

// TradeGoodTypeCellRenderer
#[component]
pub fn TradeGoodTypeCellRenderer<F: 'static>(
    class: String,
    value: Signal<TradeGoodType>,
    #[allow(unused_variables)] row: RwSignal<F>,
    #[allow(unused_variables)] index: usize,
) -> impl IntoView {
    view! { <td class=class>{move || format!("{:?}", value.get_untracked())}</td> }
}

// SupplyLevelCellRenderer
#[component]
pub fn SupplyLevelCellRenderer<F: 'static>(
    class: String,
    value: Signal<SupplyLevel>,
    #[allow(unused_variables)] row: RwSignal<F>,
    #[allow(unused_variables)] index: usize,
) -> impl IntoView {
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
pub fn ActivityLevelCellRenderer<F: 'static>(
    class: String,
    value: Signal<Option<ActivityLevel>>,
    #[allow(unused_variables)] row: RwSignal<F>,
    #[allow(unused_variables)] index: usize,
) -> impl IntoView {
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
pub fn ProfitCellRenderer<F: 'static>(
    class: String,
    value: Signal<u64>,
    #[allow(unused_variables)] row: RwSignal<F>,
    #[allow(unused_variables)] index: usize,
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
            <span class=profit_class()>
                {move || format!("{}c", value.get_untracked().separate_with_commas())}
            </span>
        </td>
    }
}

// ProfitCellRenderer
#[component]
pub fn FloatCellRenderer<F: 'static>(
    class: String,
    value: Signal<f64>,
    #[allow(unused_variables)] row: RwSignal<F>,
    #[allow(unused_variables)] index: usize,
) -> impl IntoView {
    view! {
        <td class=class>
            <span>{move || format!("{}c", format_number(value.get_untracked()))}</span>
        </td>
    }
}

/// Print a number with 2 decimal places and comma-separated
pub fn format_number(value: f64) -> String {
    // thousands will format floating point numbers just fine, but we can't
    // format the number this way _and_ specify the precision. So we're going
    // to separate out the fractional part and format that separately, but this
    // means we have to count the digits in the fractional part (up to 2).
    let fractional = ((value - value.floor()) * 100.0).round() as u64;
    let separated = (value.floor() as i64).separate_with_commas();

    // because we multiply the fractional component by only 100.0, we can only
    // ever have up to 2 digits.
    let num_digits = fractional.checked_ilog10().unwrap_or_default() + 1;
    match num_digits {
        1 => format!("{}.0{}", separated, fractional),
        _ => format!("{}.{}", separated, fractional),
    }
}

// PriceCellRenderer (for purchase_price and sell_price)
#[component]
pub fn PriceCellRenderer<F: 'static>(
    class: String,
    value: Signal<i32>,
    #[allow(unused_variables)] row: RwSignal<F>,
    #[allow(unused_variables)] index: usize,
) -> impl IntoView {
    view! {
        <td class=class>{move || format!("{}c", value.get_untracked().separate_with_commas())}</td>
    }
}

// CreditCellRenderer
#[component]
pub fn CreditCellRenderer<F: 'static>(
    class: String,
    value: Signal<Credits>,
    #[allow(unused_variables)] row: RwSignal<F>,
    #[allow(unused_variables)] index: usize,
) -> impl IntoView {
    view! {
        <td class=class>
            {move || format!("{}c", value.get_untracked().0.separate_with_commas())}
        </td>
    }
}

// TradeVolumeCellRenderer
#[component]
pub fn TradeVolumeCellRenderer<F: 'static>(
    class: String,
    value: Signal<i32>,
    #[allow(unused_variables)] row: RwSignal<F>,
    #[allow(unused_variables)] index: usize,
) -> impl IntoView {
    view! { <td class=class>{move || format!("{}", value.get_untracked())}</td> }
}

// TradeGoodSymbolCellRenderer
#[component]
pub fn TradeGoodSymbolCellRenderer<F: 'static>(
    class: String,
    value: Signal<TradeGoodSymbol>,
    #[allow(unused_variables)] row: RwSignal<F>,
    #[allow(unused_variables)] index: usize,
) -> impl IntoView {
    view! { <td class=class>{move || format!("{}", value.get_untracked())}</td> }
}

// TradeGoodSymbolListCellRenderer
#[component]
pub fn TradeGoodSymbolListCellRenderer<F: 'static>(
    class: String,
    value: Signal<Vec<TradeGoodSymbol>>,
    #[allow(unused_variables)] row: RwSignal<F>,
    #[allow(unused_variables)] index: usize,
) -> impl IntoView {
    view! {
        <td class=class>
            {move || {
                value
                    .get_untracked()
                    .iter()
                    .map(|tg| tg.to_string())
                    .sorted()
                    .join(", ")
                    .to_string()
            }}
        </td>
    }
}
