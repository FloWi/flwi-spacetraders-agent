#[cfg(test)]
mod tests {
    use crate::bmc_blackboard::BmcBlackboard;
    use crate::fleet::fleet::collect_fleet_decision_facts;
    use crate::fleet::fleet_runner::FleetRunner;
    use crate::st_client::StClientTrait;
    use crate::universe_server::universe_server::{InMemoryUniverse, InMemoryUniverseClient, InMemoryUniverseOverrides};
    use anyhow::Result;
    use chrono::Utc;
    use itertools::Itertools;
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

        // easier to get the supply chain this way, since we need plenty of things for computing it
        let facts = collect_fleet_decision_facts(bmc, &hq_system_symbol).await?;

        let materialized_supply_chain = facts.materialized_supply_chain.unwrap();

        assert!(
            materialized_supply_chain.raw_delivery_routes.is_empty().not(),
            "raw_delivery_routes should not be empty"
        );

        Ok(())
    }
}
