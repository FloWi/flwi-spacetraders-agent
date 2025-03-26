use crate::fleet::{are_vecs_equal_ignoring_order, collect_fleet_decision_facts};
use anyhow::Result;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use st_domain::{FleetDecisionFacts, MaterializedSupplyChain, Ship, ShipSymbol, ShipTask, ShipType, SystemSymbol, TradeGoodSymbol, WaypointSymbol};
use st_store::DbModelManager;
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SystemSpawningFleetConfig {
    pub system_symbol: SystemSymbol,
    pub marketplace_waypoints_of_interest: Vec<WaypointSymbol>,
    pub shipyard_waypoints_of_interest: Vec<WaypointSymbol>,
    pub desired_fleet_config: Vec<(ShipType, u32)>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MarketObservationFleetConfig {
    pub system_symbol: SystemSymbol,
    pub marketplace_waypoints_of_interest: Vec<WaypointSymbol>,
    pub shipyard_waypoints_of_interest: Vec<WaypointSymbol>,
    pub desired_fleet_config: Vec<(ShipType, u32)>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TradingFleetConfig {
    pub system_symbol: SystemSymbol,
    pub materialized_supply_chain: Option<MaterializedSupplyChain>,
    pub desired_fleet_config: Vec<(ShipType, u32)>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ConstructJumpGateFleetConfig {
    pub system_symbol: SystemSymbol,
    pub jump_gate_waypoint: WaypointSymbol,
    pub materialized_supply_chain: Option<MaterializedSupplyChain>,
    pub desired_fleet_config: Vec<(ShipType, u32)>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MiningFleetConfig {
    pub system_symbol: SystemSymbol,
    pub mining_waypoint: WaypointSymbol,
    pub materials: Vec<TradeGoodSymbol>,
    pub delivery_locations: HashMap<TradeGoodSymbol, WaypointSymbol>,
    pub materialized_supply_chain: Option<MaterializedSupplyChain>,
    pub desired_fleet_config: Vec<(ShipType, u32)>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SiphoningFleetConfig {
    pub system_symbol: SystemSymbol,
    pub siphoning_waypoint: WaypointSymbol,
    pub materials: Vec<TradeGoodSymbol>,
    pub delivery_locations: HashMap<TradeGoodSymbol, WaypointSymbol>,
    pub materialized_supply_chain: Option<MaterializedSupplyChain>,
    pub desired_fleet_config: Vec<(ShipType, u32)>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum SuperFleetConfig {
    SystemSpawningCfg(SystemSpawningFleetConfig),
    MarketObservationCfg(MarketObservationFleetConfig),
    TradingCfg(TradingFleetConfig),
    ConstructJumpGateCfg(ConstructJumpGateFleetConfig),
    MiningCfg(MiningFleetConfig),
    SiphoningCfg(SiphoningFleetConfig),
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct SuperFleetId(i32);

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct SuperFleetTaskCompletion {
    pub task: SuperFleetTask,
    pub completed_at: DateTime<Utc>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum SuperFleetTask {
    CollectMarketInfosOnce { system_symbol: SystemSymbol },
    ObserveAllWaypointsOfSystemWithStationaryProbes { system_symbol: SystemSymbol },
    ConstructJumpGate { system_symbol: SystemSymbol },
    TradeProfitably { system_symbol: SystemSymbol },
    MineOres { system_symbol: SystemSymbol },
    SiphonGases { system_symbol: SystemSymbol },
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct SuperFleetAdmiral {
    completed_fleet_tasks: Vec<SuperFleetTaskCompletion>,
    fleets: HashMap<SuperFleetId, SuperFleet>,
    all_ships: HashMap<ShipSymbol, Ship>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct SuperFleet {
    id: SuperFleetId,
    cfg: SuperFleetConfig,
    tasks: Vec<SuperFleetTask>,
    ship_tasks: HashMap<ShipSymbol, ShipTask>,
    ships: HashMap<ShipSymbol, ShipType>,
}

impl SuperFleetAdmiral {
    pub async fn new(mm: &DbModelManager, system_symbol: SystemSymbol) -> Result<Self> {
        let completed_tasks = Default::default(); // TODO - refactor this to return correct type FleetBmc::load_completed_fleet_tasks(&Ctx::Anonymous, &mm).await?;
        let facts = collect_fleet_decision_facts(mm, &system_symbol).await?;
        let fleet_tasks = compute_fleet_tasks(system_symbol, &facts, completed_tasks);
        let fleet_configs = compute_fleet_configs(&fleet_tasks, &facts);
        let fleets: Vec<SuperFleet> = fleet_configs
            .into_iter()
            .enumerate()
            .map(|(idx, (cfg, task))| Self::create_fleet(cfg.clone(), task.clone(), (completed_tasks.len() + idx) as i32).unwrap())
            .collect_vec();

        Ok(Self {
            completed_fleet_tasks: completed_tasks.into_iter().cloned().collect(),
            fleets: fleets.into_iter().map(|f| (f.id.clone(), f)).collect(),
            all_ships: Default::default(),
        })
    }

    pub fn create_fleet(super_fleet_config: SuperFleetConfig, fleet_task: SuperFleetTask, id: i32) -> Result<SuperFleet> {
        let id = SuperFleetId(id);
        let mut fleet = SuperFleet {
            id: id.clone(),
            cfg: super_fleet_config,
            tasks: vec![fleet_task],
            ship_tasks: Default::default(),
            ships: Default::default(),
        };

        Ok(fleet)
    }
}

pub fn compute_fleet_configs(tasks: &[SuperFleetTask], fleet_decision_facts: &FleetDecisionFacts) -> Vec<(SuperFleetConfig, SuperFleetTask)> {
    let all_waypoints_of_interest =
        fleet_decision_facts.marketplaces_of_interest.iter().chain(fleet_decision_facts.shipyards_of_interest.iter()).unique().collect_vec();

    tasks
        .into_iter()
        .filter_map(|t| {
            let maybe_cfg = match t {
                SuperFleetTask::CollectMarketInfosOnce { system_symbol } => Some(SuperFleetConfig::SystemSpawningCfg(SystemSpawningFleetConfig {
                    system_symbol: system_symbol.clone(),
                    marketplace_waypoints_of_interest: fleet_decision_facts.marketplaces_of_interest.clone(),
                    shipyard_waypoints_of_interest: fleet_decision_facts.shipyards_of_interest.clone(),
                    desired_fleet_config: vec![(ShipType::SHIP_COMMAND_FRIGATE, 1)],
                })),
                SuperFleetTask::ObserveAllWaypointsOfSystemWithStationaryProbes { system_symbol } => {
                    Some(SuperFleetConfig::MarketObservationCfg(MarketObservationFleetConfig {
                        system_symbol: system_symbol.clone(),
                        marketplace_waypoints_of_interest: fleet_decision_facts.marketplaces_of_interest.clone(),
                        shipyard_waypoints_of_interest: fleet_decision_facts.shipyards_of_interest.clone(),
                        desired_fleet_config: vec![(ShipType::SHIP_PROBE, all_waypoints_of_interest.len() as u32)],
                    }))
                }
                SuperFleetTask::ConstructJumpGate { system_symbol } => Some(SuperFleetConfig::ConstructJumpGateCfg(ConstructJumpGateFleetConfig {
                    system_symbol: system_symbol.clone(),
                    jump_gate_waypoint: WaypointSymbol(fleet_decision_facts.construction_site.clone().expect("construction_site").symbol),
                    materialized_supply_chain: None,
                    desired_fleet_config: vec![(ShipType::SHIP_COMMAND_FRIGATE, 1), (ShipType::SHIP_LIGHT_HAULER, 4)],
                })),
                SuperFleetTask::TradeProfitably { system_symbol } => Some(SuperFleetConfig::TradingCfg(TradingFleetConfig {
                    system_symbol: system_symbol.clone(),
                    materialized_supply_chain: None,
                    desired_fleet_config: vec![(ShipType::SHIP_COMMAND_FRIGATE, 1), (ShipType::SHIP_LIGHT_HAULER, 4)],
                })),
                SuperFleetTask::MineOres { system_symbol } => Some(SuperFleetConfig::MiningCfg(MiningFleetConfig {
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
                SuperFleetTask::SiphonGases { system_symbol } => Some(SuperFleetConfig::SiphoningCfg(SiphoningFleetConfig {
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

pub fn compute_fleet_tasks(
    system_symbol: SystemSymbol,
    fleet_decision_facts: &FleetDecisionFacts,
    completed_tasks: &[SuperFleetTaskCompletion],
) -> Vec<SuperFleetTask> {
    use SuperFleetTask::*;

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
