use crate::ctx::Ctx;
use crate::{db, DbModelManager};
use anyhow::*;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use mockall::automock;
use st_domain::{ShipPriceInfo, Shipyard, ShipyardData, SupplyChain, SystemSymbol, WaypointSymbol};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug)]
pub struct DbSupplyChainBmc {
    pub(crate) mm: DbModelManager,
}

#[async_trait]
impl SupplyChainBmcTrait for DbSupplyChainBmc {
    async fn get_supply_chain(&self, _ctx: &Ctx) -> Result<Option<SupplyChain>> {
        db::load_supply_chain(self.mm.pool()).await
    }

    async fn insert_supply_chain(&self, ctx: &Ctx, supply_chain: SupplyChain, now: DateTime<Utc>) -> Result<()> {
        db::insert_supply_chain(self.mm.pool(), supply_chain, now).await
    }
}

#[automock]
#[async_trait]
pub trait SupplyChainBmcTrait: Send + Sync + Debug {
    async fn get_supply_chain(&self, ctx: &Ctx) -> Result<Option<SupplyChain>>;
    async fn insert_supply_chain(&self, ctx: &Ctx, supply_chain: SupplyChain, now: DateTime<Utc>) -> Result<()>;
}

#[derive(Debug)]
pub struct InMemorySupplyChains {
    supply_chain: Option<SupplyChain>,
}

#[async_trait]
impl SupplyChainBmcTrait for InMemorySupplyChainBmc {
    async fn get_supply_chain(&self, _ctx: &Ctx) -> Result<Option<SupplyChain>> {
        Ok(self.in_memory_supply_chain.read().await.supply_chain.clone())
    }

    async fn insert_supply_chain(&self, ctx: &Ctx, supply_chain: SupplyChain, now: DateTime<Utc>) -> Result<()> {
        self.in_memory_supply_chain.write().await.supply_chain = Some(supply_chain.clone());
        Ok(())
    }
}

impl InMemorySupplyChains {
    fn new() -> Self {
        Self { supply_chain: None }
    }
}

#[derive(Debug)]
pub struct InMemorySupplyChainBmc {
    in_memory_supply_chain: Arc<RwLock<InMemorySupplyChains>>,
}

impl Default for InMemorySupplyChainBmc {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemorySupplyChainBmc {
    pub fn new() -> Self {
        Self {
            in_memory_supply_chain: Arc::new(RwLock::new(InMemorySupplyChains::new())),
        }
    }
}
