use crate::bmc::Bmc;
use crate::{Ctx, DbModelManager};
use anyhow::*;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use mockall::automock;
use sqlx::types::Json;
use st_domain::{Fleet, FleetConfig, FleetId, FleetTask, FleetTaskCompletion, FleetsOverview, ShipSymbol, ShipTask};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::RwLock;

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

#[automock]
#[async_trait]
pub trait FleetBmcTrait: Send + Sync + Debug {
    async fn load_fleet_tasks(&self, ctx: &Ctx) -> Result<HashMap<FleetId, Vec<FleetTask>>>;
    async fn load_ship_fleet_assignment(&self, ctx: &Ctx) -> Result<HashMap<ShipSymbol, FleetId>>;
    async fn load_fleets(&self, ctx: &Ctx) -> Result<Vec<Fleet>>;
    async fn load_completed_fleet_tasks(&self, _ctx: &Ctx) -> Result<Vec<FleetTaskCompletion>>;
    async fn save_completed_fleet_task(&self, _ctx: &Ctx, task: &FleetTaskCompletion) -> Result<()>;
    async fn upsert_fleets(&self, _ctx: &Ctx, fleets: &HashMap<FleetId, Fleet>) -> Result<()>;
    async fn upsert_fleet_tasks(&self, _ctx: &Ctx, fleet_tasks: &HashMap<FleetId, Vec<FleetTask>>) -> Result<()>;
    async fn upsert_ship_fleet_assignment(&self, _ctx: &Ctx, ship_fleet_assignment: &HashMap<ShipSymbol, FleetId>) -> Result<()>;
    async fn delete_fleet_ship_assignments_for_fleet(&self, _ctx: &Ctx, fleet_id: &FleetId) -> Result<()>;
    async fn delete_fleet_task_assignments_for_fleet(&self, _ctx: &Ctx, fleet_id: &FleetId) -> Result<()>;
    async fn delete_fleet(&self, _ctx: &Ctx, fleet_id: &FleetId) -> Result<()>;
    async fn delete_fleets(&self, ctx: &Ctx, fleets: &[FleetId]) -> Result<()> {
        for fleet_id in fleets {
            self.delete_fleet_ship_assignments_for_fleet(ctx, fleet_id)
                .await?;
            self.delete_fleet_task_assignments_for_fleet(ctx, fleet_id)
                .await?;
            self.delete_fleet(ctx, fleet_id).await?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct DbFleetBmc {
    pub mm: DbModelManager,
}

#[async_trait]
impl FleetBmcTrait for DbFleetBmc {
    async fn load_fleet_tasks(&self, _ctx: &Ctx) -> Result<HashMap<FleetId, Vec<FleetTask>>> {
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
        .fetch_all(self.mm.pool())
        .await?;

        Ok(assignment_entries
            .into_iter()
            .map(|db| (FleetId(db.fleet_id), db.tasks.0))
            .collect())
    }

    async fn load_ship_fleet_assignment(&self, _ctx: &Ctx) -> Result<HashMap<ShipSymbol, FleetId>> {
        let assignment_entries: Vec<DbShipFleetAssignment> = sqlx::query_as!(
            DbShipFleetAssignment,
            r#"
SELECT fleet_id
     , ship_symbol
  from fleet_ship_assignment
        "#,
        )
        .fetch_all(self.mm.pool())
        .await?;

        Ok(assignment_entries
            .into_iter()
            .map(|db| (ShipSymbol(db.ship_symbol), FleetId(db.fleet_id)))
            .collect())
    }

    async fn load_fleets(&self, _ctx: &Ctx) -> Result<Vec<Fleet>> {
        let fleet_entries: Vec<DbFleetEntry> = sqlx::query_as!(
            DbFleetEntry,
            r#"
SELECT id
     , cfg as "cfg: Json<FleetConfig>"
  from fleets
  "#,
        )
        .fetch_all(self.mm.pool())
        .await?;

        Ok(fleet_entries
            .into_iter()
            .map(|db| Fleet {
                id: FleetId(db.id),
                cfg: db.cfg.0,
            })
            .collect_vec())
    }

    async fn load_completed_fleet_tasks(&self, _ctx: &Ctx) -> Result<Vec<FleetTaskCompletion>> {
        let completed_tasks: Vec<DbFleetTaskCompletion> = sqlx::query_as!(
            DbFleetTaskCompletion,
            r#"
SELECT task as "task: Json<FleetTask>"
     , completed_at
  from completed_fleet_tasks
        "#,
        )
        .fetch_all(self.mm.pool())
        .await?;

        Ok(completed_tasks
            .into_iter()
            .map(|db| FleetTaskCompletion {
                task: db.task.0,
                completed_at: db.completed_at,
            })
            .collect_vec())
    }

    async fn save_completed_fleet_task(&self, _ctx: &Ctx, task: &FleetTaskCompletion) -> Result<()> {
        sqlx::query!(
            r#"
insert into completed_fleet_tasks (task, completed_at)
values ($1, $2)
        "#,
            Json(task.task.clone()) as _,
            task.completed_at
        )
        .execute(self.mm.pool())
        .await?;

        Ok(())
    }

    //     async fn load_fleets(&self, _ctx: &Ctx) -> Result<Vec<>> {
    //         let completed_tasks: Vec<DbFleetTaskCompletion> = sqlx::query_as!(
    //             DbFleetTaskCompletion,
    //             r#"
    // SELECT task as "task: Json<FleetTask>"
    //      , completed_at
    //   from completed_fleet_tasks
    //         "#,
    //         )
    //             .fetch_all(self.mm.pool())
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

    async fn upsert_fleets(&self, _ctx: &Ctx, fleets: &HashMap<FleetId, Fleet>) -> Result<()> {
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
            .execute(self.mm.pool())
            .await?;
        }

        Ok(())
    }

    async fn upsert_fleet_tasks(&self, _ctx: &Ctx, fleet_tasks: &HashMap<FleetId, Vec<FleetTask>>) -> Result<()> {
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
            .execute(self.mm.pool())
            .await?;
        }

        Ok(())
    }

    async fn upsert_ship_fleet_assignment(&self, _ctx: &Ctx, ship_fleet_assignment: &HashMap<ShipSymbol, FleetId>) -> Result<()> {
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
            .execute(self.mm.pool())
            .await?;
        }

        Ok(())
    }

