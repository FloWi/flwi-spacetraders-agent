use crate::universe_server::universe_server::InMemoryUniverse;
use serde::{Deserialize, Serialize};
use st_domain::{Agent, Construction, GetSupplyChainResponse, JumpGate, MarketData, Ship, Shipyard, SupplyChain, SystemsPageData, Waypoint};

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct UniverseSnapshot {
    systems: Vec<SystemsPageData>,
    waypoints: Vec<Waypoint>,
    ships: Vec<Ship>,
    marketplaces: Vec<MarketData>,
    shipyards: Vec<Shipyard>,
    construction_sites: Vec<Construction>,
    agent: Agent,
    jump_gates: Vec<JumpGate>,
    supply_chain: GetSupplyChainResponse,
}

impl UniverseSnapshot {
    /// Load a universe snapshot from a JSON file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let snapshot: UniverseSnapshot = serde_json::from_reader(reader)?;
        Ok(snapshot)
    }

    /// Convert the snapshot into an InMemoryUniverse
    pub fn into_memory_universe(self) -> InMemoryUniverse {
        // Create hashmaps from the vectors
        let systems = self
            .systems
            .into_iter()
            .map(|system| {
                // Assuming SystemsPageData has a field that can be used to get the symbol
                let symbol = system.symbol.clone();
                (symbol, system)
            })
            .collect();

        let waypoints = self
            .waypoints
            .into_iter()
            .map(|waypoint| {
                let symbol = waypoint.symbol.clone();
                (symbol, waypoint)
            })
            .collect();

        let ships = self
            .ships
            .into_iter()
            .map(|ship| {
                let symbol = ship.symbol.clone();
                (symbol, ship)
            })
            .collect();

        let marketplaces = self
            .marketplaces
            .into_iter()
            .map(|market| {
                // Assuming MarketEntry has a waypoint field or similar
                let symbol = market.symbol.clone();
                (symbol, market)
            })
            .collect();

        let shipyards = self
            .shipyards
            .into_iter()
            .map(|shipyard| {
                let symbol = shipyard.symbol.clone();
                (symbol, shipyard)
            })
            .collect();

        let construction_sites = self
            .construction_sites
            .into_iter()
            .map(|construction| (construction.symbol.clone(), construction))
            .collect();

        let agent = self.agent;

        let jump_gates = self
            .jump_gates
            .into_iter()
            .map(|jg| (jg.symbol.clone(), jg.clone()))
            .collect();

        InMemoryUniverse {
            systems,
            waypoints,
            ships,
            marketplaces,
            shipyards,
            construction_sites,
            agent,
            transactions: vec![],
            jump_gates,

            supply_chain: self.supply_chain.clone().into(),
        }
    }
}

// Function to directly load an InMemoryUniverse from a JSON file
pub fn load_universe<P: AsRef<Path>>(path: P) -> Result<InMemoryUniverse, Box<dyn std::error::Error>> {
    let snapshot = UniverseSnapshot::from_file(path)?;
    Ok(snapshot.into_memory_universe())
}

/*
with headquarters as (select regexp_replace(entry ->> 'headquarters', '-[^-]*$', '') as headquarters_system_symbol
                           , entry ->> 'headquarters'                                as headquarters_waypoint_symbol
                      from agent)
   , hq_system as (select system_symbol, entry, created_at, updated_at
                   from systems
                   where system_symbol = (select headquarters_system_symbol from headquarters))
   , hq_waypoints as (select system_symbol, waypoint_symbol, entry, created_at, updated_at
                      from waypoints
                      where system_symbol = (select headquarters_system_symbol from headquarters))
   , hq_markets as (select distinct on (waypoint_symbol) waypoint_symbol, entry, created_at
                    from markets
                    where regexp_replace(waypoint_symbol, '-[^-]*$', '') = (select headquarters_system_symbol from headquarters)
                    order by waypoint_symbol, created_at desc)
   , hq_shipyards as (select distinct on (waypoint_symbol) system_symbol, waypoint_symbol, entry, created_at
                      from shipyards
                      where regexp_replace(waypoint_symbol, '-[^-]*$', '') = (select headquarters_system_symbol from headquarters)
                      order by waypoint_symbol, created_at desc)
   , hq_construction_site as (select distinct on (waypoint_symbol) waypoint_symbol, entry, updated_at
                              from construction_sites
                              where regexp_replace(waypoint_symbol, '-[^-]*$', '') = (select headquarters_system_symbol from headquarters)
                              order by waypoint_symbol, updated_at desc)
   , hq_jump_gate as (select system_symbol, waypoint_symbol, entry, created_at, updated_at
                      from jump_gates)
   , ships as (select *
               from ships
               where ship_symbol like '%-1'
                  or ship_symbol like '%-2')
SELECT jsonb_build_object(
               'systems', (SELECT COALESCE(jsonb_agg(entry), '[]'::jsonb)
                           FROM hq_system),
               'waypoints', (SELECT COALESCE(jsonb_agg(entry), '[]'::jsonb)
                             FROM hq_waypoints),
               'marketplaces', (SELECT COALESCE(jsonb_agg(entry), '[]'::jsonb)
                                FROM hq_markets),
               'shipyards', (SELECT COALESCE(jsonb_agg(entry), '[]'::jsonb)
                             FROM hq_shipyards),
               'ships', (SELECT COALESCE(jsonb_agg(entry), '[]'::jsonb)
                         FROM ships),
               'construction_sites', (SELECT COALESCE(jsonb_agg(entry -> 'data'), '[]'::jsonb)
                                      FROM hq_construction_site),
               'jump_gates', (SELECT COALESCE(jsonb_agg(entry), '[]'::jsonb)
                              FROM hq_jump_gate),
               'agent', (select entry from agent limit 1)
       ) AS aggregated_data;


 */
