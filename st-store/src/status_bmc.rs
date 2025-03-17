use crate::ctx::Ctx;
use crate::DbModelManager;
use anyhow::*;
use sqlx::{Pool, Postgres};
use st_domain::StStatusResponse;

pub struct StatusBmc;

/*
use crate::ctx::Ctx;
use crate::DbModelManager;
use anyhow::Result;
use st_domain::Fleet;

pub struct FleetBmc;
 */

impl StatusBmc {
    pub async fn get_num_waypoints(ctx: &Ctx, mm: &DbModelManager) -> Result<i64> {
        let row = sqlx::query!(
            r#"
select count(*) as count
  from waypoints
        "#,
        )
        .fetch_one(mm.pool())
        .await?;

        row.count
            .ok_or_else(|| anyhow::anyhow!("COUNT(*) returned NULL"))
    }

    pub async fn get_status(ctx: &Ctx, mm: &DbModelManager) -> Result<Option<StStatusResponse>> {
        Ok(crate::db::load_status(mm.pool())
            .await?
            .map(|db_status| db_status.entry.0))
    }

    pub async fn get_num_systems(ctx: &Ctx, mm: &DbModelManager) -> Result<i64> {
        let row = sqlx::query!(
            r#"
select count(*) as count
  from systems
        "#,
        )
        .fetch_one(mm.pool())
        .await?;

        row.count
            .ok_or_else(|| anyhow::anyhow!("COUNT(*) returned NULL"))
    }
}
