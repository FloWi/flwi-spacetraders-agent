use crate::pathfinder::pathfinder;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::Local;
use itertools::Itertools;
use sqlx::{Pool, Postgres};
use st_domain::blackboard_ops::BlackboardOps;
use st_domain::{JumpGate, LabelledCoordinate, MarketData, Shipyard, TicketId, TradeTicket, TravelAction, Waypoint, WaypointSymbol};
use st_store::bmc::{Bmc, DbBmc};
use st_store::{
    insert_jump_gates, insert_market_data, insert_shipyards, select_latest_marketplace_entry_of_system, select_waypoints_of_system, upsert_waypoints, Ctx,
    DbModelManager,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct BehaviorArgs {
    pub blackboard: Arc<dyn BlackboardOps>,
}

// Implement Deref for BehaviorArgs to allow transparent access to BlackboardOps methods
impl std::ops::Deref for BehaviorArgs {
    type Target = dyn BlackboardOps;

    fn deref(&self) -> &Self::Target {
        &*self.blackboard
    }
}

// FIXME: This might be obsolete, if all db accessor functions are moved to their specific bmc implementations
#[derive(Debug, Clone)]
pub struct DbBlackboard {
    pub bmc: DbBmc,
}

impl DbBlackboard {
    fn model_manager(&self) -> DbModelManager {
        self.bmc.db_model_manager.clone()
    }

    fn pool(&self) -> &Pool<Postgres> {
        self.bmc.db_model_manager.pool()
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
        assert_eq!(from.system_symbol(), to.system_symbol(), "Pathfinder currently only works in same system");

        let waypoints_of_system: Vec<Waypoint> = select_waypoints_of_system(self.pool(), &from.system_symbol()).await?;

        let market_entries_of_system: Vec<MarketData> =
            select_latest_marketplace_entry_of_system(self.pool(), &from.system_symbol()).await?.into_iter().map(|me| me.market_data.clone()).collect();

        match pathfinder::compute_path(
            from.clone(),
            to.clone(),
            waypoints_of_system,
            market_entries_of_system,
            engine_speed,
            current_fuel,
            fuel_capacity,
        ) {
            Some(path) => Ok(path),
            None => Err(anyhow!("No path found from {:?} to {:?}", from, to)),
        }
    }

    async fn insert_waypoint(&self, waypoint: &Waypoint) -> Result<()> {
        let now = Local::now().to_utc();
        upsert_waypoints(self.pool(), vec![waypoint.clone()], now).await
    }
    async fn insert_market(&self, market_data: MarketData) -> Result<()> {
        let now = Local::now().to_utc();
        insert_market_data(self.pool(), vec![market_data], now).await
    }
    async fn insert_jump_gate(&self, jump_gate: JumpGate) -> Result<()> {
        let now = Local::now().to_utc();
        insert_jump_gates(self.pool(), vec![jump_gate], now).await
    }
    async fn insert_shipyard(&self, shipyard: Shipyard) -> Result<()> {
        let now = Local::now().to_utc();
        insert_shipyards(self.pool(), vec![shipyard], now).await
    }
    async fn get_closest_waypoint(&self, current_location: &WaypointSymbol, candidates: &[WaypointSymbol]) -> Result<Option<WaypointSymbol>> {
        //TODO: improve by caching a waypoint_map
        let waypoints = select_waypoints_of_system(self.pool(), &current_location.system_symbol()).await?;
        let current_waypoint = waypoints.iter().find(|wp| wp.symbol == *current_location).expect("Current location waypoint");

        Ok(candidates
            .iter()
            .map(|wps| {
                let wp = waypoints.iter().find(|wp| wp.symbol == *wps).expect("candidate waypoint");
                (wps.clone(), current_waypoint.distance_to(wp))
            })
            .sorted_by_key(|(_, distance)| *distance)
            .take(1)
            .next()
            .map(|(best, _)| best))
    }

    async fn get_waypoint(&self, waypoint_symbol: &WaypointSymbol) -> Result<Waypoint> {
        let waypoints = select_waypoints_of_system(self.pool(), &waypoint_symbol.system_symbol()).await?;
        let waypoint = waypoints.into_iter().find(|wp| wp.symbol == *waypoint_symbol).expect("waypoint");

        Ok(waypoint)
    }

    async fn get_ticket_by_id(&self, ticket_id: TicketId) -> Result<TradeTicket> {
        self.bmc.trade_bmc().get_ticket_by_id(&Ctx::Anonymous, ticket_id).await
    }
}
