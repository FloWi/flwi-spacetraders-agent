use crate::behavior_tree::behavior_args::{BehaviorArgs, DbBlackboard};
use crate::behavior_tree::behavior_tree::{ActionEvent, Actionable, Behavior, Response};
use crate::behavior_tree::ship_behaviors::{ship_navigation_behaviors, Behaviors, ShipAction};
use crate::exploration::exploration::generate_exploration_route;
use crate::marketplaces::marketplaces::{filter_waypoints_with_trait, find_marketplaces_for_exploration, find_shipyards_for_exploration};
use crate::ship::ShipOperations;
use crate::st_client::StClientTrait;
use anyhow::{anyhow, Error, Result};
use itertools::Itertools;
use log::{log, Level};
use serde::{Deserialize, Serialize};
use st_domain::{
    FleetDecisionFacts, FleetTask, FleetUpdateMessage, GetConstructionResponse, GetConstructionResponseData, MaterializedSupplyChain, Ship,
    ShipRegistrationRole, ShipRole, ShipSymbol, ShipTaskMessage, ShipType, SystemSymbol, TradeGoodSymbol, Waypoint, WaypointSymbol, WaypointTraitSymbol,
};
use st_store::{
    db, select_latest_marketplace_entry_of_system, select_latest_shipyard_entry_of_system, ConstructionBmc, Ctx, DbJumpGateData, DbModelManager,
    DbWaypointEntry, MarketBmc, ShipBmc, SystemBmc,
};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::ops::Not;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::{mpsc, Mutex};
use tracing::{event, span};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FleetId(u32);

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SystemSpawningFleet {
    id: FleetId,
    system_symbol: SystemSymbol,
    marketplace_waypoints_of_interest: Vec<WaypointSymbol>,
    shipyard_waypoints_of_interest: Vec<WaypointSymbol>,
    spawn_ship_symbol: ShipSymbol,
    #[serde(skip)]
    ship_operations: HashMap<ShipSymbol, ShipOperations>,
    completed_exploration_tasks: HashSet<WaypointSymbol>,
    budget: u64,
    current_task: Option<ShipTask>,
}

impl SystemSpawningFleet {
    pub fn all_exploration_tasks(&self) -> Vec<WaypointSymbol> {
        self.marketplace_waypoints_of_interest.iter().chain(self.shipyard_waypoints_of_interest.iter()).unique().cloned().collect_vec()
    }

    pub async fn run(
        fleet: Arc<Mutex<SystemSpawningFleet>>,
        db_model_manager: DbModelManager,
        ship_updated_tx: Sender<ShipOperations>,
        fleet_updated_tx: Sender<FleetUpdateMessage>,
    ) -> Result<()> {
        log!(Level::Info, "Running SystemSpawningFleet",);

        let task = {
            let mut fleet_guard = fleet.lock().await;
            let task = fleet_guard.compute_initial_exploration_ship_task(&db_model_manager).await?;
            fleet_guard.current_task = task.clone();
            task
        };

        log!(Level::Info, "Computed this task for the command ship: {}", serde_json::to_string_pretty(&task)?);

        match task {
            Some(ShipTask::ObserveAllWaypointsOnce { waypoint_symbols }) => {
                let (ship_action_completed_tx, ship_action_completed_rx, command_ship) = {
                    let mut fleet_guard = fleet.lock().await;
                    // Get necessary values while locked
                    let command_ship_symbol = &fleet_guard.spawn_ship_symbol.clone();
                    let mut command_ship = fleet_guard.ship_operations.get_mut(command_ship_symbol).unwrap().clone();

                    command_ship.set_explore_locations(waypoint_symbols.clone());

                    //some waypoints might have already been explored. We put them in the HashSet for bookkeeping.
                    let already_explored_shipyards = fleet_guard
                        .shipyard_waypoints_of_interest
                        .iter()
                        .cloned()
                        .filter(|wp_of_interest| waypoint_symbols.contains(&wp_of_interest).not())
                        .collect_vec();

                    let already_explored_marketplaces = fleet_guard
                        .marketplace_waypoints_of_interest
                        .iter()
                        .cloned()
                        .filter(|wp_of_interest| waypoint_symbols.contains(&wp_of_interest).not())
                        .collect_vec();

                    //mark them as completed
                    for already_explored_wp in already_explored_marketplaces.into_iter().chain(already_explored_shipyards.into_iter()) {
                        fleet_guard.completed_exploration_tasks.insert(already_explored_wp.clone());
                    }

                    // Create channels
                    let (ship_action_completed_tx, ship_action_completed_rx) = mpsc::channel(32);

                    (ship_action_completed_tx, ship_action_completed_rx, command_ship)
                }; // Lock is released here

                // Pass a clone of the fleet Arc for the task
                let fleet_for_listener = Arc::clone(&fleet);
                tokio::spawn(async move {
                    Self::consume_ship_action_messages(ship_action_completed_rx, fleet_for_listener).await;
                });

                let args = BehaviorArgs {
                    blackboard: Arc::new(DbBlackboard {
                        model_manager: db_model_manager,
                    }),
                };
                // Another clone for the ship loop
                let _ = tokio::spawn(ship_loop(command_ship, args, ship_updated_tx, ship_action_completed_tx));
            }
            maybe_task => {
                log!(Level::Warn, "Not implemented yet. {maybe_task:?}");
            }
        }

        Ok(())
    }

