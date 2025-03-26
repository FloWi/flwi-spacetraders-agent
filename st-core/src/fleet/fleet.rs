use crate::marketplaces::marketplaces::{find_marketplaces_for_exploration, find_shipyards_for_exploration};
use anyhow::Result;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use log::{log, Level};
use serde::{Deserialize, Serialize};
use st_domain::FleetTask::{
    CollectMarketInfosOnce, ConstructJumpGate, MineOres, ObserveAllWaypointsOfSystemWithStationaryProbes, SiphonGases, TradeProfitably,
};
use st_domain::{
    ConstructJumpGateFleetConfig, Fleet, FleetConfig, FleetDecisionFacts, FleetId, FleetTask, FleetTaskCompletion, FleetsOverview, GetConstructionResponse,
    MarketObservationFleetConfig, MaterializedSupplyChain, MiningFleetConfig, Ship, ShipFrameSymbol, ShipSymbol, ShipTask, ShipType, SiphoningFleetConfig,
    SystemSpawningFleetConfig, SystemSymbol, TradeGoodSymbol, TradingFleetConfig, WaypointSymbol,
};
use st_store::{
    select_latest_marketplace_entry_of_system, select_latest_shipyard_entry_of_system, ConstructionBmc, Ctx, DbModelManager, FleetBmc, ShipBmc, SystemBmc,
};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct FleetAdmiral {
    completed_fleet_tasks: Vec<FleetTaskCompletion>,
    fleets: HashMap<FleetId, Fleet>,
    all_ships: HashMap<ShipSymbol, Ship>,
    ship_fleet_assignment: HashMap<ShipSymbol, FleetId>,
}

impl FleetAdmiral {
    pub async fn run_fleets(&mut self) -> Result<()> {
        log!(Level::Info, "Running fleets");
        Ok(())
    }

    pub fn get_overview(&self) -> FleetsOverview {
        FleetsOverview {
            completed_fleet_tasks: self.completed_fleet_tasks.clone(),
            fleets: self.fleets.clone(),
            all_ships: self.all_ships.clone(),
            ship_fleet_assignment: self.ship_fleet_assignment.clone(),
        }
    }
}

impl FleetAdmiral {
    pub async fn new(mm: &DbModelManager, system_symbol: SystemSymbol) -> Result<Self> {
        let ships = ShipBmc::get_ships(&Ctx::Anonymous, mm, None).await?;
        let ship_map: HashMap<ShipSymbol, Ship> = ships.into_iter().map(|s| (s.symbol.clone(), s)).collect();
        let ship_type_map: HashMap<ShipSymbol, ShipType> = {
            let mapping = role_to_ship_type_mapping();
            ship_map
                .iter()
                .map(|(ship_symbol, ship)| {
                    let frame_type = ship_map.get(&ship_symbol).unwrap().frame.symbol.clone();
                    let ship_type = mapping.get(&frame_type).unwrap().clone();
                    (ship_symbol.clone(), ship_type)
                })
                .collect()
        };

        let completed_tasks = FleetBmc::load_completed_fleet_tasks(&Ctx::Anonymous, mm).await?;
        let facts = collect_fleet_decision_facts(mm, &system_symbol).await?;
        let fleet_tasks = compute_fleet_tasks(system_symbol, &facts, &completed_tasks);
        let fleet_configs = compute_fleet_configs(&fleet_tasks, &facts);
        let fleets: Vec<Fleet> = fleet_configs
            .into_iter()
            .enumerate()
            .map(|(idx, (cfg, task))| Self::create_fleet(cfg.clone(), task.clone(), (completed_tasks.len() + idx) as i32).unwrap())
            .collect_vec();

        let ship_fleet_assignment = Self::assign_ships(&fleets, &ship_map);

        let mut fleet_map: HashMap<FleetId, Fleet> = fleets.into_iter().map(|f| (f.id.clone(), f)).collect();

        for (ship_symbol, fleet_id) in ship_fleet_assignment.iter() {
            let ship_type = ship_type_map.get(&ship_symbol).unwrap().clone();
            fleet_map.entry(fleet_id.clone()).and_modify(|fleet| {
                fleet.ships.insert(ship_symbol.clone(), ship_type);
            });
        }

        let mut admiral = Self {
            completed_fleet_tasks: completed_tasks,
            fleets: fleet_map,
            all_ships: ship_map,
            ship_fleet_assignment,
        };

        Ok(admiral)
    }

