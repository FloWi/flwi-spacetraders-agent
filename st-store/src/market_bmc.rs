use crate::ctx::Ctx;
use crate::{db, DbMarketEntry, DbModelManager};
use anyhow::*;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use mockall::automock;
use sqlx::types::Json;
use st_domain::{MarketData, MarketEntry, SystemSymbol, WaypointSymbol};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug)]
pub struct DbMarketBmc {
    pub(crate) mm: DbModelManager,
}

#[automock]
#[async_trait]
pub trait MarketBmcTrait: Send + Sync + Debug {
    async fn get_latest_market_data_for_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<MarketEntry>>;
    async fn save_market_data(&self, ctx: &Ctx, market_entries: Vec<MarketData>, now: DateTime<Utc>) -> Result<()>;
}

#[async_trait]
impl MarketBmcTrait for DbMarketBmc {
    async fn get_latest_market_data_for_system(&self, _ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<MarketEntry>> {
        let waypoint_symbol_pattern = format!("{}%", system_symbol.0);

        let market_entries: Vec<DbMarketEntry> = sqlx::query_as!(
            DbMarketEntry,
            r#"
SELECT DISTINCT ON (waypoint_symbol)
       waypoint_symbol
     , entry as "entry: Json<MarketData>"
     , created_at
  from markets
 where waypoint_symbol like $1
ORDER BY waypoint_symbol, created_at DESC

        "#,
            waypoint_symbol_pattern
        )
        .fetch_all(self.mm.pool())
        .await?;

        let result = market_entries
            .into_iter()
            .map(|db_entry| MarketEntry {
                waypoint_symbol: WaypointSymbol(db_entry.waypoint_symbol.clone()),
                market_data: db_entry.entry.0,
                created_at: db_entry.created_at,
            })
            .collect_vec();

        Ok(result)
    }

    async fn save_market_data(&self, _ctx: &Ctx, market_entries: Vec<MarketData>, now: DateTime<Utc>) -> Result<()> {
        db::insert_market_data(self.mm.pool(), market_entries, now).await
    }
}

#[derive(Debug)]
pub struct InMemoryMarket {
    latest_market_data: HashMap<SystemSymbol, HashMap<WaypointSymbol, MarketEntry>>,
}

#[derive(Debug)]
pub struct InMemoryMarketBmc {
    in_memory_market: Arc<RwLock<InMemoryMarket>>,
}

#[async_trait]
impl MarketBmcTrait for InMemoryMarketBmc {
    async fn get_latest_market_data_for_system(&self, _ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<MarketEntry>> {
        Ok(self
            .in_memory_market
            .read()
            .await
            .latest_market_data
            .get(system_symbol)
            .cloned()
            .unwrap_or_default()
            .values()
            .cloned()
            .collect_vec())
    }

    async fn save_market_data(&self, _ctx: &Ctx, market_entries: Vec<MarketData>, now: DateTime<Utc>) -> Result<()> {
        let mut guard = self.in_memory_market.write().await;

        for me in market_entries {
            guard
                .latest_market_data
                .entry(me.symbol.system_symbol())
                .or_default()
                .insert(
                    me.symbol.clone(),
                    MarketEntry {
                        waypoint_symbol: me.symbol.clone(),
                        market_data: me.clone(),
                        created_at: now,
                    },
                );
        }
        Ok(())
    }
}

impl Default for InMemoryMarketBmc {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryMarketBmc {
    pub fn new() -> Self {
        Self {
            in_memory_market: Arc::new(RwLock::new(InMemoryMarket {
                latest_market_data: Default::default(),
            })),
        }
    }
}
