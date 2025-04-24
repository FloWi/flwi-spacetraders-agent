use chrono::Duration;

pub mod app;
pub mod behavior_tree_page;
pub mod db_overview_page;
pub mod fleet_overview_page;
pub mod supply_chain_page;

#[cfg(feature = "ssr")]
pub mod cli_args;
pub mod components;
mod petgraph_example_page;
pub mod ship_overview_page;
pub mod tailwind;
mod trading_opportunity_table;
mod treasurer_experiment_page;

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    use crate::app::*;
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(App);
}

fn format_duration(duration: &Duration) -> String {
    // Get total seconds
    let total_seconds = duration.num_seconds();

    // Calculate hours, minutes and seconds
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    // Format as hh:mm:ss
    format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
}
