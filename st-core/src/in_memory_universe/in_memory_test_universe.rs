use crate::st_client::StClientTrait;
use crate::universe_server::universe_server::{InMemoryUniverse, InMemoryUniverseClient};
use st_store::bmc::jump_gate_bmc::InMemoryJumpGateBmc;
use st_store::bmc::ship_bmc::{InMemoryShips, InMemoryShipsBmc};
use st_store::bmc::{Bmc, InMemoryBmc};
use st_store::shipyard_bmc::InMemoryShipyardBmc;
use st_store::survey_bmc::InMemorySurveyBmc;
use st_store::trade_bmc::InMemoryTradeBmc;
use st_store::{InMemoryAgentBmc, InMemoryConstructionBmc, InMemoryFleetBmc, InMemoryMarketBmc, InMemoryStatusBmc, InMemorySupplyChainBmc, InMemorySystemsBmc};
use std::collections::HashSet;
use std::sync::Arc;

pub async fn get_test_universe() -> (Arc<dyn Bmc>, Arc<dyn StClientTrait>) {
    // Get the path to the Cargo.toml directory
    let manifest_dir = env!("CARGO_MANIFEST_DIR");

    // Construct path to the shared JSON file
    let json_path = std::path::Path::new(manifest_dir)
        .parent()
        .unwrap()
        .join("resources")
        .join("universe_snapshot.json");

    let in_memory_universe = InMemoryUniverse::from_snapshot(json_path).expect("InMemoryUniverse::from_snapshot");

    let shipyard_waypoints = in_memory_universe
        .shipyards
        .keys()
        .cloned()
        .collect::<HashSet<_>>();
    let marketplace_waypoints = in_memory_universe
        .marketplaces
        .keys()
        .cloned()
        .collect::<HashSet<_>>();

    let in_memory_client = InMemoryUniverseClient::new(in_memory_universe);

    let agent = in_memory_client.get_agent().await.expect("agent").data;
    let hq_system_symbol = agent.headquarters.system_symbol();

    let ship_bmc = InMemoryShipsBmc::new(InMemoryShips::new());
    let agent_bmc = InMemoryAgentBmc::new(agent);
    let trade_bmc = InMemoryTradeBmc::new();
    let fleet_bmc = InMemoryFleetBmc::new();
    let system_bmc = InMemorySystemsBmc::new();
    let construction_bmc = InMemoryConstructionBmc::new();
    let survey_bmc = InMemorySurveyBmc::new();

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
        in_mem_survey_bmc: Arc::new(survey_bmc),
        in_mem_market_bmc: Arc::clone(&market_bmc),
        in_mem_jump_gate_bmc: Arc::new(jump_gate_bmc),
        in_mem_shipyard_bmc: Arc::new(shipyard_bmc),
        in_mem_supply_chain_bmc: Arc::new(supply_chain_bmc),
        in_mem_status_bmc: Arc::new(status_bmc),
    };

    let client = Arc::new(in_memory_client) as Arc<dyn StClientTrait>;
    let bmc = Arc::new(bmc) as Arc<dyn Bmc>;

    (bmc, client)
}
