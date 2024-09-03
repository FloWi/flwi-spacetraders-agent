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

use crate::api_client::api_model::FlightMode;

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
