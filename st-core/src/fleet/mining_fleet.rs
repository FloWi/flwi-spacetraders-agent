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
    pub fn compute_ship_tasks(
        admiral: &FleetAdmiral,
        cfg: &MiningFleetConfig,
        fleet: &Fleet,
        facts: &FleetDecisionFacts,
        ships: &[&Ship],
    ) -> anyhow::Result<HashMap<ShipSymbol, ShipTask>> {
        if ships.is_empty() {
            return Ok(HashMap::new());
        }

        if let Some(msc) = &facts.materialized_supply_chain {
            let delivery_locations: HashMap<TradeGoodSymbol, RawDeliveryRoute> = msc
                .raw_delivery_routes
                .iter()
                .filter_map(|route| match &route.source {
                    RawMaterialSource { trade_good, source_type, .. } if *source_type == RawMaterialSourceType::Mining => {
                        Some((trade_good.clone(), route.clone()))
                    }
                    _ => None,
                })
                .collect::<HashMap<_, _>>();

            let demanded_goods: HashSet<TradeGoodSymbol> = msc
                .raw_delivery_routes
                .iter()
                .filter_map(|route| match &route.source {
                    RawMaterialSource { trade_good, source_type, .. } if *source_type == RawMaterialSourceType::Mining => {
                        let is_supply_too_low = (route.delivery_market_entry.supply < SupplyLevel::High);

                        is_supply_too_low.then_some(trade_good.clone())
                    }
                    _ => None,
                })
                .collect::<HashSet<_>>();

            let new_tasks: HashMap<ShipSymbol, ShipTask> = ships
                .iter()
                .filter_map(|s| match Self::calc_ship_task(*s, cfg, &delivery_locations, &demanded_goods) {
                    Ok(task) => Some((s.symbol.clone(), task)),
                    Err(e) => {
                        event!(Level::ERROR, "Failed to compute ship task: {:?}", e);
                        None
                    }
                })
                .collect();

            Ok(new_tasks)
        } else {
            Err(anyhow!("Missing materialized supply chain"))
        }
    }
    fn calc_ship_task(
        s: &Ship,
        cfg: &MiningFleetConfig,
        delivery_locations: &HashMap<TradeGoodSymbol, RawDeliveryRoute>,
        demanded_goods: &HashSet<TradeGoodSymbol>,
    ) -> anyhow::Result<ShipTask> {
        let task = if s.is_mining_drone() {
            ShipTask::MineMaterialsAtWaypoint {
                mining_waypoint: cfg.mining_waypoint.clone(),
                demanded_goods: demanded_goods.clone(),
            }
        } else if s.is_surveyor() {
            ShipTask::SurveyMiningSite {
                mining_waypoint: cfg.mining_waypoint.clone(),
            }
        } else if s.is_hauler() {
            ShipTask::HaulMiningGoods {
                mining_waypoint: cfg.mining_waypoint.clone(),
                delivery_locations: delivery_locations.clone(),
                demanded_goods: demanded_goods.clone(),
            }
        } else {
            anyhow::bail!("This should not happen. The type of the ship doesn't match our expectation in the mining-fleet. We're quite picky here... {s:?}")
        };

        Ok(task)
    }
}
