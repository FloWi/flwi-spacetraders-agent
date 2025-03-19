use crate::ctx::Ctx;
use crate::{DbModelManager, DbShipEntry};
use anyhow::*;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use sqlx::types::Json;
use sqlx::{Pool, Postgres};
use st_domain::{Ship, ShipSymbol, StStatusResponse};

pub struct ShipBmc;

impl ShipBmc {
    pub async fn get_ships(
        ctx: &Ctx,
        mm: &DbModelManager,
        timestamp_filter_gte: Option<DateTime<Utc>>,
    ) -> Result<Vec<Ship>> {
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
}