    async fn delete_fleet_ship_assignments_for_fleet(&self, _ctx: &Ctx, fleet_id: &FleetId) -> Result<()> {
        sqlx::query!(
            r#"
delete from fleet_ship_assignment
where fleet_id = $1
"#,
            fleet_id.0,
        )
        .execute(self.mm.pool())
        .await?;

        Ok(())
    }

    async fn delete_fleet_task_assignments_for_fleet(&self, _ctx: &Ctx, fleet_id: &FleetId) -> Result<()> {
        sqlx::query!(
            r#"
delete from fleet_task_assignments
where fleet_id = $1
"#,
            fleet_id.0,
        )
        .execute(self.mm.pool())
        .await?;

        Ok(())
    }

    async fn delete_fleet(&self, _ctx: &Ctx, fleet_id: &FleetId) -> Result<()> {
        sqlx::query!(
            r#"
delete from fleets
where id = $1
"#,
            fleet_id.0,
        )
        .execute(self.mm.pool())
        .await?;

        Ok(())
    }
}

pub async fn upsert_fleets_data(
    bmc: Arc<dyn Bmc>,
    _ctx: &Ctx,
    fleets: &HashMap<FleetId, Fleet>,
    fleet_tasks: &HashMap<FleetId, Vec<FleetTask>>,
    ship_fleet_assignment: &HashMap<ShipSymbol, FleetId>,
    ship_task_assignment: &HashMap<ShipSymbol, ShipTask>,
    //active_trades: &HashMap<ShipSymbol, TradeTicket>,
) -> Result<()> {
    let fleet_bmc = bmc.fleet_bmc();

    fleet_bmc.upsert_fleets(_ctx, fleets).await?;
    fleet_bmc.upsert_fleet_tasks(_ctx, fleet_tasks).await?;
    fleet_bmc
        .upsert_ship_fleet_assignment(_ctx, ship_fleet_assignment)
        .await?;
    bmc.ship_bmc()
        .save_ship_tasks(_ctx, ship_task_assignment)
        .await?;

    // fleet_bmc.upsert_ship_task_assignment(_ctx, &ship_task_assignment).await?;

    let trade_bmc = bmc.trade_bmc();

    // for (ss, ticket) in active_trades {
    //     trade_bmc.upsert_ticket(&Ctx::Anonymous, ss, &ticket.ticket_id(), ticket, ticket.is_complete()).await?
    // }

    Ok(())
}

