pub mod api_client;
pub mod cli_args;
pub mod configuration;
pub mod db;
pub mod pagination;
pub mod reqwest_helpers;
pub mod ship;
pub mod st_client;
pub mod st_model;
pub mod supply_chain;
extern crate serde;

use chrono::TimeDelta;
use st_model::FlightMode;

mod behavior_tree;
pub mod exploration;
pub mod marketplaces;
pub mod pathfinder;

impl FlightMode {
    pub fn calculate_fuel_consumption(&self, distance: u32) -> u32 {
        match self {
            FlightMode::Drift => 1,
            FlightMode::Cruise => u32::max(1, distance),
            FlightMode::Stealth => u32::max(1, distance),
            FlightMode::Burn => 2 * u32::max(1, distance),
        }
    }

    pub fn calculate_time(&self, distance: u32, engine_speed: u32) -> u32 {
        let navigation_multiplier: f32 = match self {
            FlightMode::Drift => 250.,
            FlightMode::Stealth => 30.,
            FlightMode::Cruise => 25.,
            FlightMode::Burn => 12.5,
        };

        (f32::max(distance as f32, 1.0) * navigation_multiplier / engine_speed as f32 + 15.0)
            .round() as u32
    }
}

pub fn format_time_delta_hh_mm_ss(delta: TimeDelta) -> String {
    let total_seconds = delta.num_seconds();
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
}