    async fn consume_ship_action_messages(mut ship_action_completed_rx: Receiver<ActionEvent>, fleet: Arc<Mutex<SystemSpawningFleet>>) {
        while let Some(event) = ship_action_completed_rx.recv().await {
            match event {
                ActionEvent::ShipActionCompleted(result) => match result {
                    Ok((ship_op, action)) => {
                        log!(Level::Info, "ShipAction completed successfully: {}", action);
                        // Update the ship operations in the fleet with the latest version
                        {
                            let mut fleet_guard = fleet.lock().await;
                            // Store the updated ship operations in the map
                            fleet_guard.ship_operations.insert(ship_op.symbol.clone(), ship_op.clone());
                        }

                        match action {
                            ShipAction::CollectWaypointInfos => {
                                // Lock the fleet to update it
                                let mut fleet_guard = fleet.lock().await;
                                let current_location = ship_op.current_location();

                                fleet_guard.completed_exploration_tasks.insert(current_location.clone());
                                log!(
                                        Level::Info,
                                        "CollectWaypointInfos: {} of {} exploration_tasks complete for SystemSpawningFleet. Current location: {:?}\nCompleted tasks: {:?}\nQueue: {:?}",
                                        fleet_guard.completed_exploration_tasks.len(),
                                        fleet_guard.all_exploration_tasks().len(),
                                        current_location,
                                        fleet_guard.completed_exploration_tasks,
                                        ship_op.explore_location_queue
                                    );
                            }
                            _ => {}
                        }
                    }
                    Err(e) => {
                        log!(Level::Warn, "ShipAction failed: {}", e);
                    }
                },
                ActionEvent::BehaviorCompleted(result) => match result {
                    Ok(behavior) => {
                        log!(Level::Debug, "Behavior completed successfully: {}", behavior);
                    }
                    Err(e) => {
                        log!(Level::Warn, "Behavior failed: {}", e);
                    }
                },
            }
        }
    }
}

impl SystemSpawningFleet {
    pub async fn compute_initial_exploration_ship_task(&self, mm: &DbModelManager) -> Result<Option<ShipTask>> {
        let waypoints_of_system = SystemBmc::get_waypoints_of_system(&Ctx::Anonymous, mm, &self.system_symbol).await?;

        let marketplace_entries = select_latest_marketplace_entry_of_system(mm.pool(), &self.system_symbol).await?;

        let marketplaces_to_explore = find_marketplaces_for_exploration(marketplace_entries.clone());

        let shipyard_entries = select_latest_shipyard_entry_of_system(mm.pool(), &self.system_symbol).await?;

        let shipyards_to_explore = find_shipyards_for_exploration(shipyard_entries.clone());

        log!(Level::Debug, "waypoints_of_system: {waypoints_of_system:?}");
        log!(Level::Debug, "marketplace_entries: {marketplace_entries:?}");
        log!(Level::Debug, "marketplaces_to_explore: {marketplaces_to_explore:?}");

        log!(Level::Debug, "shipyard_entries: {shipyard_entries:?}");
        log!(Level::Debug, "shipyards_to_explore: {shipyards_to_explore:?}");

        let relevant_exploration_targets = marketplaces_to_explore
            .into_iter()
            .chain(shipyards_to_explore.into_iter())
            .filter(|wp_symbol| self.marketplace_waypoints_of_interest.contains(wp_symbol) || self.shipyard_waypoints_of_interest.contains(wp_symbol))
            .unique()
            .collect_vec();

        log!(Level::Info, "relevant_exploration_targets: {relevant_exploration_targets:?}");

        let current_location = self.ship_operations.get(&self.spawn_ship_symbol).unwrap().nav.waypoint_symbol.clone();

        let exploration_route = generate_exploration_route(&relevant_exploration_targets, &waypoints_of_system, &current_location);

        let exploration_route_symbols = exploration_route.unwrap_or_default().into_iter().map(|wp| wp.symbol).collect_vec();

        Ok(exploration_route_symbols.is_empty().not().then_some(ShipTask::ObserveAllWaypointsOnce {
            waypoint_symbols: exploration_route_symbols,
        }))
    }

