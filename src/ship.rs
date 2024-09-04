use crate::st_client::StClient;
use crate::st_model::Ship;
use anyhow::*;
use serde::{Deserialize, Serialize};
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

pub struct ShipOperations {
    ship: MyShip,
    client: Arc<StClient>,
}

impl ShipOperations {
    pub fn new(ship: MyShip, client: Arc<StClient>) -> Self {
        ShipOperations { ship, client }
    }

    pub async fn dock(&mut self) -> Result<()> {
        let response = self.client.dock_ship(self.ship.symbol.clone()).await?;
        self.nav = response.data.nav.clone();
        println!("{:?}", response);
        Ok(())
    }

    pub async fn orbit(&mut self) -> Result<()> {
        let response = self.client.orbit_ship(self.ship.symbol.clone()).await?;
        self.nav = response.data.nav.clone();
        println!("{:?}", response);
        Ok(())
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
