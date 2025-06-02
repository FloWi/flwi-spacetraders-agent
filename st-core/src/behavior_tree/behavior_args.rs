use crate::pathfinder::pathfinder;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::Local;
use itertools::Itertools;
use sqlx::{Pool, Postgres};
use st_domain::blackboard_ops::BlackboardOps;

use crate::materialized_supply_chain_manager::MaterializedSupplyChainManager;
use crate::survey_manager;
use crate::transfer_cargo_manager::TransferCargoManager;
use st_domain::budgeting::treasury_redesign::{FinanceTicket, ThreadSafeTreasurer};
use st_domain::{
    Construction, CreateSurveyResponse, Extraction, JumpGate, LabelledCoordinate, MarketData, MiningOpsConfig, PurchaseShipResponse, PurchaseTradeGoodResponse,
    SellTradeGoodResponse, Shipyard, SupplyConstructionSiteResponse, Survey, TravelAction, Waypoint, WaypointSymbol,
};
use st_store::bmc::{Bmc, DbBmc};
use st_store::{
    insert_jump_gates, insert_market_data, insert_shipyards, select_latest_marketplace_entry_of_system, select_waypoints_of_system, upsert_waypoints, Ctx,
    DbModelManager,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct BehaviorArgs {
    pub blackboard: Arc<dyn BlackboardOps>,
    pub treasurer: ThreadSafeTreasurer,
    pub transfer_cargo_manager: Arc<TransferCargoManager>,
    pub materialized_supply_chain_manager: MaterializedSupplyChainManager,
}

impl BehaviorArgs {
    pub(crate) async fn mark_purchase_as_completed(&self, ticket: FinanceTicket, response: &PurchaseTradeGoodResponse) -> Result<()> {
        self.treasurer
            .complete_ticket(&ticket.fleet_id, &ticket, response.data.transaction.price_per_unit.into())
            .await?;

        Ok(())
    }

    pub(crate) async fn mark_sell_as_completed(&self, ticket: FinanceTicket, response: &SellTradeGoodResponse) -> Result<()> {
        self.treasurer
            .complete_ticket(&ticket.fleet_id, &ticket, response.data.transaction.price_per_unit.into())
            .await?;

        Ok(())
    }

    pub(crate) async fn mark_ship_purchase_as_completed(&self, ticket: FinanceTicket, response: &PurchaseShipResponse) -> Result<()> {
        self.treasurer
            .complete_ticket(&ticket.fleet_id, &ticket, (response.data.transaction.price as i64).into())
            .await?;

        Ok(())
    }

    pub(crate) async fn mark_construction_delivery_as_completed(&self, ticket: FinanceTicket, response: &SupplyConstructionSiteResponse) -> Result<()> {
        self.treasurer
            .complete_ticket(&ticket.fleet_id, &ticket, 0.into())
            .await?;

        Ok(())
    }
}

// Implement Deref for BehaviorArgs to allow transparent access to BlackboardOps methods
impl std::ops::Deref for BehaviorArgs {
    type Target = dyn BlackboardOps;

    fn deref(&self) -> &Self::Target {
        &*self.blackboard
    }
}
