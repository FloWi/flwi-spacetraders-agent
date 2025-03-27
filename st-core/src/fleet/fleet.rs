use crate::behavior_tree::behavior_args::{BehaviorArgs, DbBlackboard};
use crate::behavior_tree::behavior_tree::{ActionEvent, Actionable, Behavior, Response};
use crate::behavior_tree::ship_behaviors::{ship_behaviors, ShipAction};
use crate::fleet::market_observation_fleet::MarketObservationFleet;
use crate::fleet::system_spawning_fleet::SystemSpawningFleet;
use crate::marketplaces::marketplaces::{find_marketplaces_for_exploration, find_shipyards_for_exploration};
use crate::ship::ShipOperations;
use crate::st_client::{StClient, StClientTrait};
use anyhow::{anyhow, Error, Result};
use chrono::{DateTime, Utc};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use sqlx::__rt::JoinHandle;
use sqlx::{Pool, Postgres};
use st_domain::FleetConfig::SystemSpawningCfg;
use st_domain::FleetTask::{
    CollectMarketInfosOnce, ConstructJumpGate, MineOres, ObserveAllWaypointsOfSystemWithStationaryProbes, SiphonGases, TradeProfitably,
};
use st_domain::{
    ConstructJumpGateFleetConfig, Fleet, FleetConfig, FleetDecisionFacts, FleetId, FleetTask, FleetTaskCompletion, FleetsOverview, GetConstructionResponse,
    MarketObservationFleetConfig, MaterializedSupplyChain, MiningFleetConfig, Ship, ShipFrameSymbol, ShipSymbol, ShipTask, ShipType, SiphoningFleetConfig,
    SystemSpawningFleetConfig, SystemSymbol, TradeGoodSymbol, TradingFleetConfig, WaypointSymbol,
};
use st_store::{
    db, select_latest_marketplace_entry_of_system, select_latest_shipyard_entry_of_system, ConstructionBmc, Ctx, DbModelManager, FleetBmc, ShipBmc, SystemBmc,
};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::Mutex;
use tracing::{event, span, Level};

struct FleetRunner {
    ship_fibers: HashMap<ShipSymbol, tokio::task::JoinHandle<Result<()>>>,
    ship_ops: HashMap<ShipSymbol, Arc<Mutex<ShipOperations>>>,
    ship_updated_tx: Sender<ShipOperations>,
    ship_updated_listener_join_handle: tokio::task::JoinHandle<Result<()>>,
    ship_action_completed_tx: Sender<ActionEvent>,
    ship_action_completed_rx: Receiver<ActionEvent>,
}

impl FleetRunner {
    pub async fn run_fleets(fleet_admiral: &mut FleetAdmiral, client: Arc<dyn StClientTrait>, db_model_manager: &DbModelManager) -> Result<()> {
        event!(Level::INFO, "Running fleets");

        // Create Arc<Mutex<>> wrappers around each ShipOperations to allow shared ownership
        let ship_ops: HashMap<ShipSymbol, Arc<Mutex<ShipOperations>>> = fleet_admiral
            .all_ships
            .iter()
            .map(|(_, s)| (s.symbol.clone(), Arc::new(Mutex::new(ShipOperations::new(s.clone(), Arc::clone(&client))))))
            .collect();

        let args = BehaviorArgs {
            blackboard: Arc::new(DbBlackboard {
                model_manager: db_model_manager.clone(),
            }),
        };

        let (ship_updated_tx, ship_updated_rx): (Sender<ShipOperations>, Receiver<ShipOperations>) = tokio::sync::mpsc::channel(32);
        let (ship_action_completed_tx, ship_action_completed_rx): (Sender<ActionEvent>, Receiver<ActionEvent>) = tokio::sync::mpsc::channel(32);

        // Clone fleet_admiral.ship_tasks to avoid the lifetime issues
        let ship_tasks = fleet_admiral.ship_tasks.clone();

        let ship_updated_listener_join_handle = tokio::spawn(Self::listen_to_ship_changes_and_persist(ship_updated_rx, db_model_manager.clone()));
        let ship_updated_listener_join_handle = tokio::spawn(Self::listen_to_ship_action_update_messages(ship_action_completed_rx, db_model_manager.clone()));

        let mut ship_fibers: HashMap<ShipSymbol, tokio::task::JoinHandle<Result<()>>> = HashMap::new();

        // Populate ship_fibers with spawned tasks
        for (ship_symbol, ship_op_mutex) in &ship_ops {
            let maybe_ship_task = ship_tasks.get(ship_symbol);

            if let Some(ship_task) = maybe_ship_task {
                // Clone all the values that need to be moved into the async task
                let ship_op_clone = Arc::clone(ship_op_mutex);
                let args_clone = args.clone();
                let ship_updated_tx_clone = ship_updated_tx.clone();
                let ship_action_completed_tx_clone = ship_action_completed_tx.clone();
                let ship_task_clone = ship_task.clone();
                let ship_symbol_clone = ship_symbol.clone();

                let fiber = tokio::spawn(async move {
                    Self::ship_loop(
                        ship_op_clone,
                        args_clone,
                        ship_updated_tx_clone,
                        ship_action_completed_tx_clone,
                        ship_task_clone,
                    )
                    .await?;
                    Ok(())
                });

                ship_fibers.insert(ship_symbol_clone, fiber);
            }
        }
        // run forever
        tokio::join!(ship_updated_listener_join_handle);
        Ok(())
    }

