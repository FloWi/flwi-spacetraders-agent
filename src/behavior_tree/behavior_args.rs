use crate::db;
use crate::db::*;
use crate::pathfinder::pathfinder;
use crate::pathfinder::pathfinder::TravelAction;
use crate::st_model::{
    JumpGate, MarketData, Ship, Shipyard, Waypoint, WaypointSymbol, WaypointTraitSymbol,
    WaypointType,
};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::Local;
use sqlx::{Pool, Postgres};
use std::sync::Arc;
use strum_macros::Display;

#[async_trait]
pub trait BlackboardOps: Send + Sync {
    async fn compute_path(
        &self,
        from: WaypointSymbol,
        to: WaypointSymbol,
        engine_speed: u32,
        current_fuel: u32,
        fuel_capacity: u32,
    ) -> Result<Vec<TravelAction>>;
    async fn get_exploration_tasks_for_current_waypoint(
        &self,
        current_location: WaypointSymbol,
    ) -> Result<Vec<ExplorationTask>>;
    async fn insert_waypoint(&self, waypoint: &Waypoint) -> Result<()>;
    async fn insert_market(&self, market_data: MarketData) -> Result<()>;
    async fn insert_jump_gate(&self, jump_gate: JumpGate) -> Result<()>;
    async fn insert_shipyard(&self, shipyard: Shipyard) -> Result<()>;
}

#[derive(Clone)]
pub struct BehaviorArgs {
    pub(crate) blackboard: Arc<dyn BlackboardOps>,
}

#[derive(Debug, Clone)]
pub struct DbBlackboard {
    pub db: Pool<Postgres>,
}

// Implement Deref for BehaviorArgs to allow transparent access to BlackboardOps methods
impl std::ops::Deref for BehaviorArgs {
    type Target = dyn BlackboardOps;

    fn deref(&self) -> &Self::Target {
        &*self.blackboard
    }
}

#[async_trait]
impl BlackboardOps for DbBlackboard {
    async fn compute_path(
        &self,
        from: WaypointSymbol,
        to: WaypointSymbol,
        engine_speed: u32,
        current_fuel: u32,
        fuel_capacity: u32,
    ) -> Result<Vec<TravelAction>> {
        assert_eq!(
            from.system_symbol(),
            to.system_symbol(),
            "Pathfinder currently only works in same system"
        );

        let waypoints_of_system: Vec<Waypoint> =
            select_waypoints_of_system(&self.db, &from.system_symbol())
                .await?
                .into_iter()
                .map(|db_wp| db_wp.entry.0.clone())
                .collect();

        let market_entries_of_system: Vec<MarketData> =
            select_latest_marketplace_entry_of_system(&self.db, &from.system_symbol())
                .await?
                .into_iter()
                .map(|db_wp| db_wp.entry.0.clone())
                .collect();

        match pathfinder::compute_path(
            from,
            to,
            waypoints_of_system,
            market_entries_of_system,
            engine_speed,
            current_fuel,
            fuel_capacity,
        ) {
            Some(path) => Ok(path),
            None => Err(anyhow!("No path found")),
        }
    }
    async fn get_exploration_tasks_for_current_waypoint(
        &self,
        current_location: WaypointSymbol,
    ) -> Result<Vec<ExplorationTask>> {
        let waypoints =
            select_waypoints_of_system(&self.db, &current_location.system_symbol()).await?;

        //let maybe_jump_gate: Option<DbJumpGateData> = db::select_jump_gate(&self.db, &current_location).await?;

        match waypoints
            .iter()
            .find(|wp| wp.entry.symbol == current_location)
        {
            None => Err(anyhow::anyhow!("can't find waypoint in db")),
            Some(wp) => {
                let mut tasks = Vec::new();
                if wp
                    .entry
                    .traits
                    .iter()
                    .any(|t| t.symbol == WaypointTraitSymbol::UNCHARTED)
                {
                    tasks.push(ExplorationTask::CreateChart);
                }
                if wp
                    .entry
                    .traits
                    .iter()
                    .any(|t| t.symbol == WaypointTraitSymbol::SHIPYARD)
                {
                    tasks.push(ExplorationTask::GetShipyard);
                }
                if wp
                    .entry
                    .traits
                    .iter()
                    .any(|t| t.symbol == WaypointTraitSymbol::MARKETPLACE)
                {
                    tasks.push(ExplorationTask::GetMarket);
                }
                if wp.entry.r#type == WaypointType::JUMP_GATE {
                    //maybe_jump_gate.map(|db_jg| db_jg.)
                    tasks.push(ExplorationTask::GetJumpGate);
                }

                Ok(tasks)
            }
        }
    }
    async fn insert_waypoint(&self, waypoint: &Waypoint) -> Result<()> {
        let now = Local::now().to_utc();
        upsert_waypoints(&self.db, vec![waypoint.clone()], now).await
    }
    async fn insert_market(&self, market_data: MarketData) -> Result<()> {
        let now = Local::now().to_utc();
        insert_market_data(&self.db, vec![market_data], now).await
    }
    async fn insert_jump_gate(&self, jump_gate: JumpGate) -> Result<()> {
        let now = Local::now().to_utc();
        insert_jump_gates(&self.db, vec![jump_gate], now).await
    }
    async fn insert_shipyard(&self, shipyard: Shipyard) -> Result<()> {
        let now = Local::now().to_utc();
        insert_shipyards(&self.db, vec![shipyard], now).await
    }
}

/// What observation to do once a ship is present at this waypoint
#[derive(Eq, PartialEq, Debug, Display)]
pub enum ExplorationTask {
    GetMarket,
    GetJumpGate,
    CreateChart,
    GetShipyard,
}
