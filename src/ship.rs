use crate::pathfinder::pathfinder::TravelAction;
use crate::st_client::StClient;
use crate::st_model::{FlightMode, Nav, Ship, WaypointSymbol};
use anyhow::*;
use std::collections::VecDeque;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ShipOperations {
    pub ship: Ship,
    client: Arc<StClient>,
    pub route: VecDeque<TravelAction>,
    pub current_action: Option<TravelAction>,
}

impl ShipOperations {
    pub(crate) fn set_nav(&mut self, new_nav: Nav) {
        self.nav = new_nav;
    }

    pub fn set_route(&mut self, new_route: Vec<TravelAction>) {
        self.route = VecDeque::from(new_route);
    }

    pub fn new(ship: Ship, client: Arc<StClient>) -> Self {
        ShipOperations {
            ship,
            client,
            route: VecDeque::new(),
            current_action: None,
        }
    }

    pub fn pop_travel_action(&mut self) {
        self.current_action = self.route.pop_front();
    }

    pub async fn dock(&mut self) -> Result<Nav> {
        let response = self.client.dock_ship(self.ship.symbol.clone()).await?;
        println!("{:?}", response);
        Ok(response.data.nav)
    }
    pub(crate) async fn set_flight_mode(&self, mode: &FlightMode) -> Result<Nav> {
        let response = self
            .client
            .set_flight_mode(self.ship.symbol.clone(), mode)
            .await?;
        println!("{:?}", response);
        Ok(response.data.nav)
    }

    pub async fn orbit(&mut self) -> Result<Nav> {
        let response = self.client.orbit_ship(self.ship.symbol.clone()).await?;
        println!("{:?}", response);
        Ok(response.data.nav)
    }

    pub async fn navigate(&self, to: &WaypointSymbol) -> Result<Nav> {
        let response = self.client.navigate(self.ship.symbol.clone(), to).await?;
        println!("{:?}", response);
        Ok(response.data.nav)
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
