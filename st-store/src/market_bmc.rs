use crate::ctx::Ctx;
use crate::{DbMarketEntry, DbModelManager};
use anyhow::*;
use async_trait::async_trait;
use itertools::Itertools;
use mockall::automock;
use sqlx::types::Json;
use st_domain::{MarketData, SystemSymbol};
use std::fmt::Debug;

#[derive(Debug)]
pub struct DbMarketBmc {
    pub(crate) mm: DbModelManager,
}

#[automock]
#[async_trait]
pub trait MarketBmcTrait: Send + Sync + Debug {
    async fn get_latest_market_data_for_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<MarketData>>;
}

#[async_trait]
impl MarketBmcTrait for DbMarketBmc {
    async fn get_latest_market_data_for_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<MarketData>> {
        let waypoint_symbol_pattern = format!("{}%", system_symbol.0);

        let market_entriy: Vec<DbMarketEntry> = sqlx::query_as!(
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

        let market_data = market_entriy.into_iter().map(|me| me.entry.0).collect_vec();

        Ok(market_data)
    }
}
