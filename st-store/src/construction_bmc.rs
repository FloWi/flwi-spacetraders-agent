use crate::ctx::Ctx;
use crate::{DbConstructionSiteEntry, DbModelManager};
use anyhow::*;
use async_trait::async_trait;
use itertools::Itertools;
use mockall::automock;
use sqlx::types::Json;
use st_domain::{GetConstructionResponse, SystemSymbol};
use std::fmt::Debug;

#[derive(Debug)]
pub struct DbConstructionBmc {
    pub(crate) mm: DbModelManager,
}

#[automock]
#[async_trait]
pub trait ConstructionBmcTrait: Send + Sync + Debug {
    async fn get_construction_site_for_system(&self, ctx: &Ctx, system_symbol: SystemSymbol) -> Result<Option<GetConstructionResponse>>;
}

#[async_trait]
impl ConstructionBmcTrait for DbConstructionBmc {
    async fn get_construction_site_for_system(&self, ctx: &Ctx, system_symbol: SystemSymbol) -> Result<Option<GetConstructionResponse>> {
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
        .fetch_optional(self.mm.pool())
        .await?;

        Ok(maybe_construction_entry.map(|db_entry| db_entry.entry.0))
    }
}
