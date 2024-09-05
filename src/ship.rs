use crate::pathfinder::pathfinder::TravelAction;
use crate::st_client::StClient;
use crate::st_model::{Nav, Ship};
use anyhow::*;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MyShip {
    #[serde(flatten)]
    pub ship: Ship,
}

impl MyShip {
    pub fn new(api_ship: Ship) -> Self {
        MyShip { ship: api_ship }
    }
}
#[derive(Clone, Debug)]
pub struct ShipOperations {
    pub ship: MyShip,
    client: Arc<StClient>,
    pub route: VecDeque<TravelAction>,
    pub current_action: Option<TravelAction>,
}

impl ShipOperations {
    pub(crate) fn set_nav(&mut self, new_nav: Nav) {
        self.nav = new_nav;
    }
}

impl ShipOperations {
    pub fn set_route(&mut self, new_route: Vec<TravelAction>) {
        self.route = VecDeque::from(new_route);
    }
}

impl ShipOperations {
    pub fn new(ship: MyShip, client: Arc<StClient>) -> Self {
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

    pub async fn dock(&mut self) -> Result<()> {
        let response = self.client.dock_ship(self.ship.symbol.clone()).await?;
        self.nav = response.data.nav.clone();
        println!("{:?}", response);
        Ok(())
    }

    pub async fn orbit(&mut self) -> Result<Nav> {
        let response = self.client.orbit_ship(self.ship.symbol.clone()).await?;
        println!("{:?}", response);
        Ok(response.data.nav)
    }

    pub fn navigate(&mut self, destination: &str) -> Result<()> {
        Ok(())
    }

    // Other methods that require API access...

    pub fn get_ship(&self) -> &MyShip {
        &self.ship
    }

    pub fn get_ship_mut(&mut self) -> &mut MyShip {
        &mut self.ship
    }
}

impl Deref for ShipOperations {
    type Target = MyShip;

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

impl Deref for MyShip {
    type Target = Ship;

    fn deref(&self) -> &Self::Target {
        &self.ship
    }
}

// If you need mutable access, you can also implement DerefMut
impl DerefMut for MyShip {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ship
    }
}
