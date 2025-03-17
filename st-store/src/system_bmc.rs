use crate::ctx::Ctx;
use crate::{db, DbModelManager};
use anyhow::*;
use itertools::Itertools;
use st_domain::{SystemSymbol, Waypoint};

pub struct SystemBmc;

impl SystemBmc {
    pub async fn get_waypoints_of_system(
        ctx: &Ctx,
        mm: &DbModelManager,
        system_symbol: &SystemSymbol,
    ) -> Result<Vec<Waypoint>> {
        let waypoints = db::select_waypoints_of_system(mm.pool(), system_symbol)
            .await?
            .into_iter()
            .map(|db_entry| db_entry.entry.0.clone())
            .collect_vec();

        Ok(waypoints)
    }
}
