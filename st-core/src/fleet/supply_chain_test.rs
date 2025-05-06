use anyhow::Result;
use itertools::Itertools;
use st_domain::{FleetDecisionFacts, FleetPhaseName, MarketEntry, ShipSymbol, SupplyChain, TradeGoodSymbol, WaypointSymbol};
use std::collections::{HashMap, HashSet};
use std::ops::Not;
use strum::IntoEnumIterator;

#[cfg(test)]
mod tests {
    use crate::bmc_blackboard::BmcBlackboard;
    use crate::fleet::fleet::collect_fleet_decision_facts;
    use crate::fleet::fleet_runner::FleetRunner;
    use crate::fleet::supply_chain_test::calc_trading_decisions;
    use crate::st_client::StClientTrait;
    use crate::universe_server::universe_server::{InMemoryUniverse, InMemoryUniverseClient, InMemoryUniverseOverrides};
    use anyhow::Result;
    use itertools::Itertools;
    use st_domain::{FleetPhaseName, ShipSymbol, TradeGoodSymbol, WaypointSymbol};
    use st_store::bmc::jump_gate_bmc::InMemoryJumpGateBmc;
    use st_store::bmc::ship_bmc::{InMemoryShips, InMemoryShipsBmc};
    use st_store::bmc::{Bmc, InMemoryBmc};
    use st_store::shipyard_bmc::InMemoryShipyardBmc;
    use st_store::trade_bmc::InMemoryTradeBmc;
    use st_store::{
        Ctx, InMemoryAgentBmc, InMemoryConstructionBmc, InMemoryFleetBmc, InMemoryMarketBmc, InMemoryStatusBmc, InMemorySupplyChainBmc, InMemorySystemsBmc,
    };
    use std::collections::HashSet;
    use std::ops::Not;
    use std::sync::Arc;
    use test_log::test;

    #[test(tokio::test)]
    //#[tokio::test] // for accessing runtime-infos with tokio-console
    async fn test_supply_chain_materialization() -> Result<()> {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");

        let json_path = std::path::Path::new(manifest_dir).parent().unwrap().join("resources").join("universe_snapshot.json");

        let in_memory_universe = InMemoryUniverse::from_snapshot(json_path).expect("InMemoryUniverse::from_snapshot");

        let shipyard_waypoints = in_memory_universe.shipyards.keys().cloned().collect::<HashSet<_>>();
        let marketplace_waypoints = in_memory_universe.marketplaces.keys().cloned().collect::<HashSet<_>>();

        let in_memory_client = InMemoryUniverseClient::new(in_memory_universe);

        let agent = in_memory_client.get_agent().await.expect("agent").data;
        let hq_system_symbol = agent.headquarters.system_symbol();

        let ship_bmc = InMemoryShipsBmc::new(InMemoryShips::new());
        let agent_bmc = InMemoryAgentBmc::new(agent);
        let trade_bmc = InMemoryTradeBmc::new();
        let fleet_bmc = InMemoryFleetBmc::new();
        let system_bmc = InMemorySystemsBmc::new();
        let construction_bmc = InMemoryConstructionBmc::new();

        //insert some data
        //construction_bmc.save_construction_site(&Ctx::Anonymous, in_memory_client.get_construction_site().unwrap())

        let market_bmc = InMemoryMarketBmc::new();
        let shipyard_bmc = InMemoryShipyardBmc::new();
        let jump_gate_bmc = InMemoryJumpGateBmc::new();
        let supply_chain_bmc = InMemorySupplyChainBmc::new();
        let status_bmc = InMemoryStatusBmc::new();

        let trade_bmc = Arc::new(trade_bmc);
        let market_bmc = Arc::new(market_bmc);
        let bmc = InMemoryBmc {
            in_mem_ship_bmc: Arc::new(ship_bmc),
            in_mem_fleet_bmc: Arc::new(fleet_bmc),
            in_mem_trade_bmc: Arc::clone(&trade_bmc),
            in_mem_system_bmc: Arc::new(system_bmc),
            in_mem_agent_bmc: Arc::new(agent_bmc),
            in_mem_construction_bmc: Arc::new(construction_bmc),
            in_mem_market_bmc: Arc::clone(&market_bmc),
            in_mem_jump_gate_bmc: Arc::new(jump_gate_bmc),
            in_mem_shipyard_bmc: Arc::new(shipyard_bmc),
            in_mem_supply_chain_bmc: Arc::new(supply_chain_bmc),
            in_mem_status_bmc: Arc::new(status_bmc),
        };

        let client = Arc::new(in_memory_client) as Arc<dyn StClientTrait>;
        let bmc = Arc::new(bmc) as Arc<dyn Bmc>;
        let blackboard = BmcBlackboard::new(Arc::clone(&bmc));

        FleetRunner::load_and_store_initial_data_in_bmcs(Arc::clone(&client), Arc::clone(&bmc)).await.expect("FleetRunner::load_and_store_initial_data");

        // easier to get the supply chain this way, since we need plenty of things for computing it
        let facts = collect_fleet_decision_facts(bmc, &hq_system_symbol).await?;
        let materialized_supply_chain = facts.materialized_supply_chain.unwrap();

        assert!(
            materialized_supply_chain.raw_delivery_routes.is_empty(),
            "empty on first run, since we didn't have the exact market_data yet"
        );

        Ok(())
    }

