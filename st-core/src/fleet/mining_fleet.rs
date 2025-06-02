use crate::fleet::fleet::FleetAdmiral;
use anyhow::anyhow;
use st_domain::{
    Fleet, FleetDecisionFacts, MarketObservationFleetConfig, MiningFleetConfig, RawDeliveryRoute, RawMaterialSource, RawMaterialSourceType, Ship, ShipSymbol,
    ShipTask, SupplyLevel, SystemSpawningFleetConfig, TradeGoodSymbol, WaypointSymbol,
};
use std::collections::{HashMap, HashSet};
use tracing::event;
use tracing_core::Level;

pub struct MiningFleet;

impl MiningFleet {
    pub fn compute_ship_tasks(cfg: &MiningFleetConfig, ships: &[&Ship]) -> anyhow::Result<HashMap<ShipSymbol, ShipTask>> {
        if ships.is_empty() {
            return Ok(HashMap::new());
        }

        let new_tasks: HashMap<ShipSymbol, ShipTask> = ships
            .iter()
            .filter_map(|s| match Self::calc_ship_task(*s, cfg) {
                Ok(task) => Some((s.symbol.clone(), task)),
                Err(e) => {
                    event!(Level::ERROR, "Failed to compute ship task: {:?}", e);
                    None
                }
            })
            .collect();

        Ok(new_tasks)
    }
    fn calc_ship_task(s: &Ship, cfg: &MiningFleetConfig) -> anyhow::Result<ShipTask> {
        let task = if s.is_mining_drone() {
            ShipTask::MineMaterialsAtWaypoint {
                mining_waypoint: cfg.mining_waypoint.clone(),
            }
        } else if s.is_surveyor() {
            ShipTask::SurveyMiningSite {
                mining_waypoint: cfg.mining_waypoint.clone(),
            }
        } else if s.is_hauler() {
            ShipTask::HaulMiningGoods {
                mining_waypoint: cfg.mining_waypoint.clone(),
            }
        } else {
            anyhow::bail!("This should not happen. The type of the ship doesn't match our expectation in the mining-fleet. We're quite picky here... {s:?}")
        };

        Ok(task)
    }
}
