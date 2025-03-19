use crate::pathfinder::pathfinder::TravelAction;
use crate::st_client::StClientTrait;
use anyhow::*;
use itertools::Itertools;
use st_domain::{
    CreateChartBody, FlightMode, Fuel, JumpGate, MarketData, Nav, NavAndFuelResponse,
    RefuelShipResponse, Ship, Shipyard, Waypoint, WaypointSymbol,
};
use std::collections::VecDeque;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct ShipOperations {
    pub ship: Ship,
    client: Arc<dyn StClientTrait>,
    pub travel_action_queue: VecDeque<TravelAction>,
    pub current_navigation_destination: Option<WaypointSymbol>,
    pub explore_location_queue: VecDeque<WaypointSymbol>,
}

impl PartialEq for ShipOperations {
    fn eq(&self, other: &Self) -> bool {
        self.ship.eq(&other.ship)
    }
}

impl ShipOperations {
    pub(crate) fn current_travel_action(&self) -> Option<&TravelAction> {
        self.travel_action_queue.front()
    }

    pub fn last_travel_action(&self) -> Option<&TravelAction> {
        self.travel_action_queue.back()
    }

    pub(crate) fn set_nav(&mut self, new_nav: Nav) {
        self.nav = new_nav;
    }

    pub(crate) fn set_fuel(&mut self, new_fuel: Fuel) {
        self.fuel = new_fuel;
    }

    pub fn set_route(&mut self, new_route: Vec<TravelAction>) {
        self.travel_action_queue = VecDeque::from(new_route);
    }

    pub fn new(ship: Ship, client: Arc<dyn StClientTrait>) -> Self {
        ShipOperations {
            ship,
            client,
            travel_action_queue: VecDeque::new(),
            current_navigation_destination: None,
            explore_location_queue: VecDeque::new(),
        }
    }

    pub fn pop_travel_action(&mut self) {
        let _ = self.travel_action_queue.pop_front();
    }

    pub fn set_destination(&mut self, destination: WaypointSymbol) {
        self.current_navigation_destination = Some(destination)
    }

    pub fn pop_explore_location_as_destination(&mut self) {
        self.current_navigation_destination = self.explore_location_queue.pop_front();
    }

    pub fn set_explore_locations(&mut self, waypoint_symbols: Vec<WaypointSymbol>) {
        let deque = VecDeque::from(waypoint_symbols);
        self.explore_location_queue = deque;
    }

    pub async fn dock(&mut self) -> Result<Nav> {
        let response = self.client.dock_ship(self.ship.symbol.clone()).await?;
        println!("{:?}", response);
        Ok(response.data.nav)
    }

    pub(crate) async fn get_market(&self) -> Result<MarketData> {
        let response = self
            .client
            .get_marketplace(self.nav.waypoint_symbol.clone())
            .await?;
        println!("{:?}", response);
        Ok(response.data)
    }

    pub(crate) async fn get_jump_gate(&self) -> Result<JumpGate> {
        let response = self
            .client
            .get_jump_gate(self.nav.waypoint_symbol.clone())
            .await?;
        println!("{:?}", response);
        Ok(response.data)
    }

    pub(crate) async fn get_shipyard(&self) -> Result<Shipyard> {
        let response = self
            .client
            .get_shipyard(self.nav.waypoint_symbol.clone())
            .await?;
        println!("{:?}", response);
        Ok(response.data)
    }

    pub(crate) async fn chart_waypoint(&self) -> Result<CreateChartBody> {
        let response = self.client.create_chart(self.symbol.clone()).await?;
        println!("{:?}", response);
        Ok(response.data)
    }

    pub(crate) async fn set_flight_mode(&self, mode: &FlightMode) -> Result<Nav> {
        let response = self
            .client
            .set_flight_mode(self.ship.symbol.clone(), mode)
            .await?;
        println!("{:?}", response);
        Ok(response.data)
    }

    pub async fn orbit(&mut self) -> Result<Nav> {
        let response = self.client.orbit_ship(self.ship.symbol.clone()).await?;
        println!("{:?}", response);
        Ok(response.data.nav)
    }

    pub async fn navigate(&self, to: &WaypointSymbol) -> Result<NavAndFuelResponse> {
        let response = self.client.navigate(self.ship.symbol.clone(), to).await?;
        println!("{:?}", response);
        Ok(response.data)
    }

    pub(crate) async fn refuel(&self, from_cargo: bool) -> Result<RefuelShipResponse> {
        let amount = self.fuel.capacity - self.fuel.current;

        let response = self
            .client
            .refuel(self.ship.symbol.clone(), amount as u32, from_cargo)
            .await?;
        println!("{:?}", response);
        Ok(response)
    }

    // Other methods that require API access...

    pub fn get_ship(&self) -> &Ship {
        &self.ship
    }

    pub fn get_ship_mut(&mut self) -> &mut Ship {
        &mut self.ship
    }
}

impl Deref for ShipOperations {
    type Target = Ship;

    fn deref(&self) -> &Self::Target {
        &self.ship
    }
}

// If you need mutable access, you can also implement DerefMut
impl DerefMut for ShipOperations {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ship
    }
}