    pub fn create_fleet(super_fleet_config: FleetConfig, fleet_task: FleetTask, id: i32) -> Result<Fleet> {
        let id = FleetId(id);
        let mut fleet = Fleet {
            id: id.clone(),
            cfg: super_fleet_config,
            tasks: vec![fleet_task],
            ship_tasks: Default::default(),
            ships: Default::default(),
        };

        Ok(fleet)
    }

    pub fn assign_ships(fleets: &[Fleet], all_ships: &HashMap<ShipSymbol, Ship>) -> HashMap<ShipSymbol, FleetId> {
        fleets
            .iter()
            .flat_map(|fleet| {
                let desired_fleet_config = match fleet.cfg.clone() {
                    FleetConfig::SystemSpawningCfg(cfg) => cfg.desired_fleet_config,
                    FleetConfig::MarketObservationCfg(cfg) => cfg.desired_fleet_config,
                    FleetConfig::TradingCfg(cfg) => cfg.desired_fleet_config,
                    FleetConfig::ConstructJumpGateCfg(cfg) => cfg.desired_fleet_config,
                    FleetConfig::MiningCfg(cfg) => cfg.desired_fleet_config,
                    FleetConfig::SiphoningCfg(cfg) => cfg.desired_fleet_config,
                };
                let mut available_ships = all_ships.clone();
                let assigned_ships_for_fleet = assign_matching_ships(&desired_fleet_config, &mut available_ships);
                assigned_ships_for_fleet.into_iter().map(|(sym, _)| (sym, fleet.id.clone()))
            })
            .collect::<HashMap<_, _>>()
    }
}

pub fn compute_fleet_configs(tasks: &[FleetTask], fleet_decision_facts: &FleetDecisionFacts) -> Vec<(FleetConfig, FleetTask)> {
    let all_waypoints_of_interest =
        fleet_decision_facts.marketplaces_of_interest.iter().chain(fleet_decision_facts.shipyards_of_interest.iter()).unique().collect_vec();

    tasks
        .into_iter()
        .filter_map(|t| {
            let maybe_cfg = match t {
                FleetTask::CollectMarketInfosOnce { system_symbol } => Some(FleetConfig::SystemSpawningCfg(SystemSpawningFleetConfig {
                    system_symbol: system_symbol.clone(),
                    marketplace_waypoints_of_interest: fleet_decision_facts.marketplaces_of_interest.clone(),
                    shipyard_waypoints_of_interest: fleet_decision_facts.shipyards_of_interest.clone(),
                    desired_fleet_config: vec![(ShipType::SHIP_COMMAND_FRIGATE, 1)],
                })),
                FleetTask::ObserveAllWaypointsOfSystemWithStationaryProbes { system_symbol } => {
                    Some(FleetConfig::MarketObservationCfg(MarketObservationFleetConfig {
                        system_symbol: system_symbol.clone(),
                        marketplace_waypoints_of_interest: fleet_decision_facts.marketplaces_of_interest.clone(),
                        shipyard_waypoints_of_interest: fleet_decision_facts.shipyards_of_interest.clone(),
                        desired_fleet_config: vec![(ShipType::SHIP_PROBE, all_waypoints_of_interest.len() as u32)],
                    }))
                }
                FleetTask::ConstructJumpGate { system_symbol } => Some(FleetConfig::ConstructJumpGateCfg(ConstructJumpGateFleetConfig {
                    system_symbol: system_symbol.clone(),
                    jump_gate_waypoint: WaypointSymbol(fleet_decision_facts.construction_site.clone().expect("construction_site").symbol),
                    materialized_supply_chain: None,
                    desired_fleet_config: vec![(ShipType::SHIP_COMMAND_FRIGATE, 1), (ShipType::SHIP_LIGHT_HAULER, 4)],
                })),
                FleetTask::TradeProfitably { system_symbol } => Some(FleetConfig::TradingCfg(TradingFleetConfig {
                    system_symbol: system_symbol.clone(),
                    materialized_supply_chain: None,
                    desired_fleet_config: vec![(ShipType::SHIP_COMMAND_FRIGATE, 1), (ShipType::SHIP_LIGHT_HAULER, 4)],
                })),
                FleetTask::MineOres { system_symbol } => Some(FleetConfig::MiningCfg(MiningFleetConfig {
                    system_symbol: system_symbol.clone(),
                    mining_waypoint: WaypointSymbol("TODO add engineered asteroid".to_string()),
                    materials: vec![],
                    delivery_locations: Default::default(),
                    materialized_supply_chain: None,
                    desired_fleet_config: vec![
                        (ShipType::SHIP_MINING_DRONE, 7),
                        (ShipType::SHIP_SURVEYOR, 2),
                        (ShipType::SHIP_LIGHT_HAULER, 2),
                    ],
                })),
                FleetTask::SiphonGases { system_symbol } => Some(FleetConfig::SiphoningCfg(SiphoningFleetConfig {
                    system_symbol: system_symbol.clone(),
                    siphoning_waypoint: WaypointSymbol("TODO add gas giant".to_string()),
                    materials: vec![],
                    delivery_locations: Default::default(),
                    materialized_supply_chain: None,
                    desired_fleet_config: vec![(ShipType::SHIP_SIPHON_DRONE, 5)],
                })),
            };
            maybe_cfg.map(|cfg| (cfg, t.clone()))
        })
        .collect_vec()
}

