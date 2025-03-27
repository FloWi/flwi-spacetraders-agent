use crate::ctx::Ctx;
use crate::{DbModelManager, DbShipEntry, DbShipTaskEntry};
use anyhow::*;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use sqlx::types::Json;
use sqlx::{Pool, Postgres};
use st_domain::{Ship, ShipSymbol, ShipTask, StStatusResponse};
use std::collections::HashMap;

pub struct ShipBmc;

impl ShipBmc {
    pub async fn get_ships(ctx: &Ctx, mm: &DbModelManager, timestamp_filter_gte: Option<DateTime<Utc>>) -> Result<Vec<Ship>> {
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
        .fetch_all(mm.pool())
        .await?;

        let ships = ship_entries.into_iter().map(|se| se.entry.0).collect_vec();

        Ok(ships)
    }

    pub async fn get_ship(ctx: &Ctx, mm: &DbModelManager, ship_symbol: ShipSymbol) -> Result<Ship> {
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
        .fetch_one(mm.pool())
        .await?;

        Ok(ship_entry.entry.0)
    }

    pub async fn load_ship_tasks(ctx: &Ctx, mm: &DbModelManager) -> Result<HashMap<ShipSymbol, ShipTask>> {
        let entries: Vec<DbShipTaskEntry> = sqlx::query_as!(
            DbShipTaskEntry,
            r#"
select ship_symbol
     , task as "task: Json<ShipTask>"
  from ship_task_assignments
        "#,
        )
        .fetch_all(mm.pool())
        .await?;

        Ok(entries.into_iter().map(|db_entry| (ShipSymbol(db_entry.ship_symbol), db_entry.task.0)).collect())
    }

    pub async fn save_ship_tasks(ctx: &Ctx, mm: &DbModelManager, ship_task_assignments: &HashMap<ShipSymbol, ShipTask>) -> Result<()> {
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
            .execute(mm.pool())
            .await?;
        }
        Ok(())
    }
}