    fn compute_shopping_list() {
        /*
           List(
             // IMPORTANT: the shopping list is persisted to DB, later changes might not be effective
             (1, SHIP_COMMAND_FRIGATE, FRIGATE_GENERALIST),
             (numExplorersForSystem, SHIP_PROBE, INNER_SYSTEM_EXPLORER),
             (2, SHIP_SURVEYOR, SURVEYOR),
             (2, SHIP_SIPHON_DRONE, STARTING_SIPHONER_I),
             (1, SHIP_MINING_DRONE, STARTING_MINER_I),
             (1, SHIP_LIGHT_HAULER, LIGHT_MINING_HAULER),
             (4, SHIP_MINING_DRONE, STARTING_MINER_I),
             (1, SHIP_LIGHT_HAULER, CONSTRUCTOR),
             (3, SHIP_SIPHON_DRONE, STARTING_SIPHONER_I),
             (3, SHIP_MINING_DRONE, STARTING_MINER_I),
             (2, SHIP_LIGHT_HAULER, CONSTRUCTOR),
             (1, SHIP_LIGHT_HAULER, LIGHT_MINING_HAULER),
             (5, SHIP_MINING_DRONE, STARTING_MINER_I),
             (1, SHIP_LIGHT_HAULER, CONTRACTOR),
             (2, SHIP_LIGHT_HAULER, CONSTRUCTOR),
             // IMPORTANT: the shopping list is persisted to DB, later changes might not be effective
           ).flatMap { case (num, tpe, role) => 1.to(num).map(_ => ShipShoppingListEntry(tpe, role)) }
         }
        */
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MarketObservationFleet {
    id: FleetId,
    system_symbol: SystemSymbol,
    marketplace_waypoints_of_interest: Vec<WaypointSymbol>,
    shipyard_waypoints_of_interest: Vec<WaypointSymbol>,
    ship_assignment: HashMap<ShipSymbol, WaypointSymbol>,
    ship_role_assignment: HashMap<ShipSymbol, Vec<ShipRole>>,
    budget: u64,
}

impl MarketObservationFleet {
    pub async fn run(&self) -> Result<()> {
        log!(Level::Info, "Running MarketObservationFleet",);
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MiningFleet {
    id: FleetId,
    system_symbol: SystemSymbol,
    mining_waypoint: WaypointSymbol,
    materials: Vec<TradeGoodSymbol>,
    mining_ships: Vec<ShipSymbol>,
    mining_haulers: Vec<ShipSymbol>,
    delivery_locations: HashMap<TradeGoodSymbol, WaypointSymbol>,
    budget: u64,
    desired_ship_roles: HashMap<ShipRole, u16>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SiphoningFleet {
    id: FleetId,
    system_symbol: SystemSymbol,
    siphoning_waypoint: WaypointSymbol,
    materials: Vec<TradeGoodSymbol>,
    mining_ships: Vec<ShipSymbol>,
    mining_haulers: Vec<ShipSymbol>,
    delivery_locations: HashMap<TradeGoodSymbol, WaypointSymbol>,
    budget: u64,
    desired_ship_roles: HashMap<ShipRole, u16>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TradingFleet {
    id: FleetId,
    system_symbol: SystemSymbol,
    materials: Vec<TradeGoodSymbol>,
    trading_ships: Vec<ShipSymbol>,
    budget: u64,
    desired_ship_roles: HashMap<ShipRole, u16>,
}

impl MiningFleet {
    pub async fn run(&self) -> Result<()> {
        log!(Level::Info, "Running MiningFleet",);
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum FleetType {
    SystemSpawning,
    MarketObservation,
    Mining,
    Siphon,
    Trade,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Fleet {
    SystemSpawning(SystemSpawningFleet),
    MarketObservation(MarketObservationFleet),
    Mining(MiningFleet),
    Siphon(SiphoningFleet),
    Trade(TradingFleet),
}

pub fn compute_fleet_tasks(system_symbol: SystemSymbol, fleet_decision_facts: FleetDecisionFacts) -> Vec<FleetTask> {
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

    let is_jump_gate_done = fleet_decision_facts.construction_site.map(|cs| cs.is_complete).unwrap_or(false);
    let is_shipyard_exploration_complete = are_vecs_equal_ignoring_order(
        &fleet_decision_facts.shipyards_of_interest,
        &fleet_decision_facts.shipyards_with_up_to_date_infos,
    );
    let is_marketplace_exploration_complete = are_vecs_equal_ignoring_order(
        &fleet_decision_facts.marketplaces_of_interest,
        &fleet_decision_facts.marketplaces_with_up_to_date_infos,
    );
    let has_collected_all_waypoint_details_once = is_shipyard_exploration_complete && is_marketplace_exploration_complete;

    let tasks = if !has_collected_all_waypoint_details_once {
        vec![CollectMarketInfosOnce {
            system_symbol: system_symbol.clone(),
        }]
    } else if !is_jump_gate_done {
        vec![
            ConstructJumpGate {
                system_symbol: system_symbol.clone(),
            },
            ObserveAllWaypointsOfSystemWithProbes {
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
            ObserveAllWaypointsOfSystemWithProbes {
                system_symbol: system_symbol.clone(),
            },
        ]
    } else {
        unimplemented!("this shouldn't happen - think harder")
    };

    tasks
}

pub async fn collect_fleet_decision_facts(mm: &DbModelManager, system_symbol: SystemSymbol) -> Result<FleetDecisionFacts> {
    let ships = ShipBmc::get_ships(&Ctx::Anonymous, mm, None).await?;
    let waypoints_of_system = SystemBmc::get_waypoints_of_system(&Ctx::Anonymous, mm, &system_symbol).await?;

    let marketplaces_of_interest = select_latest_marketplace_entry_of_system(mm.pool(), &system_symbol).await?;
    let marketplace_symbols_of_interest = marketplaces_of_interest.iter().map(|db_entry| WaypointSymbol(db_entry.waypoint_symbol.clone())).collect_vec();
    let marketplaces_to_explore = find_marketplaces_for_exploration(marketplaces_of_interest.clone());

    let shipyards_of_interest = select_latest_shipyard_entry_of_system(mm.pool(), &system_symbol).await?;
    let shipyard_symbols_of_interest = shipyards_of_interest.iter().map(|db_entry| WaypointSymbol(db_entry.waypoint_symbol.clone())).collect_vec();
    let shipyards_to_explore = find_shipyards_for_exploration(shipyards_of_interest.clone());

    let maybe_construction_site: Option<GetConstructionResponse> =
        ConstructionBmc::get_construction_site_for_system(&Ctx::Anonymous, mm, system_symbol).await?;

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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ShipTask {
    PurchaseShip {
        r#type: ShipType,
        max_amount: u32,
        waypoint_symbol: WaypointSymbol,
    },

    ObserveWaypointDetails {
        waypoint_symbol: WaypointSymbol,
    },

    ObserveAllWaypointsOnce {
        waypoint_symbols: Vec<WaypointSymbol>,
    },

    MineMaterialsAtWaypoint {
        mining_waypoint: WaypointSymbol,
    },

    DeliverMaterials {
        delivery_locations: HashMap<TradeGoodSymbol, WaypointSymbol>,
    },

    SurveyAsteroid {
        waypoint_symbol: WaypointSymbol,
    },
}

/*
- Game starts with two ships - command ship and one probe
- we first need some data for markets and shipyards in order to earn money for more ships
- we assign the command ship to the SystemSpawningFleet and give it the relevant waypoints
- we assign the probe to the MarketObservationFleet. It should already be placed at the shipyard, so we can assign this waypoint already
- we create an empty Mining Fleet
- we create an empty Siphoning Fleet
- we create an empty Trading/Construction Fleet

 */

pub fn fleet_foo() {}

pub(crate) async fn compute_initial_fleets(
    ships: Vec<Ship>,
    home_system_symbol: &SystemSymbol,
    waypoints_of_home_system: &[Waypoint],
    client: Arc<dyn StClientTrait>,
) -> Result<Vec<Fleet>> {
    assert_eq!(ships.len(), 2, "Expecting two ships to start");

    if ships.len() != 2 {
        return anyhow::bail!("Expected 2 ships, but found {}", ships.len());
    }

    let marketplace_waypoints =
        filter_waypoints_with_trait(waypoints_of_home_system, WaypointTraitSymbol::MARKETPLACE).map(|wp| wp.symbol.clone()).collect_vec();
    let shipyard_waypoints = filter_waypoints_with_trait(waypoints_of_home_system, WaypointTraitSymbol::SHIPYARD).map(|wp| wp.symbol.clone()).collect_vec();

    let command_ship = ships.iter().find(|ship| ship.registration.role == ShipRegistrationRole::Command).unwrap().clone();

    let probe_ship = ships.iter().find(|ship| ship.registration.role == ShipRegistrationRole::Satellite).unwrap();

    // iirc the probe gets spawned at a shipyard
    // make sure, this is the case and expect it
    let probe_at_shipyard_location =
        shipyard_waypoints.iter().find(|wps| **wps == probe_ship.nav.waypoint_symbol).cloned().expect("expecting probe to be spawned at shipyard");

    let unexplored_shipyards = shipyard_waypoints.iter().filter(|wp| **wp != probe_at_shipyard_location).cloned().collect_vec();

    log!(Level::Info, "found {} ships: {}", &ships.len(), serde_json::to_string_pretty(&ships)?);

    log!(Level::Info, "command_ship: {}", serde_json::to_string_pretty(&command_ship)?);
    log!(Level::Info, "probe_ship: {}", serde_json::to_string_pretty(&probe_ship)?);

    let command_ship_op = ShipOperations::new(command_ship.clone(), Arc::clone(&client));

    let system_spawning_fleet = SystemSpawningFleet {
        id: FleetId(1),
        system_symbol: home_system_symbol.clone(),
        marketplace_waypoints_of_interest: marketplace_waypoints.clone(),
        shipyard_waypoints_of_interest: unexplored_shipyards.clone(),

        spawn_ship_symbol: command_ship.symbol.clone(),
        ship_operations: HashMap::from([(command_ship.symbol.clone(), command_ship_op)]),
        completed_exploration_tasks: HashSet::new(),
        current_task: None,
        budget: 0,
    };

    // let command_ship_tasks: ShipTask = system_spawning_fleet
    //     .compute_ship_task(&model_manager)
    //     .await?;

    let market_observation_fleet = MarketObservationFleet {
        id: FleetId(2),
        system_symbol: home_system_symbol.clone(),
        marketplace_waypoints_of_interest: marketplace_waypoints.clone(),
        shipyard_waypoints_of_interest: shipyard_waypoints.clone(),
        ship_assignment: HashMap::from([(probe_ship.symbol.clone(), probe_at_shipyard_location.clone())]),
        ship_role_assignment: HashMap::from([(command_ship.symbol.clone(), vec![ShipRole::MarketObserver, ShipRole::ShipPurchaser])]),
        budget: 0,
    };

    let fleets = vec![
        Fleet::SystemSpawning(system_spawning_fleet),
        Fleet::MarketObservation(market_observation_fleet),
    ];

    log!(Level::Info, "Created these fleets: {}", serde_json::to_string_pretty(&fleets)?);

    Ok(fleets)
}

pub async fn ship_loop(
    mut ship: ShipOperations,
    args: BehaviorArgs,
    ship_updated_tx: Sender<ShipOperations>,
    ship_action_completed_tx: Sender<ActionEvent>,
) -> Result<()> {
    use tracing::Level;

    let behaviors = ship_navigation_behaviors();
    let ship_behavior: Behavior<ShipAction> = behaviors.explorer_behavior;

    println!("Running behavior tree. \n<mermaid>\n{}\n</mermaid>", ship_behavior.to_mermaid());

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

    event!(Level::INFO, "Ship Loop done",);

    Ok(())
}