pub fn compute_fleet_tasks(system_symbol: SystemSymbol, fleet_decision_facts: &FleetDecisionFacts, completed_tasks: &[FleetTaskCompletion]) -> Vec<FleetTask> {
    use FleetTask::*;

    // three phases
    // 1. gather initial infos about system
    // 2. construct jump gate
    //    - trade profitably and deliver construction material with hauler fleet
    //    - mine ores with mining fleet
    //    - siphon gases with siphoning fleet
    // 3. trade profitably
    //    - trade profitably with hauler fleet
    //    - prob. stop mining and siphoning

    let has_construct_jump_gate_task_been_completed = completed_tasks.iter().any(|t| matches!(&t.task, ConstructJumpGate { system_symbol }));
    let has_collect_market_infos_once_task_been_completed = completed_tasks.iter().any(|t| matches!(&t.task, CollectMarketInfosOnce { system_symbol }));

    let is_jump_gate_done =
        fleet_decision_facts.construction_site.clone().map(|cs| cs.is_complete).unwrap_or(false) || has_construct_jump_gate_task_been_completed;
    let is_shipyard_exploration_complete = are_vecs_equal_ignoring_order(
        &fleet_decision_facts.shipyards_of_interest,
        &fleet_decision_facts.shipyards_with_up_to_date_infos,
    );
    let is_marketplace_exploration_complete = are_vecs_equal_ignoring_order(
        &fleet_decision_facts.marketplaces_of_interest,
        &fleet_decision_facts.marketplaces_with_up_to_date_infos,
    );
    let has_collected_all_waypoint_details_once =
        is_shipyard_exploration_complete && is_marketplace_exploration_complete || has_collect_market_infos_once_task_been_completed;

    let tasks = if !has_collected_all_waypoint_details_once {
        vec![
            CollectMarketInfosOnce {
                system_symbol: system_symbol.clone(),
            },
            ObserveAllWaypointsOfSystemWithStationaryProbes {
                system_symbol: system_symbol.clone(),
            },
        ]
    } else if !is_jump_gate_done {
        vec![
            ConstructJumpGate {
                system_symbol: system_symbol.clone(),
            },
            ObserveAllWaypointsOfSystemWithStationaryProbes {
                system_symbol: system_symbol.clone(),
            },
            MineOres {
                system_symbol: system_symbol.clone(),
            },
            SiphonGases {
                system_symbol: system_symbol.clone(),
            },
        ]
    } else if is_jump_gate_done {
        vec![
            TradeProfitably {
                system_symbol: system_symbol.clone(),
            },
            ObserveAllWaypointsOfSystemWithStationaryProbes {
                system_symbol: system_symbol.clone(),
            },
        ]
    } else {
        unimplemented!("this shouldn't happen - think harder")
    };

    tasks
}

