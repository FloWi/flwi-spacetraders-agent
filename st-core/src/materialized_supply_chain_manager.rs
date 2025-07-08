use anyhow::anyhow;
use st_domain::{
    MaterializedSupplyChain, MiningOpsConfig, RawDeliveryRoute, RawMaterialSource, RawMaterialSourceType, SiphoningOpsConfig, SupplyLevel,
    SystemSymbol, TradeGoodSymbol, WaypointSymbol,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::Mutex;
use tracing::debug;

#[derive(Clone, Debug)]
pub struct MaterializedSupplyChainManager {
    materialized_supply_chain: Arc<Mutex<HashMap<SystemSymbol, MaterializedSupplyChain>>>,
}

impl Default for MaterializedSupplyChainManager {
    fn default() -> Self {
        Self::new()
    }
}

impl MaterializedSupplyChainManager {
    pub fn new() -> Self {
        Self {
            materialized_supply_chain: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn register_materialized_supply_chain(&self, system_symbol: SystemSymbol, materialized_supply_chain: MaterializedSupplyChain) -> anyhow::Result<()> {
        self.materialized_supply_chain
            .lock()
            .map_err(|_| anyhow!("Lock poisoned"))?
            .insert(system_symbol.clone(), materialized_supply_chain);

        debug!("Updated materialized supply chain registered for {}", system_symbol);

        Ok(())
    }

    pub fn get_materialized_supply_chain_for_system(&self, system: SystemSymbol) -> Option<MaterializedSupplyChain> {
        self.materialized_supply_chain
            .lock()
            .ok()?
            .get(&system)
            .cloned()
    }

    pub fn get_mining_ops_config_for_system(&self, system: SystemSymbol) -> Option<MiningOpsConfig> {
        let msc = self
            .materialized_supply_chain
            .lock()
            .ok()?
            .get(&system)
            .cloned()?;

        let raw_material_source_type = RawMaterialSourceType::Mining;

        let (mining_waypoint, delivery_locations, demanded_goods) = get_locations_and_demand_for_raw_material(raw_material_source_type, msc);

        Some(MiningOpsConfig {
            mining_waypoint,
            demanded_goods,
            delivery_locations,
        })
    }

    pub fn get_siphoning_ops_config_for_system(&self, system: SystemSymbol) -> Option<SiphoningOpsConfig> {
        let msc = self
            .materialized_supply_chain
            .lock()
            .ok()?
            .get(&system)
            .cloned()?;

        let raw_material_source_type = RawMaterialSourceType::Siphoning;

        let (siphoning_waypoint, delivery_locations, demanded_goods) = get_locations_and_demand_for_raw_material(raw_material_source_type, msc);

        Some(SiphoningOpsConfig {
            siphoning_waypoint,
            demanded_goods,
            delivery_locations,
        })
    }

    pub(crate) fn get_raw_delivery_routes(&self, system_symbol: &SystemSymbol) -> anyhow::Result<HashMap<TradeGoodSymbol, RawDeliveryRoute>> {
        if let Some(msc) = self
            .materialized_supply_chain
            .lock()
            .map_err(|_| anyhow!("Lock poisoned"))?
            .get(system_symbol)
            .cloned()
        {
            let raw_routes = msc
                .raw_delivery_routes
                .iter()
                .map(|raw| (raw.delivery_market_entry.symbol.clone(), raw.clone()))
                .collect();

            Ok(raw_routes)
        } else {
            Err(anyhow!("Unable to get delivery locations for system {}", system_symbol))
        }
    }
}

fn get_locations_and_demand_for_raw_material(
    raw_material_source_type: RawMaterialSourceType,
    msc: MaterializedSupplyChain,
) -> (WaypointSymbol, HashMap<TradeGoodSymbol, RawDeliveryRoute>, HashSet<TradeGoodSymbol>) {
    let delivery_locations = msc
        .raw_delivery_routes
        .iter()
        .filter_map(|route| match &route.source {
            RawMaterialSource { trade_good, source_type, .. } if *source_type == raw_material_source_type => Some((trade_good.clone(), route.clone())),
            _ => None,
        })
        .collect::<HashMap<_, _>>();

    let demanded_goods = msc
        .raw_delivery_routes
        .iter()
        .filter_map(|route| match &route.source {
            RawMaterialSource { trade_good, source_type, .. } if *source_type == raw_material_source_type => {
                let is_supply_too_low = route.delivery_market_entry.supply < SupplyLevel::High;

                is_supply_too_low.then_some(trade_good.clone())
            }
            _ => None,
        })
        .collect::<HashSet<_>>();

    let waypoint = msc
        .source_waypoints
        .get(&raw_material_source_type)
        .unwrap()
        .first()
        .unwrap();
    (waypoint.symbol.clone(), delivery_locations, demanded_goods)
}
