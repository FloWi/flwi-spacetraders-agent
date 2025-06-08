pub mod configuration;
pub mod fleet;
pub mod pagination;
pub mod reqwest_helpers;
pub mod ship;
pub mod st_client;

use chrono::TimeDelta;
use itertools::Itertools;
use st_domain::FlightMode;
use std::fmt::Display;

pub mod agent;
pub mod agent_manager;
pub mod app_state;
pub mod behavior_tree;
mod bmc_blackboard;
pub mod exploration;
pub mod in_memory_universe;
pub mod marketplaces;
pub mod pathfinder;
pub mod universe_server;

pub mod contract_manager;
pub mod materialized_supply_chain_manager;
pub mod survey_manager;

#[cfg(test)]
pub mod test_objects;
mod transfer_cargo_manager;

pub fn calculate_fuel_consumption(flight_mode: &FlightMode, distance: u32) -> u32 {
    match flight_mode {
        FlightMode::Drift => 1,
        FlightMode::Cruise => u32::max(1, distance),
        FlightMode::Stealth => u32::max(1, distance),
        FlightMode::Burn => 2 * u32::max(1, distance),
    }
}

pub fn calculate_time(flight_mode: &FlightMode, distance: u32, engine_speed: u32) -> u32 {
    let navigation_multiplier: f32 = match flight_mode {
        FlightMode::Drift => 250.,
        FlightMode::Stealth => 30.,
        FlightMode::Cruise => 25.,
        FlightMode::Burn => 12.5,
    };

    (f32::max(distance as f32, 1.0) * navigation_multiplier / engine_speed as f32 + 15.0).round() as u32
}

pub fn format_time_delta_hh_mm_ss(delta: TimeDelta) -> String {
    let total_seconds = delta.num_seconds();
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
}

// Format any collection whose items implement Display
pub fn format_and_sort_collection<'a, T, I>(collection: I) -> String
where
    T: Display + 'a,
    I: IntoIterator<Item = &'a T>,
{
    collection
        .into_iter()
        .map(|item| item.to_string())
        .sorted()
        .join(", ")
}

fn calc_batches_based_on_volume_constraint(number_items: u32, trade_volume: u32) -> Vec<u32> {
    if number_items == 0 {
        // inventory-entry should never quantity of 0, but you never know.
        return vec![];
    }

    let result = if number_items <= trade_volume {
        vec![number_items]
    } else {
        let mut batches = vec![];
        let num_batches = number_items / trade_volume;
        for i in 0..=num_batches {
            if i < num_batches {
                batches.push(trade_volume);
            } else {
                let rest = number_items - batches.iter().sum::<u32>();
                batches.push(rest);
            }
        }
        batches
    };

    let total = result.iter().sum::<u32>();
    assert_eq!(total, number_items);

    result
}
