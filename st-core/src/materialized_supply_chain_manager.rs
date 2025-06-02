use st_domain::cargo_transfer::HaulerTransferSummary;
use st_domain::{Cargo, MaterializedSupplyChain, ShipSymbol, SystemSymbol, WaypointSymbol};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio::sync::Mutex;

pub struct MaterializedSupplyChainManager {
    // Haulers waiting at each location
    materialized_supply_chain: Arc<Mutex<HashMap<SystemSymbol, MaterializedSupplyChain>>>,
}

impl MaterializedSupplyChainManager {
    pub fn new() -> Self {
        Self {
            materialized_supply_chain: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}
