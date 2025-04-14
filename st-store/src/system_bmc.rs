use crate::ctx::Ctx;
use crate::{db, DbMarketEntry, DbModelManager, DbShipyardData};
use anyhow::*;
use async_trait::async_trait;
use chrono::Utc;
use itertools::Itertools;
use mockall::automock;
use st_domain::{Agent, MarketEntry, ShipyardData, SystemSymbol, Waypoint};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug)]
pub struct DbSystemBmc {
    pub(crate) mm: DbModelManager,
}

#[automock]
#[async_trait]
pub trait SystemBmcTrait: Send + Sync + Debug {
    async fn get_waypoints_of_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<Waypoint>>;
    async fn save_waypoints_of_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol, waypoints: Vec<Waypoint>) -> Result<()>;
    async fn select_latest_marketplace_entry_of_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<MarketEntry>>;
    async fn select_latest_shipyard_entry_of_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<ShipyardData>>;
}

#[async_trait]
impl SystemBmcTrait for DbSystemBmc {
    async fn get_waypoints_of_system(&self, _ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<Waypoint>> {
        db::select_waypoints_of_system(self.mm.pool(), system_symbol).await
    }

    async fn save_waypoints_of_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol, waypoints: Vec<Waypoint>) -> Result<()> {
        db::upsert_waypoints(self.mm.pool(), waypoints, Utc::now()).await
    }

    async fn select_latest_marketplace_entry_of_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<MarketEntry>> {
        db::select_latest_marketplace_entry_of_system(self.mm.pool(), system_symbol).await
    }

    async fn select_latest_shipyard_entry_of_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<ShipyardData>> {
        db::select_latest_shipyard_entry_of_system(self.mm.pool(), system_symbol).await
    }
}

#[derive(Debug)]
pub struct InMemorySystems {
    waypoints_per_system: HashMap<SystemSymbol, Vec<Waypoint>>,
    latest_marketplace_entries: HashMap<SystemSymbol, Vec<MarketEntry>>,
    latest_shipyard_entries: HashMap<SystemSymbol, Vec<ShipyardData>>,
}

#[derive(Debug)]
pub struct InMemorySystemsBmc {
    in_memory_systems: Arc<RwLock<InMemorySystems>>,
}

impl InMemorySystemsBmc {
    pub fn new() -> Self {
        Self {
            in_memory_systems: Arc::new(RwLock::new(InMemorySystems {
                waypoints_per_system: Default::default(),
                latest_marketplace_entries: Default::default(),
                latest_shipyard_entries: Default::default(),
            })),
        }
    }
}

#[async_trait]
impl SystemBmcTrait for InMemorySystemsBmc {
    async fn get_waypoints_of_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<Waypoint>> {
        Ok(self.in_memory_systems.read().await.waypoints_per_system.get(system_symbol).cloned().unwrap_or_default())
    }

    async fn save_waypoints_of_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol, waypoints: Vec<Waypoint>) -> Result<()> {
        self.in_memory_systems.write().await.waypoints_per_system.insert(system_symbol.clone(), waypoints);
        Ok(())
    }

    async fn select_latest_marketplace_entry_of_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<MarketEntry>> {
        Ok(self.in_memory_systems.read().await.latest_marketplace_entries.get(system_symbol).cloned().unwrap_or_default())
    }

    async fn select_latest_shipyard_entry_of_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<ShipyardData>> {
        Ok(self.in_memory_systems.read().await.latest_shipyard_entries.get(system_symbol).cloned().unwrap_or_default())
    }
}
