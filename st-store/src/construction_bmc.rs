use crate::ctx::Ctx;
use crate::{db, DbConstructionSiteEntry, DbModelManager};
use anyhow::*;
use async_trait::async_trait;
use chrono::Utc;
use mockall::automock;
use sqlx::types::Json;
use st_domain::{Construction, SystemSymbol};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug)]
pub struct DbConstructionBmc {
    pub(crate) mm: DbModelManager,
}

#[automock]
#[async_trait]
pub trait ConstructionBmcTrait: Send + Sync + Debug {
    async fn get_construction_site_for_system(&self, ctx: &Ctx, system_symbol: SystemSymbol) -> Result<Option<Construction>>;
    async fn save_construction_site(&self, ctx: &Ctx, construction_site: Construction) -> Result<()>;
}

#[async_trait]
impl ConstructionBmcTrait for DbConstructionBmc {
    async fn get_construction_site_for_system(&self, _ctx: &Ctx, system_symbol: SystemSymbol) -> Result<Option<Construction>> {
        let waypoint_symbol_pattern = format!("{}%", system_symbol.0);

        let maybe_construction_entry: Option<DbConstructionSiteEntry> = sqlx::query_as!(
            DbConstructionSiteEntry,
            r#"
SELECT waypoint_symbol
     , entry as "entry: Json<Construction>"
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

    async fn save_construction_site(&self, _ctx: &Ctx, construction_site: Construction) -> Result<()> {
        db::upsert_construction_site(self.mm.pool(), construction_site, Utc::now()).await
    }
}

#[derive(Debug)]
pub struct InMemoryConstruction {
    construction_sites: HashMap<SystemSymbol, Construction>,
}

impl Default for InMemoryConstruction {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryConstruction {
    pub fn new() -> Self {
        Self {
            construction_sites: Default::default(),
        }
    }
}

#[derive(Debug)]
pub struct InMemoryConstructionBmc {
    in_memory_construction: Arc<RwLock<InMemoryConstruction>>,
}

impl Default for InMemoryConstructionBmc {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryConstructionBmc {
    pub fn new() -> Self {
        Self {
            in_memory_construction: Arc::new(RwLock::new(InMemoryConstruction::new())),
        }
    }
}

#[async_trait]
impl ConstructionBmcTrait for InMemoryConstructionBmc {
    async fn get_construction_site_for_system(&self, _ctx: &Ctx, system_symbol: SystemSymbol) -> Result<Option<Construction>> {
        Ok(self
            .in_memory_construction
            .read()
            .await
            .construction_sites
            .get(&system_symbol)
            .cloned())
    }

    async fn save_construction_site(&self, _ctx: &Ctx, construction_site: Construction) -> Result<()> {
        self.in_memory_construction
            .write()
            .await
            .construction_sites
            .insert(construction_site.symbol.system_symbol(), construction_site.clone());
        Ok(())
    }
}
