use crate::ctx::Ctx;
use crate::{db, DbMarketEntry, DbModelManager, DbShipyardData};
use anyhow::*;
use async_trait::async_trait;
use itertools::Itertools;
use mockall::automock;
use st_domain::{SystemSymbol, Waypoint};
use std::fmt::Debug;

#[derive(Debug)]
pub struct DbSystemBmc {
    pub(crate) mm: DbModelManager,
}

#[automock]
#[async_trait]
pub trait SystemBmcTrait: Send + Sync + Debug {
    async fn get_waypoints_of_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<Waypoint>>;
    async fn select_latest_marketplace_entry_of_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<DbMarketEntry>>;
    async fn select_latest_shipyard_entry_of_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<DbShipyardData>>;
}

#[async_trait]
impl SystemBmcTrait for DbSystemBmc {
    async fn get_waypoints_of_system(&self, _ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<Waypoint>> {
        db::select_waypoints_of_system(self.mm.pool(), system_symbol).await
    }

    async fn select_latest_marketplace_entry_of_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<DbMarketEntry>> {
        db::select_latest_marketplace_entry_of_system(self.mm.pool(), system_symbol).await
    }

    async fn select_latest_shipyard_entry_of_system(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Vec<DbShipyardData>> {
        db::select_latest_shipyard_entry_of_system(self.mm.pool(), system_symbol).await
    }
}