    #[test(tokio::test)]
    //#[tokio::test] // for accessing runtime-infos with tokio-console
    async fn test_supply_chain_materialization_with_precise_marketdata() -> Result<()> {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");

        let json_path = std::path::Path::new(manifest_dir).parent().unwrap().join("resources").join("universe_snapshot.json");

        let in_memory_universe = InMemoryUniverse::from_snapshot(json_path).expect("InMemoryUniverse::from_snapshot");

        let shipyard_waypoints = in_memory_universe.shipyards.keys().cloned().collect::<HashSet<_>>();
        let marketplace_waypoints = in_memory_universe.marketplaces.keys().cloned().collect::<HashSet<_>>();

        let in_memory_client = InMemoryUniverseClient::new_with_overrides(
            in_memory_universe,
            InMemoryUniverseOverrides {
                always_respond_with_detailed_marketplace_data: true,
            },
        );

        let agent = in_memory_client.get_agent().await.expect("agent").data;
        let hq_system_symbol = agent.headquarters.system_symbol();

        let ship_bmc = InMemoryShipsBmc::new(InMemoryShips::new());
        let agent_bmc = InMemoryAgentBmc::new(agent);
        let trade_bmc = InMemoryTradeBmc::new();
        let fleet_bmc = InMemoryFleetBmc::new();
        let system_bmc = InMemorySystemsBmc::new();
        let construction_bmc = InMemoryConstructionBmc::new();

        //insert some data
        //construction_bmc.save_construction_site(&Ctx::Anonymous, in_memory_client.get_construction_site().unwrap())

        let market_bmc = InMemoryMarketBmc::new();
        let shipyard_bmc = InMemoryShipyardBmc::new();
        let jump_gate_bmc = InMemoryJumpGateBmc::new();
        let supply_chain_bmc = InMemorySupplyChainBmc::new();
        let status_bmc = InMemoryStatusBmc::new();

        let trade_bmc = Arc::new(trade_bmc);
        let market_bmc = Arc::new(market_bmc);
        let bmc = InMemoryBmc {
            in_mem_ship_bmc: Arc::new(ship_bmc),
            in_mem_fleet_bmc: Arc::new(fleet_bmc),
            in_mem_trade_bmc: Arc::clone(&trade_bmc),
            in_mem_system_bmc: Arc::new(system_bmc),
            in_mem_agent_bmc: Arc::new(agent_bmc),
            in_mem_construction_bmc: Arc::new(construction_bmc),
            in_mem_market_bmc: Arc::clone(&market_bmc),
            in_mem_jump_gate_bmc: Arc::new(jump_gate_bmc),
            in_mem_shipyard_bmc: Arc::new(shipyard_bmc),
            in_mem_supply_chain_bmc: Arc::new(supply_chain_bmc),
            in_mem_status_bmc: Arc::new(status_bmc),
        };

        let client = Arc::new(in_memory_client) as Arc<dyn StClientTrait>;
        let bmc = Arc::new(bmc) as Arc<dyn Bmc>;
        let blackboard = BmcBlackboard::new(Arc::clone(&bmc));

        // because of the override, we should have detailed market data
        FleetRunner::load_and_store_initial_data_in_bmcs(Arc::clone(&client), Arc::clone(&bmc)).await.expect("FleetRunner::load_and_store_initial_data");

        let market_data = bmc.market_bmc().get_latest_market_data_for_system(&Ctx::Anonymous, &hq_system_symbol).await.expect("market_data");

        // easier to get the supply chain this way, since we need plenty of things for computing it
        let facts = collect_fleet_decision_facts(bmc.clone(), &hq_system_symbol).await?;

        let phase = FleetPhaseName::ConstructJumpGate;

        let active_trades: Vec<(ShipSymbol, (TradeGoodSymbol, WaypointSymbol), (TradeGoodSymbol, WaypointSymbol), u32)> = vec![];

        let supply_chain = bmc.supply_chain_bmc().get_supply_chain(&Ctx::Anonymous).await?;

        calc_trading_decisions(&facts, &phase, &active_trades, &vec![], supply_chain.unwrap(), &market_data);

        let materialized_supply_chain = facts.materialized_supply_chain.unwrap();

        assert!(
            materialized_supply_chain.raw_delivery_routes.is_empty().not(),
            "raw_delivery_routes should not be empty"
        );

        Ok(())
    }
}

