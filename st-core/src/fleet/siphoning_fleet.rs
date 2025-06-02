use st_domain::{Ship, ShipSymbol, ShipTask, SiphoningFleetConfig};
use std::collections::HashMap;

pub struct SiphoningFleet;

impl SiphoningFleet {
    pub fn compute_ship_tasks(cfg: &SiphoningFleetConfig, ships: &[&Ship]) -> anyhow::Result<HashMap<ShipSymbol, ShipTask>> {
        if ships.is_empty() {
            return Ok(HashMap::new());
        }

        let new_tasks: HashMap<ShipSymbol, ShipTask> = ships
            .iter()
            .map(|s| {
                (
                    s.symbol.clone(),
                    ShipTask::SiphonCarboHydratesAtWaypoint {
                        siphoning_waypoint: cfg.siphoning_waypoint.clone(),
                    },
                )
            })
            .collect();

        Ok(new_tasks)
    }
}
