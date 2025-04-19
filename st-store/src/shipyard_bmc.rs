use crate::ctx::Ctx;
use crate::{db, DbModelManager};
use anyhow::*;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use mockall::automock;
use st_domain::{ShipPriceInfo, Shipyard, ShipyardData, SystemSymbol, WaypointSymbol};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug)]
pub struct DbShipyardBmc {
    pub(crate) mm: DbModelManager,
}

#[automock]
#[async_trait]
pub trait ShipyardBmcTrait: Send + Sync + Debug {
    async fn get_latest_ship_prices(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<ShipPriceInfo>;
    async fn get_latest_shipyard_entries_of_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<ShipyardData>>;
    async fn save_shipyard_data(&self, ctx: &Ctx, shipyard: Shipyard, now: DateTime<Utc>) -> Result<()>;
}

#[async_trait]
impl ShipyardBmcTrait for DbShipyardBmc {
    async fn get_latest_ship_prices(&self, _ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<ShipPriceInfo> {
        let shipyards = db::select_latest_shipyard_entry_of_system(self.mm.pool(), system_symbol).await?;
        let ship_price_info = extract_ship_price_infos_from_shipyards(&shipyards);
        Ok(ship_price_info)
    }

    async fn get_latest_shipyard_entries_of_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<ShipyardData>> {
        let shipyards = db::select_latest_shipyard_entry_of_system(self.mm.pool(), system_symbol).await?;
        Ok(shipyards)
    }

    async fn save_shipyard_data(&self, ctx: &Ctx, shipyard: Shipyard, now: DateTime<Utc>) -> Result<()> {
        db::insert_shipyards(self.mm.pool(), vec![shipyard], now).await
    }
}

#[derive(Debug)]
pub struct InMemoryShipyards {
    shipyards: HashMap<SystemSymbol, HashMap<WaypointSymbol, ShipyardData>>,
}

impl InMemoryShipyards {
    fn new() -> Self {
        Self { shipyards: Default::default() }
    }
}

#[derive(Debug)]
pub struct InMemoryShipyardBmc {
    in_memory_shipyards: Arc<RwLock<InMemoryShipyards>>,
}

impl Default for InMemoryShipyardBmc {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryShipyardBmc {
    pub fn new() -> Self {
        Self {
            in_memory_shipyards: Arc::new(RwLock::new(InMemoryShipyards::new())),
        }
    }
}

#[async_trait]
impl ShipyardBmcTrait for InMemoryShipyardBmc {
    async fn get_latest_ship_prices(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<ShipPriceInfo> {
        let shipyards = self.in_memory_shipyards.read().await.shipyards.get(system_symbol).cloned().unwrap_or_default().values().cloned().collect_vec();
        let ship_price_info = extract_ship_price_infos_from_shipyards(&shipyards);
        Ok(ship_price_info)
    }

    async fn get_latest_shipyard_entries_of_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<ShipyardData>> {
        let shipyards = self.in_memory_shipyards.read().await.shipyards.get(system_symbol).cloned().unwrap_or_default().values().cloned().collect_vec();
        Ok(shipyards)
    }

    async fn save_shipyard_data(&self, ctx: &Ctx, shipyard: Shipyard, now: DateTime<Utc>) -> Result<()> {
        let mut guard = self.in_memory_shipyards.write().await;
        guard.shipyards.entry(shipyard.symbol.system_symbol()).or_default().insert(
            shipyard.symbol.clone(),
            ShipyardData {
                waypoint_symbol: shipyard.symbol.clone(),
                shipyard,
                created_at: now,
            },
        );
        Ok(())
    }
}

fn extract_ship_price_infos_from_shipyards(result: &[ShipyardData]) -> ShipPriceInfo {
    let price_infos = result
        .iter()
        .filter(|&entry| entry.shipyard.has_detailed_price_information()).map(|entry| (entry.waypoint_symbol.clone(), entry.shipyard.ships.clone().unwrap_or_default()))
        .collect_vec();
    
    ShipPriceInfo { price_infos }
}
