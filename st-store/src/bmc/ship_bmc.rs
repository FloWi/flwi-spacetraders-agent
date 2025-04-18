use crate::{db, Ctx, DbModelManager, DbShipEntry, DbShipTaskEntry};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use mockall::automock;
use sqlx::types::Json;
use st_domain::{Data, ExplorationTask, Ship, ShipSymbol, ShipTask, StationaryProbeLocation, WaypointSymbol};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::RwLock;

#[automock]
#[async_trait]
pub trait ShipBmcTrait: Send + Sync + Debug {
    async fn get_ships(&self, ctx: &Ctx, timestamp_filter_gte: Option<DateTime<Utc>>) -> Result<Vec<Ship>>;
    async fn get_ship(&self, ctx: &Ctx, ship_symbol: ShipSymbol) -> Result<Ship>;
    async fn load_ship_tasks(&self, ctx: &Ctx) -> Result<HashMap<ShipSymbol, ShipTask>>;
    async fn save_ship_tasks(&self, ctx: &Ctx, ship_task_assignments: &HashMap<ShipSymbol, ShipTask>) -> Result<()>;
    async fn get_stationary_probes(&self, ctx: &Ctx) -> Result<Vec<StationaryProbeLocation>>;
    async fn insert_stationary_probe(&self, ctx: &Ctx, location: StationaryProbeLocation) -> Result<()>;
    async fn upsert_ships(&self, ctx: &Ctx, ships: &[Ship], now: DateTime<Utc>) -> Result<()>;
}

#[derive(Debug)]
pub struct DbShipBmc {
    pub(crate) mm: DbModelManager,
}

#[async_trait]
impl ShipBmcTrait for DbShipBmc {
    async fn get_ships(&self, ctx: &Ctx, timestamp_filter_gte: Option<DateTime<Utc>>) -> Result<Vec<Ship>> {
        let fallback = DateTime::<Utc>::from_timestamp(0, 0).unwrap();

        let ship_entries: Vec<DbShipEntry> = sqlx::query_as!(
            DbShipEntry,
            r#"
select ship_symbol
     , entry as "entry: Json<Ship>"
     , created_at
     , updated_at
  from ships
 where updated_at >= $1
        "#,
            timestamp_filter_gte.unwrap_or(fallback)
        )
        .fetch_all(self.mm.pool())
        .await?;

        let ships = ship_entries.into_iter().map(|se| se.entry.0).collect_vec();

        anyhow::Ok(ships)
    }

    async fn get_ship(&self, ctx: &Ctx, ship_symbol: ShipSymbol) -> Result<Ship> {
        let ship_entry: DbShipEntry = sqlx::query_as!(
            DbShipEntry,
            r#"
select ship_symbol
     , entry as "entry: Json<Ship>"
     , created_at
     , updated_at
  from ships
 where ships.ship_symbol = $1
        "#,
            ship_symbol.0
        )
        .fetch_one(self.mm.pool())
        .await?;

        anyhow::Ok(ship_entry.entry.0)
    }

    async fn load_ship_tasks(&self, ctx: &Ctx) -> Result<HashMap<ShipSymbol, ShipTask>> {
        let entries: Vec<DbShipTaskEntry> = sqlx::query_as!(
            DbShipTaskEntry,
            r#"
select ship_symbol
     , task as "task: Json<ShipTask>"
  from ship_task_assignments
        "#,
        )
        .fetch_all(self.mm.pool())
        .await?;

        anyhow::Ok(entries.into_iter().map(|db_entry| (ShipSymbol(db_entry.ship_symbol), db_entry.task.0)).collect())
    }

    async fn save_ship_tasks(&self, ctx: &Ctx, ship_task_assignments: &HashMap<ShipSymbol, ShipTask>) -> Result<()> {
        for (ship_symbol, task) in ship_task_assignments {
            sqlx::query!(
                r#"
insert into ship_task_assignments (ship_symbol, task)
values ($1, $2)
on conflict (ship_symbol) do update set task = excluded.task
        "#,
                ship_symbol.0,
                Json(task.clone()) as _
            )
            .execute(self.mm.pool())
            .await?;
        }
        anyhow::Ok(())
    }