pub async fn load_fleet_overview(bmc: Arc<dyn Bmc>, ctx: &Ctx) -> Result<FleetsOverview> {
    let fleet_bmc = bmc.fleet_bmc();
    let ship_bmc = bmc.ship_bmc();
    let trade_bmc = bmc.trade_bmc();

    let ships = ship_bmc.get_ships(ctx, None).await?;
    let ship_task_assignments = ship_bmc.load_ship_tasks(ctx).await?;
    let stationary_probe_locations = ship_bmc.get_stationary_probes(ctx).await?;

    let open_trade_tickets = trade_bmc.load_uncompleted_tickets(ctx).await?;

    let completed_fleet_tasks = fleet_bmc.load_completed_fleet_tasks(ctx).await?;
    let ship_fleet_assignment = fleet_bmc.load_ship_fleet_assignment(ctx).await?;
    let fleet_task_assignments = fleet_bmc.load_fleet_tasks(ctx).await?;
    let fleets = fleet_bmc.load_fleets(ctx).await?;

    let all_ships = ships.into_iter().map(|s| (s.symbol.clone(), s)).collect();

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

#[derive(Debug)]
pub struct InMemoryFleet {
    fleet_tasks: HashMap<FleetId, Vec<FleetTask>>,
    fleet_ship_assignments: HashMap<ShipSymbol, FleetId>,
    fleets: HashMap<FleetId, Fleet>,
    completed_fleet_tasks: Vec<FleetTaskCompletion>,
}
#[derive(Debug)]
pub struct InMemoryFleetBmc {
    in_memory_fleet: Arc<RwLock<InMemoryFleet>>,
}

impl Default for InMemoryFleetBmc {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryFleetBmc {
    pub fn new() -> Self {
        Self {
            in_memory_fleet: Arc::new(RwLock::new(InMemoryFleet {
                fleet_tasks: Default::default(),
                fleet_ship_assignments: Default::default(),
                fleets: Default::default(),
                completed_fleet_tasks: vec![],
            })),
        }
    }
}

#[async_trait]
impl FleetBmcTrait for InMemoryFleetBmc {
    async fn load_fleet_tasks(&self, _ctx: &Ctx) -> Result<HashMap<FleetId, Vec<FleetTask>>> {
        Ok(self.in_memory_fleet.read().await.fleet_tasks.clone())
    }

    async fn load_ship_fleet_assignment(&self, _ctx: &Ctx) -> Result<HashMap<ShipSymbol, FleetId>> {
        Ok(self
            .in_memory_fleet
            .read()
            .await
            .fleet_ship_assignments
            .clone())
    }

    async fn load_fleets(&self, _ctx: &Ctx) -> Result<Vec<Fleet>> {
        Ok(self
            .in_memory_fleet
            .read()
            .await
            .fleets
            .values()
            .cloned()
            .collect_vec())
    }

    async fn load_completed_fleet_tasks(&self, _ctx: &Ctx) -> Result<Vec<FleetTaskCompletion>> {
        Ok(self
            .in_memory_fleet
            .read()
            .await
            .completed_fleet_tasks
            .clone())
    }

    async fn save_completed_fleet_task(&self, _ctx: &Ctx, completed_task: &FleetTaskCompletion) -> Result<()> {
        let mut guard = self.in_memory_fleet.write().await;
        guard.completed_fleet_tasks.push(completed_task.clone());

        Ok(())
    }

    async fn upsert_fleets(&self, _ctx: &Ctx, fleets: &HashMap<FleetId, Fleet>) -> Result<()> {
        let mut guard = self.in_memory_fleet.write().await;
        for (fleet_id, fleet) in fleets.iter() {
            guard.fleets.insert(fleet_id.clone(), fleet.clone());
        }

        Ok(())
    }

    async fn upsert_fleet_tasks(&self, _ctx: &Ctx, fleet_tasks: &HashMap<FleetId, Vec<FleetTask>>) -> Result<()> {
        let mut guard = self.in_memory_fleet.write().await;
        for (fleet_id, fleet_tasks) in fleet_tasks.iter() {
            guard
                .fleet_tasks
                .insert(fleet_id.clone(), fleet_tasks.clone());
        }
        Ok(())
    }

    async fn upsert_ship_fleet_assignment(&self, _ctx: &Ctx, ship_fleet_assignment: &HashMap<ShipSymbol, FleetId>) -> Result<()> {
        let mut guard = self.in_memory_fleet.write().await;
        for (ship_symbol, fleet_id) in ship_fleet_assignment.iter() {
            guard
                .fleet_ship_assignments
                .insert(ship_symbol.clone(), fleet_id.clone());
        }
        Ok(())
    }

    async fn delete_fleet_ship_assignments_for_fleet(&self, _ctx: &Ctx, fleet_id: &FleetId) -> Result<()> {
        let mut guard = self.in_memory_fleet.write().await;
        guard.fleet_ship_assignments.retain(|_, id| id != fleet_id);
        Ok(())
    }

    async fn delete_fleet_task_assignments_for_fleet(&self, _ctx: &Ctx, fleet_id: &FleetId) -> Result<()> {
        let mut guard = self.in_memory_fleet.write().await;
        guard.fleet_tasks.remove(fleet_id);
        Ok(())
    }

    async fn delete_fleet(&self, _ctx: &Ctx, fleet_id: &FleetId) -> Result<()> {
        let mut guard = self.in_memory_fleet.write().await;
        guard.fleets.remove(fleet_id);
        Ok(())
    }
}
