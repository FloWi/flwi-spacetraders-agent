use crate::{Ctx, DbModelManager};
use anyhow::*;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use sqlx::types::Json;
use sqlx::{Pool, Postgres};
use st_domain::{Data, FleetTask, FleetTaskCompletion, FleetsOverview, RegistrationResponse};

pub struct FleetBmc;

struct DbFleetTaskCompletion {
    pub task: Json<FleetTask>,
    pub completed_at: DateTime<Utc>,
}

impl FleetBmc {
    pub async fn load_completed_fleet_tasks(_ctx: &Ctx, mm: &DbModelManager) -> Result<Vec<FleetTaskCompletion>> {
        let completed_tasks: Vec<DbFleetTaskCompletion> = sqlx::query_as!(
            DbFleetTaskCompletion,
            r#"
SELECT task as "task: Json<FleetTask>"
     , completed_at
  from completed_fleet_tasks
        "#,
        )
        .fetch_all(mm.pool())
        .await?;

        Ok(completed_tasks
            .into_iter()
            .map(|db| FleetTaskCompletion {
                task: db.task.0,
                completed_at: db.completed_at,
            })
            .collect_vec())
    }

    pub async fn save_completed_fleet_tasks(_ctx: &Ctx, mm: &DbModelManager, tasks: Vec<FleetTaskCompletion>) -> Result<()> {
        for task in tasks {
            sqlx::query!(
                r#"
insert into completed_fleet_tasks (task, completed_at)
values ($1, $2)
        "#,
                Json(task.clone()) as _,
                task.completed_at
            )
            .execute(mm.pool())
            .await?;
        }

        Ok(())
    }

    //     pub async fn load_fleets(_ctx: &Ctx, mm: &DbModelManager) -> Result<Vec<>> {
    //         let completed_tasks: Vec<DbFleetTaskCompletion> = sqlx::query_as!(
    //             DbFleetTaskCompletion,
    //             r#"
    // SELECT task as "task: Json<FleetTask>"
    //      , completed_at
    //   from completed_fleet_tasks
    //         "#,
    //         )
    //             .fetch_all(mm.pool())
    //             .await?;
    //
    //         Ok(completed_tasks
    //             .into_iter()
    //             .map(|db| FleetTaskCompletion {
    //                 task: db.task.0,
    //                 completed_at: db.completed_at,
    //             })
    //             .collect_vec())
    //     }
}
