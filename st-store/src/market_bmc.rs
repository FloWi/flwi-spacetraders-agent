use crate::ctx::Ctx;
use crate::{DbMarketEntry, DbModelManager, DbShipEntry};
use anyhow::*;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use sqlx::types::Json;
use sqlx::{Pool, Postgres};
use st_domain::{MarketData, Ship, StStatusResponse, SystemSymbol};

pub struct MarketBmc;

impl MarketBmc {
    pub async fn get_latest_market_data_for_system(ctx: &Ctx, mm: &DbModelManager, system_symbol: &SystemSymbol) -> Result<Vec<MarketData>> {
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
        .fetch_all(mm.pool())
        .await?;

        let market_data = market_entriy.into_iter().map(|me| me.entry.0).collect_vec();

        Ok(market_data)
    }
}
