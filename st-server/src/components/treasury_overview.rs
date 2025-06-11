use crate::tables::fleet_overview_table::FleetOverviewRow;
use itertools::*;
use leptos::html::*;
use leptos::prelude::*;
use leptos::{component, view, IntoView};
use leptos_struct_table::TableContent;
use st_domain::budgeting::treasury_redesign::ImprovedTreasurer;
use st_domain::Fleet;

#[component]
pub fn TreasuryOverview<'a>(treasurer: &'a ImprovedTreasurer, fleets: &'a [Fleet]) -> impl IntoView {
    let fleet_budgets = treasurer.get_fleet_budgets().unwrap_or_default();
    let fleet_overview_table_data: Vec<FleetOverviewRow> = fleet_budgets
        .iter()
        .filter_map(|(fleet_id, fleet_budget)| {
            fleets
                .iter()
                .find(|f| &f.id == fleet_id)
                .map(|fleet| (fleet.clone(), fleet_budget.clone()))
        })
        .sorted_by_key(|(fleet, _)| fleet.id.0.clone())
        .map(|(fleet, fleet_budget)| FleetOverviewRow::from((fleet, fleet_budget)))
        .collect_vec();

    let scroll_container = NodeRef::new();

    view! {
        <div node_ref=scroll_container class="rounded-md overflow-clip border dark:border-gray-700 w-fit mt-4">
            <table class="text-sm text-left mb-[-1px]">
                <TableContent rows=fleet_overview_table_data scroll_container />
            </table>
        </div>
    }
}
