use crate::fleet::fleet::FleetAdmiral;
use anyhow::anyhow;
use st_domain::{
    Fleet, FleetDecisionFacts, MarketObservationFleetConfig, RawMaterialSource, RawMaterialSourceType, Ship, ShipSymbol, ShipTask, SiphoningFleetConfig,
    SupplyLevel, SystemSpawningFleetConfig,
};
use std::collections::{HashMap, HashSet};

pub struct SiphoningFleet;

impl SiphoningFleet {
    pub fn compute_ship_tasks(
        admiral: &FleetAdmiral,
        cfg: &SiphoningFleetConfig,
        fleet: &Fleet,
        facts: &FleetDecisionFacts,
        ships: &[&Ship],
    ) -> anyhow::Result<HashMap<ShipSymbol, ShipTask>> {
        if ships.is_empty() {
            return Ok(HashMap::new());
        }

        if let Some(msc) = &facts.materialized_supply_chain {
            let delivery_locations = msc
                .raw_delivery_routes
                .iter()
                .filter_map(|route| match &route.source {
                    RawMaterialSource { trade_good, source_type, .. } if *source_type == RawMaterialSourceType::Siphoning => {
                        Some((trade_good.clone(), route.clone()))
                    }
                    _ => None,
                })
                .collect::<HashMap<_, _>>();

            let demanded_goods = msc
                .raw_delivery_routes
                .iter()
                .filter_map(|route| match &route.source {
                    RawMaterialSource { trade_good, source_type, .. } if *source_type == RawMaterialSourceType::Siphoning => {
                        let is_supply_too_low = (route.delivery_market_entry.supply < SupplyLevel::High);

                        is_supply_too_low.then_some(trade_good.clone())
                    }
                    _ => None,
                })
                .collect::<HashSet<_>>();

            let new_tasks: HashMap<ShipSymbol, ShipTask> = ships
                .iter()
                .map(|s| {
                    (
                        s.symbol.clone(),
                        ShipTask::SiphonCarboHydratesAtWaypoint {
                            siphoning_waypoint: cfg.siphoning_waypoint.clone(),
                            delivery_locations: delivery_locations.clone(),
                            demanded_goods: demanded_goods.clone(),
                        },
                    )
                })
                .collect();

            Ok(new_tasks)
        } else {
            Err(anyhow!("Missing materialized supply chain"))
        }
    }
}
