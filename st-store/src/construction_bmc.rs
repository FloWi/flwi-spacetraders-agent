use crate::ctx::Ctx;
use crate::{DbConstructionSiteEntry, DbMarketEntry, DbModelManager, DbShipEntry};
use anyhow::*;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use sqlx::{Pool, Postgres};
use sqlx::types::Json;
use st_domain::{GetConstructionResponse, MarketData, Ship, StStatusResponse, SystemSymbol};

pub struct ConstructionBmc;

impl ConstructionBmc {
    pub async fn get_construction_site_for_system(ctx: &Ctx, mm: &DbModelManager, system_symbol: SystemSymbol) -> Result<Option<GetConstructionResponse>> {

        let waypoint_symbol_pattern = format!("{}%", system_symbol.0);

        let maybe_construction_entry: Option<DbConstructionSiteEntry> = sqlx::query_as!(
            DbConstructionSiteEntry,
            r#"
SELECT waypoint_symbol
     , entry as "entry: Json<GetConstructionResponse>"
     , created_at
     , updated_at
  from construction_sites
 where waypoint_symbol like $1

        "#,
            waypoint_symbol_pattern
        )
            .fetch_optional(mm.pool())

            .await?;

        Ok(maybe_construction_entry.map(|db_entry|db_entry.entry.0))
    }
}
