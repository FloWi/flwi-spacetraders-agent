use crate::ctx::Ctx;
use crate::{db, DbModelManager};
use anyhow::*;
use itertools::Itertools;
use st_domain::{ShipPriceInfo, SystemSymbol, WaypointSymbol};

pub struct ShipyardBmc;

impl ShipyardBmc {
    pub async fn get_latest_ship_prices(_ctx: &Ctx, mm: &DbModelManager, system_symbol: &SystemSymbol) -> Result<ShipPriceInfo> {
        let result = db::select_latest_shipyard_entry_of_system(mm.pool(), system_symbol).await?;
        let price_infos = result
            .iter()
            .filter_map(|entry| {
                entry.has_detailed_price_information().then(|| (WaypointSymbol(entry.waypoint_symbol.clone()), entry.entry.ships.clone().unwrap_or_default()))
            })
            .collect_vec();
        Ok(ShipPriceInfo { price_infos })
    }
}