    async fn get_stationary_probes(&self, ctx: &Ctx) -> Result<Vec<StationaryProbeLocation>> {
        let entries: Vec<DbStationaryProbeLocation> = sqlx::query_as!(
            DbStationaryProbeLocation,
            r#"
select waypoint_symbol
     , probe_ship_symbol
     , exploration_tasks as "exploration_tasks: Json<Vec<ExplorationTask>>"
  from stationary_probe_locations
        "#,
        )
        .fetch_all(self.mm.pool())
        .await?;

        anyhow::Ok(
            entries
                .into_iter()
                .map(|db_entry| StationaryProbeLocation {
                    waypoint_symbol: WaypointSymbol(db_entry.waypoint_symbol),
                    probe_ship_symbol: ShipSymbol(db_entry.probe_ship_symbol),
                    exploration_tasks: db_entry.exploration_tasks.0,
                })
                .collect_vec(),
        )
    }

    async fn insert_stationary_probe(&self, ctx: &Ctx, location: StationaryProbeLocation) -> Result<()> {
        sqlx::query!(
            r#"
insert into stationary_probe_locations ( waypoint_symbol, probe_ship_symbol, exploration_tasks )
values ($1, $2, $3)
on conflict (waypoint_symbol) do update
    set probe_ship_symbol = excluded.probe_ship_symbol
      , exploration_tasks = excluded.exploration_tasks

        "#,
            location.waypoint_symbol.0,
            location.probe_ship_symbol.0,
            Json(location.exploration_tasks.clone()) as _
        )
        .execute(self.mm.pool())
        .await?;

        anyhow::Ok(())
    }

    async fn upsert_ships(&self, ctx: &Ctx, ships: &[Ship], now: DateTime<Utc>) -> Result<()> {
        db::upsert_ships(self.mm.pool(), ships, now).await?;

        Ok(())
    }
}

pub struct DbStationaryProbeLocation {
    pub waypoint_symbol: String,
    pub probe_ship_symbol: String,
    pub exploration_tasks: Json<Vec<ExplorationTask>>,
}

#[derive(Debug)]
pub struct InMemoryShips {
    ships: HashMap<ShipSymbol, Ship>,
    ship_tasks: HashMap<ShipSymbol, ShipTask>,
    stationary_probe_locations: HashMap<WaypointSymbol, StationaryProbeLocation>,
}

impl InMemoryShips {
    pub fn new() -> Self {
        Self {
            ships: Default::default(),
            ship_tasks: Default::default(),
            stationary_probe_locations: Default::default(),
        }
    }
}

/// Client implementation using InMemoryUniverse with interior mutability
#[derive(Debug)]
pub struct InMemoryShipsBmc {
    in_memory_ships: Arc<RwLock<InMemoryShips>>,
}

impl InMemoryShipsBmc {
    pub fn new(in_memory_ships: InMemoryShips) -> Self {
        Self {
            in_memory_ships: Arc::new(RwLock::new(in_memory_ships)),
        }
    }
}

#[async_trait]
impl ShipBmcTrait for InMemoryShipsBmc {
    async fn get_ships(&self, ctx: &Ctx, timestamp_filter_gte: Option<DateTime<Utc>>) -> Result<Vec<Ship>> {
        Ok(self.in_memory_ships.read().await.ships.values().cloned().collect_vec())
    }

    async fn get_ship(&self, ctx: &Ctx, ship_symbol: ShipSymbol) -> Result<Ship> {
        let read_data = self.in_memory_ships.read().await;

        read_data.ships.get(&ship_symbol).cloned().ok_or(anyhow!("Ship not found"))
    }

    async fn load_ship_tasks(&self, ctx: &Ctx) -> Result<HashMap<ShipSymbol, ShipTask>> {
        Ok(self.in_memory_ships.read().await.ship_tasks.clone())
    }

    async fn save_ship_tasks(&self, ctx: &Ctx, ship_task_assignments: &HashMap<ShipSymbol, ShipTask>) -> Result<()> {
        self.in_memory_ships.write().await.ship_tasks = ship_task_assignments.clone();

        Ok(())
    }

    async fn get_stationary_probes(&self, ctx: &Ctx) -> Result<Vec<StationaryProbeLocation>> {
        Ok(self.in_memory_ships.read().await.stationary_probe_locations.values().cloned().collect_vec())
    }

    async fn insert_stationary_probe(&self, ctx: &Ctx, location: StationaryProbeLocation) -> Result<()> {
        self.in_memory_ships.write().await.stationary_probe_locations.insert(location.waypoint_symbol.clone(), location);
        Ok(())
    }

    async fn upsert_ships(&self, ctx: &Ctx, ships: &[Ship], now: DateTime<Utc>) -> Result<()> {
        let mut guard = self.in_memory_ships.write().await;
        for ship in ships {
            guard.ships.insert(ship.symbol.clone(), ship.clone());
        }

        Ok(())
    }
}