fn calc_trading_decisions(
    facts: &FleetDecisionFacts,
    phase: &FleetPhaseName,
    active_trades: &[(ShipSymbol, (TradeGoodSymbol, WaypointSymbol), (TradeGoodSymbol, WaypointSymbol), u32)],
    active_construction_deliveries: &[(ShipSymbol, (TradeGoodSymbol, u32))],
    supply_chain: SupplyChain,
    market_data: &[MarketEntry],
) -> Result<()> {
    let missing_construction_material: HashMap<TradeGoodSymbol, u32> = facts.missing_construction_materials();

    let missing_construction_material: HashMap<TradeGoodSymbol, u32> = missing_construction_material
        .into_iter()
        .map(|(good, amount)| {
            // Calculate how much of this good is already being delivered
            let en_route_amount = active_construction_deliveries
                .iter()
                .filter(|(_, (delivery_good, _))| delivery_good == &good)
                .map(|(_, (_, delivery_amount))| delivery_amount)
                .sum::<u32>();

            // Return the good and the remaining amount needed (if any)
            (good, amount.saturating_sub(en_route_amount))
        })
        // Filter out materials that are fully covered by en-route deliveries
        .filter(|(_, remaining_amount)| *remaining_amount > 0)
        .collect();

    let products_for_sale = market_data.iter().flat_map(|me| me.market_data.exports.iter().map(|tg| tg.symbol.clone())).collect::<HashSet<_>>();

    let all_individual_trade_good_chains = supply_chain.individual_supply_chains;
    let all_construction_materials = facts.all_construction_materials();

    let construction_material_chains: HashMap<TradeGoodSymbol, HashSet<TradeGoodSymbol>> = missing_construction_material
        .keys()
        .filter_map(|construction_material| {
            all_individual_trade_good_chains
                .get(construction_material)
                .map(|(_, all_goods_involved)| (construction_material.clone(), all_goods_involved.clone()))
        })
        .collect();

    let non_conflicting_goods_for_sale: HashSet<TradeGoodSymbol> = products_for_sale
        .iter()
        .filter(|tg| all_construction_materials.contains_key(tg).not())
        .cloned()
        .filter(|trade_symbol| {
            let products_involved = all_individual_trade_good_chains.get(trade_symbol).cloned().unwrap().1;

            let no_conflict_with_construction_chains = construction_material_chains.iter().all(|(construction_material, construction_products_involved)| {
                let intersection = products_involved.intersection(&construction_products_involved).collect_vec();
                intersection.is_empty()
            });

            no_conflict_with_construction_chains
        })
        .collect();

    let conflicting_goods_for_sale = products_for_sale.difference(&non_conflicting_goods_for_sale).collect::<HashSet<_>>();

    println!(
        "Found {} out of {} trade goods for sale that don't conflict with the supply chains of the construction materials:\nnon conflicting goods: {:?}\n    conflicting_goods: {:?}",
        non_conflicting_goods_for_sale.len(),
        products_for_sale.len(),
        non_conflicting_goods_for_sale,
        conflicting_goods_for_sale,
    );

    Ok(())
}

/*
 // SHIP_PLATING bottlenecks FAB_MATS
 // SHIP_PARTS bottlenecks ADVANCED_CIRCUITRY (ELECTRONICS)
 def tradeGoodSymbolsToBoostBasedOnConstructionProgress(constructionMaterialRequired: Set[TradeSymbol]): List[TradeSymbol] = {
   if (constructionMaterialRequired == completeContructionMaterials) {
     List(FAB_MATS, ADVANCED_CIRCUITRY, FUEL, CLOTHING, EQUIPMENT)
   } else if (constructionMaterialRequired == Set(TradeSymbol.ADVANCED_CIRCUITRY)) {
     // FAB_MATS done
     List(ADVANCED_CIRCUITRY, FUEL, CLOTHING, SHIP_PLATING)
   } else if (constructionMaterialRequired == Set(TradeSymbol.FAB_MATS)) {
     // ADVANCED_CIRCUITRY done
     List(FAB_MATS, FUEL, CLOTHING, EQUIPMENT, SHIP_PARTS)
   } else {
     // both done
     List(SHIP_PARTS, SHIP_PLATING, FUEL, CLOTHING)
   }
 }
*/
