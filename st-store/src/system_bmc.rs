use crate::ctx::Ctx;
use crate::{db, DbModelManager, DbWaypointEntry};
use anyhow::*;
use async_trait::async_trait;
use chrono::Utc;
use itertools::Itertools;
use mockall::automock;
use sqlx::types::Json;
use st_domain::{SystemSymbol, Waypoint, WaypointSymbol};
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
    async fn get_waypoint(&self, ctx: &Ctx, waypoint_symbol: &WaypointSymbol) -> Result<Waypoint>;
    async fn save_waypoints_of_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol, waypoints: Vec<Waypoint>) -> Result<()>;
    async fn upsert_waypoint(&self, ctx: &Ctx, waypoint: Waypoint) -> Result<()>;

    async fn get_num_systems(&self, ctx: &Ctx) -> Result<i64>;
    async fn get_num_waypoints(&self, ctx: &Ctx) -> Result<i64>;
}

#[async_trait]
impl SystemBmcTrait for DbSystemBmc {
    async fn get_waypoints_of_system(&self, _ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<Waypoint>> {
        db::select_waypoints_of_system(self.mm.pool(), system_symbol).await
    }

    async fn save_waypoints_of_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol, waypoints: Vec<Waypoint>) -> Result<()> {
        db::upsert_waypoints(self.mm.pool(), waypoints, Utc::now()).await
    }

    async fn upsert_waypoint(&self, ctx: &Ctx, waypoint: Waypoint) -> Result<()> {
        db::upsert_waypoints(self.mm.pool(), vec![waypoint], Utc::now()).await
    }

    async fn get_num_systems(&self, ctx: &Ctx) -> Result<i64> {
        let row = sqlx::query!(
            r#"
select count(*) as count
  from systems
        "#,
        )
        .fetch_one(self.mm.pool())
        .await?;

        row.count
            .ok_or_else(|| anyhow::anyhow!("COUNT(*) returned NULL"))
    }

    async fn get_num_waypoints(&self, ctx: &Ctx) -> Result<i64> {
        let row = sqlx::query!(
            r#"
select count(*) as count
  from waypoints
        "#,
        )
        .fetch_one(self.mm.pool())
        .await?;

        row.count
            .ok_or_else(|| anyhow::anyhow!("COUNT(*) returned NULL"))
    }

    async fn get_waypoint(&self, ctx: &Ctx, waypoint_symbol: &WaypointSymbol) -> Result<Waypoint> {
        let maybe_waypoint_entry: Option<DbWaypointEntry> = sqlx::query_as!(
            DbWaypointEntry,
            r#"
select system_symbol
     , waypoint_symbol
     , entry as "entry: Json<Waypoint>"
     , created_at
     , updated_at
from waypoints
where waypoints.waypoint_symbol = $1
    "#,
            waypoint_symbol.0.clone()
        )
        .fetch_optional(self.mm.pool())
        .await?;

        maybe_waypoint_entry
            .map(|wp_entry| wp_entry.entry.0.clone())
            .ok_or(anyhow!("Waypoint {} not found", waypoint_symbol.0.clone()))
    }
}

#[derive(Debug)]
pub struct InMemorySystems {
    waypoints_per_system: HashMap<SystemSymbol, HashMap<WaypointSymbol, Waypoint>>,
}

#[derive(Debug)]
pub struct InMemorySystemsBmc {
    in_memory_systems: Arc<RwLock<InMemorySystems>>,
}

impl Default for InMemorySystemsBmc {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemorySystemsBmc {
    pub fn new() -> Self {
        Self {
            in_memory_systems: Arc::new(RwLock::new(InMemorySystems {
                waypoints_per_system: Default::default(),
            })),
        }
    }
}

#[async_trait]
impl SystemBmcTrait for InMemorySystemsBmc {
    async fn get_waypoints_of_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<Waypoint>> {
        Ok(self
            .in_memory_systems
            .read()
            .await
            .waypoints_per_system
            .get(system_symbol)
            .cloned()
            .unwrap_or_default()
            .values()
            .cloned()
            .collect_vec())
    }

    async fn get_waypoint(&self, ctx: &Ctx, waypoint_symbol: &WaypointSymbol) -> Result<Waypoint> {
        let waypoints = self
            .in_memory_systems
            .read()
            .await
            .waypoints_per_system
            .get(&waypoint_symbol.system_symbol())
            .cloned()
            .unwrap_or_default();
        waypoints
            .get(waypoint_symbol)
            .cloned()
            .ok_or(anyhow!("Waypoint {} not found", waypoint_symbol))
    }

    async fn save_waypoints_of_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol, waypoints: Vec<Waypoint>) -> Result<()> {
        let waypoint_map = waypoints
            .into_iter()
            .map(|wp| (wp.symbol.clone(), wp))
            .collect();
        self.in_memory_systems
            .write()
            .await
            .waypoints_per_system
            .insert(system_symbol.clone(), waypoint_map);
        Ok(())
    }

    async fn upsert_waypoint(&self, ctx: &Ctx, waypoint: Waypoint) -> Result<()> {
        let mut guard = self.in_memory_systems.write().await;

        guard
            .waypoints_per_system
            .entry(waypoint.system_symbol.clone())
            .or_default()
            .insert(waypoint.symbol.clone(), waypoint.clone());
        Ok(())
    }

    async fn get_num_systems(&self, ctx: &Ctx) -> Result<i64> {
        Ok(self
            .in_memory_systems
            .read()
            .await
            .waypoints_per_system
            .len() as i64)
    }

    async fn get_num_waypoints(&self, ctx: &Ctx) -> Result<i64> {
        let result = self
            .in_memory_systems
            .read()
            .await
            .waypoints_per_system
            .values()
            .map(|waypoints| waypoints.len() as i64)
            .sum();
        Ok(result)
    }
}
