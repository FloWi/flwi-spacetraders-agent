use crate::ctx::Ctx;
use crate::{DbModelManager, DbShipEntry};
use anyhow::*;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use sqlx::{Pool, Postgres};
use sqlx::types::Json;
use st_domain::{Ship, StStatusResponse};

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
}