fn role_to_ship_type_mapping() -> HashMap<ShipFrameSymbol, ShipType> {
    HashMap::from([
        (ShipFrameSymbol::FRAME_FRIGATE, ShipType::SHIP_COMMAND_FRIGATE),
        (ShipFrameSymbol::FRAME_PROBE, ShipType::SHIP_PROBE),
    ])
}

fn assign_matching_ships(desired_fleet_config: &[(ShipType, u32)], available_ships: &mut HashMap<ShipSymbol, Ship>) -> HashMap<ShipSymbol, Ship> {
    let mapping: HashMap<ShipFrameSymbol, ShipType> = role_to_ship_type_mapping();

    let mut assigned_ships: Vec<Ship> = vec![];

    for (ship_type, amount) in desired_fleet_config.iter() {
        let assignable_ships = available_ships
            .iter()
            .filter_map(|(_, s)| {
                let current_ship_type = mapping.get(&s.frame.symbol).expect("role_to_ship_type_mapping");
                (current_ship_type == ship_type).then_some((s.symbol.clone(), current_ship_type.clone(), s.clone()))
            })
            .take(*amount as usize)
            .collect_vec();

        for (assigned_symbol, _, ship) in assignable_ships {
            assigned_ships.push(ship);
            available_ships.remove(&assigned_symbol);
        }
    }
    let ships: HashMap<ShipSymbol, Ship> = assigned_ships.into_iter().map(|ship| (ship.symbol.clone(), ship)).collect();

    ships
}

pub async fn collect_fleet_decision_facts(mm: &DbModelManager, system_symbol: &SystemSymbol) -> Result<FleetDecisionFacts> {
    let ships = ShipBmc::get_ships(&Ctx::Anonymous, mm, None).await?;
    let waypoints_of_system = SystemBmc::get_waypoints_of_system(&Ctx::Anonymous, mm, &system_symbol).await?;

    let marketplaces_of_interest = select_latest_marketplace_entry_of_system(mm.pool(), &system_symbol).await?;
    let marketplace_symbols_of_interest = marketplaces_of_interest.iter().map(|db_entry| WaypointSymbol(db_entry.waypoint_symbol.clone())).collect_vec();
    let marketplaces_to_explore = find_marketplaces_for_exploration(marketplaces_of_interest.clone());

    let shipyards_of_interest = select_latest_shipyard_entry_of_system(mm.pool(), &system_symbol).await?;
    let shipyard_symbols_of_interest = shipyards_of_interest.iter().map(|db_entry| WaypointSymbol(db_entry.waypoint_symbol.clone())).collect_vec();
    let shipyards_to_explore = find_shipyards_for_exploration(shipyards_of_interest.clone());

    let maybe_construction_site: Option<GetConstructionResponse> =
        ConstructionBmc::get_construction_site_for_system(&Ctx::Anonymous, mm, system_symbol.clone()).await?;

    Ok(FleetDecisionFacts {
        marketplaces_of_interest: marketplace_symbols_of_interest.clone(),
        marketplaces_with_up_to_date_infos: diff_waypoint_symbols(&marketplace_symbols_of_interest, &marketplaces_to_explore),
        shipyards_of_interest: shipyard_symbols_of_interest.clone(),
        shipyards_with_up_to_date_infos: diff_waypoint_symbols(&shipyard_symbols_of_interest, &shipyards_to_explore),
        construction_site: maybe_construction_site.map(|resp| resp.data),
        ships,
        materialized_supply_chain: None,
    })
}
pub fn diff_waypoint_symbols(waypoints_of_interest: &[WaypointSymbol], waypoints_to_explore: &[WaypointSymbol]) -> Vec<WaypointSymbol> {
    let set2: HashSet<_> = waypoints_to_explore.iter().collect();

    waypoints_of_interest.iter().filter(|item| !set2.contains(item)).cloned().collect()
}

pub fn are_vecs_equal_ignoring_order<T: Eq + Hash>(vec1: &[T], vec2: &[T]) -> bool {
    // Quick check - if lengths differ, they can't be equal
    if vec1.len() != vec2.len() {
        return false;
    }

    // Convert to HashSets and compare
    let set1: HashSet<_> = vec1.iter().collect();
    let set2: HashSet<_> = vec2.iter().collect();

    set1 == set2
}