    pub async fn ship_loop(
        ship_op: Arc<Mutex<ShipOperations>>,
        args: BehaviorArgs,
        ship_updated_tx: Sender<ShipOperations>,
        ship_action_completed_tx: Sender<ActionEvent>,
        ship_task: ShipTask,
    ) -> Result<()> {
        use tracing::Level;
        let behaviors = ship_behaviors();

        let mut ship = ship_op.lock().await;

        let maybe_behavior = match ship_task {
            ShipTask::PurchaseShip { .. } => None,
            ShipTask::ObserveWaypointDetails { waypoint_symbol } => {
                ship.set_explore_locations(vec![waypoint_symbol]);
                println!("ship_loop: Ship {:?} is running explorer_behavior", ship.symbol);
                Some(behaviors.explorer_behavior)
            }
            ShipTask::ObserveAllWaypointsOnce { waypoint_symbols } => {
                ship.set_explore_locations(waypoint_symbols);
                println!("ship_loop: Ship {:?} is running explorer_behavior", ship.symbol);
                Some(behaviors.explorer_behavior)
            }
            ShipTask::MineMaterialsAtWaypoint { .. } => None,
            ShipTask::DeliverMaterials { .. } => None,
            ShipTask::SurveyAsteroid { .. } => None,
        };

        match maybe_behavior {
            None => {}
            Some(ship_behavior) => {
                let mut tick: usize = 0;
                let span = span!(Level::INFO, "ship_loop", tick, ship = format!("{}", ship.symbol.0),);
                tick += 1;

                let _enter = span.enter();

                let result: std::result::Result<Response, Error> = ship_behavior
                    .run(
                        &args,
                        &mut ship,
                        Duration::from_secs(1),
                        &ship_updated_tx.clone(),
                        &ship_action_completed_tx.clone(),
                    )
                    .await;

                match &result {
                    Ok(o) => {
                        event!(
                            name: "Ship Tick done ",
                            Level::INFO,
                            result = %o,
                        );
                    }
                    Err(e) => {
                        event!(
                            name: "Ship Tick done with Error",
                            Level::INFO,
                            result = %e,
                        );
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn listen_to_ship_changes_and_persist(mut ship_updated_rx: Receiver<ShipOperations>, mm: DbModelManager) -> Result<()> {
        let mut old_ship_state: Option<ShipOperations> = None;

        while let Some(updated_ship) = ship_updated_rx.recv().await {
            match old_ship_state {
                Some(old_ship_ops) if old_ship_ops.ship == updated_ship.ship => {
                    // no need to update
                    event!(Level::INFO, "No need to update ship {}. No change detected", updated_ship.symbol.0);
                }
                _ => {
                    event!(Level::INFO, "Ship {} updated", updated_ship.symbol.0);
                    let _ = db::upsert_ships(mm.pool(), &vec![updated_ship.ship.clone()], Utc::now()).await?;
                }
            }

            old_ship_state = Some(updated_ship.clone());
        }

        Ok(())
    }

    pub async fn listen_to_ship_action_update_messages(mut ship_action_completed_rx: Receiver<ActionEvent>, mm: DbModelManager) -> Result<()> {
        let mut old_ship_state: Option<ShipOperations> = None;

        while let Some(msg) = ship_action_completed_rx.recv().await {
            match msg {
                ActionEvent::ShipActionCompleted(result) => match result {
                    Ok((ship_op, ship_action)) => {
                        let ss = ship_op.symbol.0.clone();
                        event!(
                            Level::INFO,
                            message = "ShipActionCompleted",
                            ship = ss,
                            action = %ship_action,
                        );
                    }
                    Err(err) => {
                        event!(Level::ERROR, message = "Error completing ShipAction", error = %err,);
                    }
                },
                ActionEvent::BehaviorCompleted(result) => match result {
                    Ok(_) => {}
                    Err(_) => {}
                },
            }
        }

        Ok(())
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct FleetAdmiral {
    completed_fleet_tasks: Vec<FleetTaskCompletion>,
    fleets: HashMap<FleetId, Fleet>,
    all_ships: HashMap<ShipSymbol, Ship>,
    pub(crate) ship_tasks: HashMap<ShipSymbol, ShipTask>,
    fleet_tasks: HashMap<FleetId, Vec<FleetTask>>,
    ship_fleet_assignment: HashMap<ShipSymbol, FleetId>,
}

impl FleetAdmiral {
    pub async fn run_fleets(&mut self, client: Arc<dyn StClientTrait>, db_model_manager: &DbModelManager) -> Result<()> {
        event!(Level::INFO, "Running fleets");

        FleetRunner::run_fleets(self, Arc::clone(&client), db_model_manager).await?;

        Ok(())
    }

    pub async fn load_or_create(mm: &DbModelManager, system_symbol: SystemSymbol) -> Result<Self> {
        match Self::load_admiral(mm).await? {
            None => {
                let admiral = Self::create(mm, system_symbol).await?;
                let _ = FleetBmc::store_fleets_data(
                    &Ctx::Anonymous,
                    mm,
                    &admiral.fleets,
                    &admiral.fleet_tasks,
                    &admiral.ship_fleet_assignment,
                    &admiral.ship_tasks,
                )
                .await?;
                Ok(admiral)
            }
            Some(admiral) => Ok(admiral),
        }
    }

    async fn load_admiral(mm: &DbModelManager) -> Result<Option<Self>> {
        let overview = FleetBmc::load_overview(&Ctx::Anonymous, mm).await?;

        if overview.fleets.is_empty() || overview.all_ships.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Self {
                completed_fleet_tasks: overview.completed_fleet_tasks,
                fleets: overview.fleets,
                all_ships: overview.all_ships,
                ship_tasks: overview.ship_tasks,
                fleet_tasks: overview.fleet_task_assignments,
                ship_fleet_assignment: overview.ship_fleet_assignment,
            }))
        }
    }

    async fn create(mm: &DbModelManager, system_symbol: SystemSymbol) -> Result<Self> {
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
        let fleets_with_tasks: Vec<(Fleet, (FleetId, FleetTask))> = fleet_configs
            .into_iter()
            .enumerate()
            .map(|(idx, (cfg, task))| Self::create_fleet(cfg.clone(), task.clone(), (completed_tasks.len() + idx) as i32).unwrap())
            .collect_vec();

        let (fleets, fleet_tasks): (Vec<Fleet>, Vec<(FleetId, FleetTask)>) = fleets_with_tasks.into_iter().unzip();

        let ship_fleet_assignment = Self::assign_ships(&fleets, &ship_map);

        let mut fleet_map: HashMap<FleetId, Fleet> = fleets.into_iter().map(|f| (f.id.clone(), f)).collect();
        let fleet_task_map: HashMap<FleetId, Vec<FleetTask>> = fleet_tasks.into_iter().map(|(fleet_id, task)| (fleet_id, vec![task])).collect();

        let mut admiral = Self {
            completed_fleet_tasks: completed_tasks,
            fleets: fleet_map,
            all_ships: ship_map,
            fleet_tasks: fleet_task_map,
            ship_tasks: Default::default(),
            ship_fleet_assignment,
        };

        let _ = Self::compute_ship_tasks(&mut admiral, &facts).await?;

        Ok(admiral)
    }

    async fn compute_ship_tasks(admiral: &mut FleetAdmiral, facts: &FleetDecisionFacts) -> Result<()> {
        for (fleet_id, fleet) in admiral.fleets.clone().iter() {
            match &fleet.cfg {
                FleetConfig::SystemSpawningCfg(cfg) => {
                    let ship_tasks = SystemSpawningFleet::compute_ship_tasks(admiral, cfg, fleet, facts).await?;
                    for (ss, task) in ship_tasks {
                        admiral.ship_tasks.insert(ss, task);
                    }
                }
                FleetConfig::MarketObservationCfg(cfg) => {
                    let ship_tasks = MarketObservationFleet::compute_ship_tasks(admiral, cfg, fleet, facts).await?;
                    for (ss, task) in ship_tasks {
                        admiral.ship_tasks.insert(ss, task);
                    }
                }
                FleetConfig::TradingCfg(cfg) => (),
                FleetConfig::ConstructJumpGateCfg(cfg) => (),
                FleetConfig::MiningCfg(cfg) => (),
                FleetConfig::SiphoningCfg(cfg) => (),
            }
        }

        Ok(())
    }

    pub fn create_fleet(super_fleet_config: FleetConfig, fleet_task: FleetTask, id: i32) -> Result<(Fleet, (FleetId, FleetTask))> {
        let id = FleetId(id);
        let mut fleet = Fleet {
            id: id.clone(),
            cfg: super_fleet_config,
        };

        Ok((fleet, (id, fleet_task)))
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

    pub(crate) fn get_ships_of_fleet(&self, fleet: &Fleet) -> Vec<&Ship> {
        self.ship_fleet_assignment
            .iter()
            .filter_map(|(ship_symbol, fleet_id)| {
                if fleet_id == &fleet.id {
                    self.all_ships.get(&ship_symbol)
                } else {
                    None
                }
            })
            .collect_vec()
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
pub fn diff_waypoint_symbols(waypoints_of_interest: &[WaypointSymbol], already_explored: &[WaypointSymbol]) -> Vec<WaypointSymbol> {
    let set2: HashSet<_> = already_explored.iter().collect();

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
