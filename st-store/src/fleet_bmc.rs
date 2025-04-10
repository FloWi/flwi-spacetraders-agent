use crate::trade_bmc::TradeBmc;
use crate::{Ctx, DbModelManager, ShipBmc};
use anyhow::*;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use sqlx::types::Json;
use st_domain::{Fleet, FleetConfig, FleetId, FleetTask, FleetTaskCompletion, FleetsOverview, ShipSymbol, ShipTask, TradeTicket};
use std::collections::HashMap;

pub struct FleetBmc;

struct DbFleetTaskCompletion {
    pub task: Json<FleetTask>,
    pub completed_at: DateTime<Utc>,
}

struct DbShipFleetAssignment {
    ship_symbol: String,
    fleet_id: i32,
}

struct DbFleetTaskAssignment {
    fleet_id: i32,
    tasks: Json<Vec<FleetTask>>,
}

struct DbFleetEntry {
    cfg: Json<FleetConfig>,
    id: i32,
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
                Json(task.task.clone()) as _,
                task.completed_at
            )
            .execute(mm.pool())
            .await?;
        }

        Ok(())
    }

    pub async fn load_fleet_tasks(ctx: &Ctx, mm: &DbModelManager) -> Result<HashMap<FleetId, Vec<FleetTask>>> {
        /*
        fleet_id: Json<FleetId>
        fleet_task: Json<FleetTask>
                 */
        let assignment_entries: Vec<DbFleetTaskAssignment> = sqlx::query_as!(
            DbFleetTaskAssignment,
            r#"
SELECT fleet_id
     , tasks as "tasks: Json<Vec<FleetTask>>"
  from fleet_task_assignments
        "#,
        )
        .fetch_all(mm.pool())
        .await?;

        Ok(assignment_entries.into_iter().map(|db| (FleetId(db.fleet_id), db.tasks.0)).collect())
    }

    pub async fn load_ship_fleet_assignment(ctx: &Ctx, mm: &DbModelManager) -> Result<HashMap<ShipSymbol, FleetId>> {
        let assignment_entries: Vec<DbShipFleetAssignment> = sqlx::query_as!(
            DbShipFleetAssignment,
            r#"
SELECT fleet_id
     , ship_symbol
  from fleet_ship_assignment
        "#,
        )
        .fetch_all(mm.pool())
        .await?;

        Ok(assignment_entries.into_iter().map(|db| (ShipSymbol(db.ship_symbol), FleetId(db.fleet_id))).collect())
    }

    pub async fn load_fleets(ctx: &Ctx, mm: &DbModelManager) -> Result<Vec<Fleet>> {
        let fleet_entries: Vec<DbFleetEntry> = sqlx::query_as!(
            DbFleetEntry,
            r#"
SELECT id
     , cfg as "cfg: Json<FleetConfig>"
  from fleets
  "#,
        )
        .fetch_all(mm.pool())
        .await?;

        Ok(fleet_entries
            .into_iter()
            .map(|db| Fleet {
                id: FleetId(db.id),
                cfg: db.cfg.0,
            })
            .collect_vec())
    }

    pub async fn load_overview(ctx: &Ctx, mm: &DbModelManager) -> Result<FleetsOverview> {
        let completed_fleet_tasks = Self::load_completed_fleet_tasks(ctx, mm).await?;
        let fleets = Self::load_fleets(ctx, mm).await?;
        let ships = ShipBmc::get_ships(ctx, mm, None).await?;
        let all_ships = ships.into_iter().map(|s| (s.symbol.clone(), s)).collect();
        let ship_fleet_assignment = Self::load_ship_fleet_assignment(ctx, mm).await?;
        let fleet_task_assignments = Self::load_fleet_tasks(ctx, mm).await?;
        let ship_task_assignments = ShipBmc::load_ship_tasks(ctx, mm).await?;
        let open_trade_tickets = TradeBmc::load_uncompleted_tickets(ctx, mm).await?;
        let stationary_probe_locations = ShipBmc::get_stationary_probes(ctx, mm).await?;

        Ok(FleetsOverview {
            completed_fleet_tasks,
            fleets: fleets.into_iter().map(|f| (f.id.clone(), f)).collect(),
            all_ships,
            fleet_task_assignments,
            ship_fleet_assignment,
            ship_tasks: ship_task_assignments,
            open_trade_tickets,
            stationary_probe_locations,
        })
    }

    pub async fn store_fleets_data(
        _ctx: &Ctx,
        mm: &DbModelManager,
        fleets: &HashMap<FleetId, Fleet>,
        fleet_tasks: &HashMap<FleetId, Vec<FleetTask>>,
        ship_fleet_assignment: &HashMap<ShipSymbol, FleetId>,
        ship_task_assignment: &HashMap<ShipSymbol, ShipTask>,
        active_trades: &HashMap<ShipSymbol, TradeTicket>,
    ) -> Result<()> {
        Self::upsert_fleets(_ctx, mm, &fleets).await?;
        Self::upsert_fleet_tasks(_ctx, mm, &fleet_tasks).await?;
        Self::upsert_ship_fleet_assignment(_ctx, mm, &ship_fleet_assignment).await?;
        Self::upsert_ship_task_assignment(_ctx, mm, &ship_task_assignment).await?;

        for (ss, ticket) in active_trades {
            TradeBmc::upsert_ticket(&Ctx::Anonymous, mm, &ss, &ticket.ticket_id(), &ticket, ticket.is_complete()).await?
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

    async fn upsert_fleets(_ctx: &Ctx, mm: &DbModelManager, fleets: &HashMap<FleetId, Fleet>) -> Result<()> {
        //TODO: upsert all at once (prob. json array magic)

        for (fleet_id, fleet) in fleets {
            sqlx::query!(
                r#"
insert into fleets(id, cfg)
values ($1, $2)
on conflict (id) do update SET cfg = excluded.cfg
"#,
                fleet_id.0,
                Json(fleet.cfg.clone()) as _,
            )
            .execute(mm.pool())
            .await?;
        }

        Ok(())
    }

    async fn upsert_fleet_tasks(_ctx: &Ctx, mm: &DbModelManager, fleet_tasks: &HashMap<FleetId, Vec<FleetTask>>) -> Result<()> {
        //TODO: upsert all at once (prob. json array magic)

        //a fleet can (currently only have one task)

        for (fleet_id, fleet_tasks) in fleet_tasks {
            sqlx::query!(
                r#"
insert into fleet_task_assignments(fleet_id, tasks)
values ($1, $2)
on conflict (fleet_id) do update SET tasks = excluded.tasks
"#,
                fleet_id.0,
                Json(fleet_tasks.clone()) as _,
            )
            .execute(mm.pool())
            .await?;
        }

        Ok(())
    }

    async fn upsert_ship_fleet_assignment(_ctx: &Ctx, mm: &DbModelManager, ship_fleet_assignment: &HashMap<ShipSymbol, FleetId>) -> Result<()> {
        //TODO: upsert all at once (prob. json array magic)

        for (ship_symbol, fleet_id) in ship_fleet_assignment {
            sqlx::query!(
                r#"
insert into fleet_ship_assignment(ship_symbol, fleet_id)
values ($1, $2)
on conflict (ship_symbol) do update SET fleet_id = excluded.fleet_id
"#,
                ship_symbol.0,
                fleet_id.0,
            )
            .execute(mm.pool())
            .await?;
        }

        Ok(())
    }

    async fn upsert_ship_task_assignment(_ctx: &Ctx, mm: &DbModelManager, ship_task_assignment: &HashMap<ShipSymbol, ShipTask>) -> Result<()> {
        for (ship_symbol, ship_task) in ship_task_assignment {
            sqlx::query!(
                r#"
insert into ship_task_assignments(ship_symbol, task)
values ($1, $2)
on conflict (ship_symbol) do update SET task = excluded.task
"#,
                ship_symbol.0,
                Json(ship_task.clone()) as _,
            )
            .execute(mm.pool())
            .await?;
        }

        Ok(())
    }
}
