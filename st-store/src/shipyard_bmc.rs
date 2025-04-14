use crate::ctx::Ctx;
use crate::{db, DbModelManager};
use anyhow::*;
use async_trait::async_trait;
use itertools::Itertools;
use mockall::automock;
use st_domain::{ShipPriceInfo, SystemSymbol, WaypointSymbol};
use std::fmt::Debug;

#[derive(Debug)]
pub struct DbShipyardBmc {
    pub(crate) mm: DbModelManager,
}

#[automock]
#[async_trait]
pub trait ShipyardBmcTrait: Send + Sync + Debug {
    async fn get_latest_ship_prices(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<ShipPriceInfo>;
}

#[async_trait]
impl ShipyardBmcTrait for DbShipyardBmc {
    async fn get_latest_ship_prices(&self, _ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<ShipPriceInfo> {
        let result = db::select_latest_shipyard_entry_of_system(self.mm.pool(), system_symbol).await?;
        let price_infos = result
            .iter()
            .filter_map(|entry| {
                entry.shipyard.has_detailed_price_information().then(|| (entry.waypoint_symbol.clone(), entry.shipyard.ships.clone().unwrap_or_default()))
            })
            .collect_vec();
        Ok(ShipPriceInfo { price_infos })
    }
}
