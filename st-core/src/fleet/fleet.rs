use crate::behavior_tree::behavior_args::{BehaviorArgs, DbBlackboard};
use crate::behavior_tree::behavior_tree::{ActionEvent, Actionable, Behavior, Response};
use crate::behavior_tree::ship_behaviors::{ship_behaviors, ShipAction};
use crate::fleet::construction_fleet::{ConstructJumpGateFleet, PotentialTradingTask};
use crate::fleet::fleet_runner::FleetRunner;
use crate::fleet::market_observation_fleet::MarketObservationFleet;
use crate::fleet::system_spawning_fleet::SystemSpawningFleet;
use crate::fleet::trading_manager::TradingManager;
use crate::marketplaces::marketplaces::{find_marketplaces_for_exploration, find_shipyards_for_exploration};
use crate::pagination::fetch_all_pages;
use crate::ship::ShipOperations;
use crate::st_client::{StClient, StClientTrait};
use anyhow::{anyhow, Error, Result};
use chrono::{DateTime, Utc};
use itertools::Itertools;
use pathfinding::num_traits::Zero;
use serde::{Deserialize, Serialize};
use sqlx::__rt::JoinHandle;
use sqlx::{Pool, Postgres};
use st_domain::blackboard_ops::BlackboardOps;
use st_domain::FleetConfig::SystemSpawningCfg;
use st_domain::FleetTask::{
    CollectMarketInfosOnce, ConstructJumpGate, MineOres, ObserveAllWaypointsOfSystemWithStationaryProbes, SiphonGases, TradeProfitably,
};
use st_domain::FleetUpdateMessage::FleetTaskCompleted;
use st_domain::{
    get_exploration_tasks_for_waypoint, Agent, ConstructJumpGateFleetConfig, EvaluatedTradingOpportunity, ExplorationTask, Fleet, FleetConfig,
    FleetDecisionFacts, FleetId, FleetPhase, FleetPhaseName, FleetTask, FleetTaskCompletion, FleetsOverview, GetConstructionResponse, MarketData, MarketEntry,
    MarketObservationFleetConfig, MaterializedSupplyChain, MiningFleetConfig, PurchaseGoodTicketDetails, PurchaseReason, PurchaseShipTicketDetails,
    SellGoodTicketDetails, Ship, ShipFrameSymbol, ShipPriceInfo, ShipSymbol, ShipTask, ShipType, SiphoningFleetConfig, StationaryProbeLocation,
    SystemSpawningFleetConfig, SystemSymbol, TicketId, TradeGoodSymbol, TradeTicket, TradingFleetConfig, Transaction, TransactionActionEvent, Waypoint,
    WaypointSymbol,
};
use st_store::bmc::Bmc;
use st_store::{
    db, load_fleet_overview, select_latest_marketplace_entry_of_system, select_latest_shipyard_entry_of_system, upsert_fleets_data, Ctx, DbConstructionBmc,
    DbModelManager,
};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::ops::Not;
use std::slice::Iter;
use std::sync::Arc;
use std::time::Duration;
use strum_macros::Display;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::{Mutex, MutexGuard};
use tracing::{event, span, Level};
use uuid::Uuid;

#[derive(Deserialize, Serialize, Debug, Clone)]
pub enum ShipStatusReport {
    ShipActionCompleted(Ship, ShipAction),
    TransactionCompleted(Ship, TransactionActionEvent, TradeTicket),
    ShipFinishedBehaviorTree(Ship, ShipTask),
}

impl ShipStatusReport {
    pub(crate) fn ship_symbol(&self) -> ShipSymbol {
        match self {
            ShipStatusReport::ShipActionCompleted(s, _) => s.symbol.clone(),
            ShipStatusReport::TransactionCompleted(s, _, _) => s.symbol.clone(),
            ShipStatusReport::ShipFinishedBehaviorTree(s, _) => s.symbol.clone(),
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct FleetAdmiral {
    pub completed_fleet_tasks: Vec<FleetTaskCompletion>,
    pub fleets: HashMap<FleetId, Fleet>,
    pub all_ships: HashMap<ShipSymbol, Ship>,
    pub ship_tasks: HashMap<ShipSymbol, ShipTask>,
    pub fleet_tasks: HashMap<FleetId, Vec<FleetTask>>,
    pub ship_fleet_assignment: HashMap<ShipSymbol, FleetId>,
    pub agent_info: Agent,
    pub fleet_phase: FleetPhase,
    pub active_trades: HashMap<ShipSymbol, TradeTicket>,
    pub stationary_probe_locations: Vec<StationaryProbeLocation>,
}

impl FleetAdmiral {
    pub fn get_next_ship_purchase(&self) -> Option<ShipType> {
        let mapping = role_to_ship_type_mapping();

        let mut current_ship_types: HashMap<ShipType, u32> = HashMap::new();

        for (_, s) in self.all_ships.iter() {
            let ship_type = mapping.get(&s.frame.symbol).expect("role_to_ship_type_mapping");
            current_ship_types.entry(*ship_type).and_modify(|counter| *counter += 1).or_insert(1);
        }

        for (ship_type, _) in self.fleet_phase.shopping_list_in_order.iter() {
            let num_of_ships_left = current_ship_types.get(&ship_type).unwrap_or(&0);
            if num_of_ships_left.is_zero() {
                return Some(*ship_type);
            } else {
                // we already have this ship - continue
                current_ship_types.entry(*ship_type).and_modify(|counter| *counter -= 1);
            }
        }
        None
    }

    pub(crate) fn get_ship_tasks_of_fleet(&self, fleet: &Fleet) -> Vec<(ShipSymbol, ShipTask)> {
        self.get_ships_of_fleet(fleet).iter().flat_map(|ss| self.get_task_of_ship(&ss.symbol).map(|st| (ss.symbol.clone(), st.clone()))).collect_vec()
    }

    pub(crate) fn get_total_budget_for_fleet(&self, fleet: &Fleet) -> u64 {
        // todo: take into account what the fleet still has to do
        // e.g. a fully equipped market_observation_fleet (probes at all locations) doesn't need any budget
        let number_of_fleets = self.fleets.len();
        let budget_per_fleet = self.agent_info.credits / number_of_fleets as i64;

        let fleet_budget: u64 = self.calculate_budget_for_fleet(&self.agent_info, fleet, &self.fleets);
        fleet_budget
    }

    pub fn calculate_budget_for_fleet(&self, agent: &Agent, fleet: &Fleet, fleets: &HashMap<FleetId, Fleet>) -> u64 {
        match self.fleet_phase.name {
            FleetPhaseName::InitialExploration => 0,
            FleetPhaseName::ConstructJumpGate => match fleet.cfg {
                FleetConfig::ConstructJumpGateCfg(_) => {
                    if agent.credits < 0 {
                        0
                    } else {
                        agent.credits as u64
                    }
                }
                _ => 0,
            },
            FleetPhaseName::TradeProfitably => 0,
        }
    }

    fn sum_allocated_tickets<'a, I>(trade_tickets: I) -> u64
    where
        I: IntoIterator<Item = &'a TradeTicket>,
    {
        trade_tickets
            .into_iter()
            .map(|trade_ticket| match trade_ticket {
                TradeTicket::PurchaseShipTicket { details, .. } => details.allocated_credits,
                TradeTicket::TradeCargo {
                    purchase_completion_status, ..
                } => purchase_completion_status.iter().filter_map(|(ticket, is_completed)| (!is_completed).then_some(ticket.allocated_credits)).sum(),
                TradeTicket::DeliverConstructionMaterials {
                    purchase_completion_status, ..
                } => purchase_completion_status.iter().filter_map(|(ticket, is_completed)| (!is_completed).then_some(ticket.allocated_credits)).sum(),
            })
            .sum()
    }

    pub(crate) fn get_allocated_budget_of_fleet(&self, fleet: &Fleet) -> u64 {
        Self::sum_allocated_tickets(self.get_ships_of_fleet(fleet).iter().flat_map(|ship| self.active_trades.get(&ship.symbol)))
    }

    pub(crate) fn get_total_allocated_budget(&self) -> u64 {
        Self::sum_allocated_tickets(self.active_trades.values())
    }

    pub async fn run_fleets(
        fleet_admiral: Arc<Mutex<FleetAdmiral>>,
        client: Arc<dyn StClientTrait>,
        bmc: Arc<dyn Bmc>,
        blackboard: Arc<dyn BlackboardOps>,
    ) -> Result<()> {
        event!(Level::INFO, "Running fleets");

        FleetRunner::run_fleets(Arc::clone(&fleet_admiral), Arc::clone(&client), bmc, blackboard, Duration::from_secs(5)).await?;

        Ok(())
    }

    pub async fn report_ship_action_completed(&mut self, ship_status_report: &ShipStatusReport, bmc: Arc<dyn Bmc>) -> Result<()> {
        match ship_status_report {
            ShipStatusReport::ShipActionCompleted(ship, ship_action) => {
                let maybe_fleet = self.get_fleet_of_ship(&ship.symbol);
                let fleet_tasks: Vec<FleetTask> = maybe_fleet.map(|fleet_id| self.get_tasks_of_fleet(&fleet_id.id)).unwrap_or_default();
                let maybe_ship_task = self.get_task_of_ship(&ship.symbol);
                if let Some((fleet, ship_task)) = maybe_fleet.zip(maybe_ship_task) {
                    let fleet_decision_facts: FleetDecisionFacts = collect_fleet_decision_facts(Arc::clone(&bmc), &ship.nav.system_symbol).await?;
                    match &fleet.cfg {
                        SystemSpawningCfg(cfg) => {
                            if let Some(task_complete) =
                                SystemSpawningFleet::check_for_task_completion(ship_task, fleet, &fleet_tasks, cfg, &fleet_decision_facts)
                            {
                                let uncompleted_tasks = fleet_tasks.iter().filter(|&ft| ft != &task_complete.task).cloned().collect_vec();

                                event!(
                                    Level::INFO,
                                    message = "FleetTaskCompleted",
                                    ship = ship.symbol.0,
                                    fleet_id = fleet.id.0,
                                    task = task_complete.task.to_string()
                                );
                                self.fleet_tasks.insert(fleet.id.clone(), uncompleted_tasks);
                                self.completed_fleet_tasks.push(task_complete.clone());
                                bmc.fleet_bmc().save_completed_fleet_task(&Ctx::Anonymous, &task_complete).await?;
                            };
                        }
                        FleetConfig::MarketObservationCfg(_) => {}
                        FleetConfig::TradingCfg(_) => {}
                        FleetConfig::ConstructJumpGateCfg(_) => {}
                        FleetConfig::MiningCfg(_) => {}
                        FleetConfig::SiphoningCfg(_) => {}
                    }
                    Ok(())
                } else {
                    Ok(())
                }
            }
            ShipStatusReport::TransactionCompleted(ship, transaction_event, updated_trade_ticket) => {
                let ticket_id = match &updated_trade_ticket {
                    TradeTicket::TradeCargo { ticket_id, .. } => ticket_id,
                    TradeTicket::DeliverConstructionMaterials { ticket_id, .. } => ticket_id,
                    TradeTicket::PurchaseShipTicket { ticket_id, .. } => ticket_id,
                };

                let is_complete = updated_trade_ticket.is_complete();
                bmc.trade_bmc().upsert_ticket(&Ctx::Anonymous, &ship.symbol, ticket_id, &updated_trade_ticket, is_complete).await?;

                let tx_summary =
                    TradingManager::log_transaction_completed(Ctx::Anonymous, bmc.trade_bmc(), &ship, &transaction_event, &updated_trade_ticket).await?;

                let maybe_updated_agent_credits = tx_summary.transaction_action_event.maybe_updated_agent_credits();
                let old_credits = self.agent_info.credits;
                let new_credits = self.agent_info.credits + tx_summary.total_price;

                match maybe_updated_agent_credits {
                    None => {}
                    Some(agent_credits_from_response) => {
                        if agent_credits_from_response != new_credits {
                            event!(
                        Level::WARN,
                            "Agent Credits differ from our expectation!\nExpected Agent Credits: {new_credits}\n Actual Agent Credits: {agent_credits_from_response}\nApplying correct agent credits."
                              );

                            self.agent_info.credits = agent_credits_from_response;
                        }
                    }
                };

                bmc.agent_bmc().store_agent(&Ctx::Anonymous, &self.agent_info).await?;

                if is_complete {
                    event!(
                        Level::INFO,
                        "Transaction complete. It completed the whole trade.\nTransaction: {:?}\nTrade: {:?}\nTotal Price: {}\nOld Agent Credits: {}\nNew Agent Credits: {}",
                        &tx_summary.transaction_ticket_id,
                        &tx_summary.trade_ticket.ticket_id(),
                        &tx_summary.total_price,
                        old_credits,
                        self.agent_info.credits,
                    );
                    self.active_trades.remove(&ship.symbol);
                } else {
                    self.active_trades.insert(ship.symbol.clone(), updated_trade_ticket.clone());
                    event!(
                        Level::INFO,
                        "Transaction complete. Transaction is not complete yet.\nTransaction: {:?}\nTrade: {:?}\nTotal Price: {}\nOld Agent Credits: {}\nNew Agent Credits: {}",
                        &tx_summary.transaction_ticket_id,
                        &tx_summary.trade_ticket.ticket_id(),
                        &tx_summary.total_price,
                        old_credits,
                        self.agent_info.credits,
                    );
                }
                Ok(())
            }
            ShipStatusReport::ShipFinishedBehaviorTree(ship, task) => {
                event!(
                    Level::INFO,
                    message = "Ship finished behavior tree",
                    ship = ship.symbol.0,
                    task = task.to_string()
                );
                Ok(())
            }
        }
    }

    pub fn get_fleet_of_ship(&self, ship_symbol: &ShipSymbol) -> Option<&Fleet> {
        self.ship_fleet_assignment.get(ship_symbol).and_then(|fleet_id| self.fleets.get(fleet_id))
    }

    pub fn get_task_of_ship(&self, ship_symbol: &ShipSymbol) -> Option<&ShipTask> {
        self.ship_tasks.get(ship_symbol)
    }

    pub fn get_tasks_of_fleet(&self, fleet_id: &FleetId) -> Vec<FleetTask> {
        self.fleet_tasks.get(fleet_id).cloned().unwrap_or_default()
    }

    pub async fn load_or_create(bmc: Arc<dyn Bmc>, system_symbol: SystemSymbol, client: Arc<dyn StClientTrait>) -> Result<Self> {
        //make sure we have up-to-date agent info
        let agent = client.get_agent().await?;
        bmc.agent_bmc().store_agent(&Ctx::Anonymous, &agent.data).await?;

        match Self::load_admiral(Arc::clone(&bmc)).await? {
            None => {
                println!("loading admiral failed - creating a new one");
                let admiral = Self::create(Arc::clone(&bmc), system_symbol, Arc::clone(&client)).await?;
                let _ = st_store::fleet_bmc::upsert_fleets_data(
                    Arc::clone(&bmc),
                    &Ctx::Anonymous,
                    &admiral.fleets,
                    &admiral.fleet_tasks,
                    &admiral.ship_fleet_assignment,
                    &admiral.ship_tasks,
                    &admiral.active_trades,
                )
                .await?;
                Ok(admiral)
            }
            Some(admiral) => Ok(admiral),
        }
    }

    async fn load_admiral(bmc: Arc<dyn Bmc>) -> Result<Option<Self>> {
        let overview = load_fleet_overview(Arc::clone(&bmc), &Ctx::Anonymous).await?;

        if overview.fleets.is_empty() || overview.all_ships.is_empty() {
            Ok(None)
        } else {
            // fixme: needs to be aware of multiple systems
            let all_ships = overview.all_ships.values().cloned().collect_vec();
            let ship_map: HashMap<ShipSymbol, Ship> = all_ships.iter().map(|s| (s.symbol.clone(), s.clone())).collect();

            let system_symbol = all_ships.first().cloned().unwrap().nav.system_symbol;

            // recompute ship-tasks and persist them. Might have been outdated since last agent restart
            let facts = collect_fleet_decision_facts(Arc::clone(&bmc), &system_symbol).await?;
            let (fleets, fleet_tasks, fleet_phase) = compute_fleets_with_tasks(
                system_symbol,
                &overview.completed_fleet_tasks,
                &facts,
                &overview.fleets,
                &overview.fleet_task_assignments,
            );
            let mut fleet_map: HashMap<FleetId, Fleet> = fleets.iter().map(|f| (f.id.clone(), f.clone())).collect();
            let fleet_task_map: HashMap<FleetId, Vec<FleetTask>> = fleet_tasks.iter().map(|(fleet_id, task)| (fleet_id.clone(), vec![task.clone()])).collect();

            let ship_fleet_assignment = Self::assign_ships(&fleet_tasks, &ship_map, &fleet_phase.shopping_list_in_order);

            let agent_info = bmc.agent_bmc().load_agent(&Ctx::Anonymous).await?;

            let ships = overview
                .all_ships
                .into_iter()
                .filter(|(ss, ship)| overview.stationary_probe_locations.iter().any(|spl| ss == &spl.probe_ship_symbol).not())
                .collect();

            let mut admiral = Self {
                completed_fleet_tasks: overview.completed_fleet_tasks.clone(),
                fleets: fleet_map,
                all_ships: ships,
                ship_tasks: overview.ship_tasks,
                fleet_tasks: fleet_task_map,
                ship_fleet_assignment,
                agent_info,
                fleet_phase,
                active_trades: overview.open_trade_tickets,
                stationary_probe_locations: overview.stationary_probe_locations,
            };

            let new_ship_tasks = Self::compute_ship_tasks(&mut admiral, &facts, Arc::clone(&bmc)).await?;
            Self::assign_ship_tasks_and_potential_requirements(&mut admiral, new_ship_tasks);

            let _ = upsert_fleets_data(
                Arc::clone(&bmc),
                &Ctx::Anonymous,
                &admiral.fleets,
                &admiral.fleet_tasks,
                &admiral.ship_fleet_assignment,
                &admiral.ship_tasks,
                &admiral.active_trades,
            )
            .await?;

            Ok(Some(admiral))
        }
    }

    pub async fn create(bmc: Arc<dyn Bmc>, system_symbol: SystemSymbol, client: Arc<dyn StClientTrait>) -> Result<Self> {
        let ships = bmc.ship_bmc().get_ships(&Ctx::Anonymous, None).await?;
        let stationary_probe_locations = bmc.ship_bmc().get_stationary_probes(&Ctx::Anonymous).await?;

        let facts = collect_fleet_decision_facts(Arc::clone(&bmc), &system_symbol).await?;

        let ships = if ships.is_empty() {
            let ships: Vec<Ship> = fetch_all_pages(|p| client.list_ships(p)).await?;
            bmc.ship_bmc().upsert_ships(&Ctx::Anonymous, &ships, Utc::now()).await?;
            ships
        } else {
            ships
        };

        let non_probe_ships =
            ships.iter().filter(|ship| stationary_probe_locations.iter().any(|spl| ship.symbol == spl.probe_ship_symbol).not()).cloned().collect_vec();

        let ship_map: HashMap<ShipSymbol, Ship> = non_probe_ships.into_iter().map(|s| (s.symbol.clone(), s)).collect();

        let completed_tasks = bmc.fleet_bmc().load_completed_fleet_tasks(&Ctx::Anonymous).await?;

        let (fleets, fleet_tasks, fleet_phase) = compute_fleets_with_tasks(system_symbol, &completed_tasks, &facts, &HashMap::new(), &HashMap::new());

        let ship_fleet_assignment = Self::assign_ships(&fleet_tasks, &ship_map, &fleet_phase.shopping_list_in_order);

        let mut fleet_map: HashMap<FleetId, Fleet> = fleets.into_iter().map(|f| (f.id.clone(), f)).collect();
        let fleet_task_map: HashMap<FleetId, Vec<FleetTask>> = fleet_tasks.into_iter().map(|(fleet_id, task)| (fleet_id, vec![task])).collect();

        let agent_info = client.get_agent().await?.data;
        bmc.agent_bmc().store_agent(&Ctx::Anonymous, &agent_info).await?;

        let mut admiral = Self {
            completed_fleet_tasks: completed_tasks,
            fleets: fleet_map,
            all_ships: ship_map,
            fleet_tasks: fleet_task_map,
            ship_tasks: Default::default(),
            ship_fleet_assignment,
            agent_info,
            fleet_phase,
            active_trades: Default::default(),
            stationary_probe_locations,
        };

        let new_ship_tasks = Self::compute_ship_tasks(&mut admiral, &facts, Arc::clone(&bmc)).await?;
        Self::assign_ship_tasks_and_potential_requirements(&mut admiral, new_ship_tasks);

        upsert_fleets_data(
            Arc::clone(&bmc),
            &Ctx::Anonymous,
            &admiral.fleets,
            &admiral.fleet_tasks,
            &admiral.ship_fleet_assignment,
            &admiral.ship_tasks,
            &admiral.active_trades,
        )
        .await?;

        Ok(admiral)
    }

    pub(crate) fn pure_compute_ship_tasks(
        admiral: &FleetAdmiral,
        facts: &FleetDecisionFacts,
        latest_market_data: Vec<MarketEntry>,
        ship_prices: ShipPriceInfo,
        waypoints: Vec<Waypoint>,
    ) -> Result<Vec<(ShipSymbol, ShipTask, ShipTaskRequirement)>> {
        let mut new_ship_tasks: Vec<(ShipSymbol, ShipTask, ShipTaskRequirement)> = Vec::new();
        for (fleet_id, fleet) in admiral.fleets.clone().iter() {
            match &fleet.cfg {
                FleetConfig::SystemSpawningCfg(cfg) => {
                    let ship_tasks = SystemSpawningFleet::compute_ship_tasks(admiral, cfg, fleet, facts)?;
                    for (ss, task) in ship_tasks {
                        new_ship_tasks.push((ss, task, ShipTaskRequirement::None));
                    }
                }
                FleetConfig::MarketObservationCfg(cfg) => {
                    let ship_tasks = MarketObservationFleet::compute_ship_tasks(admiral, cfg, fleet, facts)?;
                    for (ss, task) in ship_tasks {
                        new_ship_tasks.push((ss, task, ShipTaskRequirement::None));
                    }
                }
                FleetConfig::ConstructJumpGateCfg(cfg) => {
                    let potential_trading_tasks =
                        ConstructJumpGateFleet::compute_ship_tasks(admiral, cfg, fleet, facts, &latest_market_data, &ship_prices, &waypoints)?;

                    for PotentialTradingTask {
                        ship_symbol,
                        trade_ticket,
                        ship_task,
                    } in potential_trading_tasks
                    {
                        new_ship_tasks.push((ship_symbol, ship_task, ShipTaskRequirement::TradeTicket { trade_ticket }));
                    }
                }
                FleetConfig::TradingCfg(cfg) => (),
                FleetConfig::MiningCfg(cfg) => (),
                FleetConfig::SiphoningCfg(cfg) => (),
            }
        }

        Ok(new_ship_tasks)
    }

    pub(crate) async fn compute_ship_tasks(
        admiral: &FleetAdmiral,
        facts: &FleetDecisionFacts,
        bmc: Arc<dyn Bmc>,
    ) -> Result<Vec<(ShipSymbol, ShipTask, ShipTaskRequirement)>> {
        let system_symbol = facts.agent_info.headquarters.system_symbol();

        let waypoints = bmc.system_bmc().get_waypoints_of_system(&Ctx::Anonymous, &system_symbol).await?;
        let ship_prices = bmc.shipyard_bmc().get_latest_ship_prices(&Ctx::Anonymous, &system_symbol).await?;
        let latest_market_data = bmc.market_bmc().get_latest_market_data_for_system(&Ctx::Anonymous, &system_symbol).await?;

        Self::pure_compute_ship_tasks(admiral, facts, latest_market_data, ship_prices, waypoints)
    }

    pub(crate) fn assign_ship_tasks_and_potential_requirements(admiral: &mut FleetAdmiral, ship_tasks: Vec<(ShipSymbol, ShipTask, ShipTaskRequirement)>) {
        for (ship_symbol, ship_task, requirements) in ship_tasks {
            Self::assign_ship_task_and_potential_requirement(admiral, ship_symbol, ship_task, requirements)
        }
    }

    pub fn assign_ship_task_and_potential_requirement(
        admiral: &mut FleetAdmiral,
        ship_symbol: ShipSymbol,
        ship_task: ShipTask,
        requirements: ShipTaskRequirement,
    ) {
        match requirements {
            ShipTaskRequirement::TradeTicket { trade_ticket } => {
                if !(admiral.active_trades.contains_key(&ship_symbol)) {
                    admiral.active_trades.insert(ship_symbol.clone(), trade_ticket);
                    admiral.ship_tasks.insert(ship_symbol.clone(), ship_task);
                } else {
                    event!(
                        Level::WARN,
                        message = "Can't assign new trade_ticket to ship - there's already a trade assigned to it",
                        ship = ship_symbol.0
                    );
                }
            }
            ShipTaskRequirement::None => {
                admiral.ship_tasks.insert(ship_symbol, ship_task);
            }
        }
    }

    pub(crate) fn dismantle_fleets(admiral: &mut FleetAdmiral, fleets_to_dismantle: Vec<FleetId>) {
        for fleet_id in fleets_to_dismantle {
            admiral.mark_fleet_tasks_as_complete(&fleet_id);
            admiral.remove_ships_from_fleet(&fleet_id);
            admiral.fleets.remove(&fleet_id);
        }
    }

    pub(crate) fn remove_ship_from_fleet(admiral: &mut FleetAdmiral, ship_symbol: &ShipSymbol) {
        admiral.ship_fleet_assignment.remove(ship_symbol);
    }

    pub(crate) fn add_stationary_probe_location(admiral: &mut FleetAdmiral, stationary_probe_location: StationaryProbeLocation) {
        admiral.stationary_probe_locations.push(stationary_probe_location);
    }

    pub(crate) fn remove_ship_task(admiral: &mut FleetAdmiral, ship_symbol: &ShipSymbol) {
        admiral.ship_tasks.remove(ship_symbol);
    }

    pub fn assign_ships(
        fleet_tasks: &Vec<(FleetId, FleetTask)>,
        all_ships: &HashMap<ShipSymbol, Ship>,
        fleet_shopping_list: &Vec<(ShipType, FleetTask)>,
    ) -> HashMap<ShipSymbol, FleetId> {
        //TODO: I think this might assign a ship to two fleets if the types match. Make sure to test it
        fleet_tasks
            .iter()
            .flat_map(|(fleet_id, fleet_task)| {
                let desired_fleet_config = fleet_shopping_list.iter().filter(|(st, ft)| ft == fleet_task).map(|(st, _)| st).cloned().collect_vec();

                let mut available_ships = all_ships.clone();
                let assigned_ships_for_fleet = assign_matching_ships(&desired_fleet_config, &mut available_ships);
                assigned_ships_for_fleet.into_iter().map(|(sym, _)| (sym, fleet_id.clone()))
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

    fn mark_fleet_tasks_as_complete(&mut self, fleet_id: &FleetId) {
        let fleet_tasks = self.fleet_tasks.clone();
        if let Some(tasks) = fleet_tasks.get(fleet_id) {
            for task in tasks {
                self.completed_fleet_tasks.push(FleetTaskCompletion {
                    task: task.clone(),
                    completed_at: Utc::now(),
                })
            }
        }
        self.fleet_tasks.remove(fleet_id);
    }

    fn remove_ships_from_fleet(&mut self, fleet_id: &FleetId) {
        let maybe_fleet = self.fleets.get(fleet_id).cloned();
        // borrow checker made me do this
        if let Some(fleet) = maybe_fleet {
            let ship_symbols: Vec<_> = self.get_ships_of_fleet(&fleet).iter().map(|ship| ship.symbol.clone()).collect();

            for symbol in ship_symbols {
                self.ship_fleet_assignment.remove(&symbol);
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Display)]
pub enum NewTaskResult {
    DismantleFleets {
        fleets_to_dismantle: Vec<FleetId>,
    },
    AssignNewTaskToShip {
        ship_symbol: ShipSymbol,
        task: ShipTask,
        ship_task_requirement: ShipTaskRequirement,
    },
    RegisterWaypointForPermanentObservation {
        ship_symbol: ShipSymbol,
        waypoint_symbol: WaypointSymbol,
        exploration_tasks: Vec<ExplorationTask>,
    },
}

pub async fn recompute_tasks_after_ship_finishing_behavior_tree(
    admiral: &FleetAdmiral,
    ship: &Ship,
    finished_task: &ShipTask,
    bmc: Arc<dyn Bmc>,
) -> Result<NewTaskResult> {
    match finished_task {
        ShipTask::MineMaterialsAtWaypoint { .. } => {
            unreachable!("this behavior should run forever")
        }
        ShipTask::SurveyAsteroid { .. } => {
            unreachable!("this behavior should run forever")
        }
        ShipTask::ObserveWaypointDetails { waypoint_symbol } => {
            let waypoints = bmc.system_bmc().get_waypoints_of_system(&Ctx::Anonymous, &waypoint_symbol.system_symbol()).await?;
            let waypoint = waypoints.iter().find(|wp| &wp.symbol == waypoint_symbol).unwrap();
            Ok(NewTaskResult::RegisterWaypointForPermanentObservation {
                ship_symbol: ship.symbol.clone(),
                waypoint_symbol: waypoint_symbol.clone(),
                exploration_tasks: get_exploration_tasks_for_waypoint(waypoint),
            })
        }
        ShipTask::ObserveAllWaypointsOnce { .. } => Ok(NewTaskResult::DismantleFleets {
            fleets_to_dismantle: vec![admiral.ship_fleet_assignment.get(&ship.symbol).unwrap().clone()],
        }),
        ShipTask::Trade { .. } => {
            let facts = collect_fleet_decision_facts(Arc::clone(&bmc), &ship.nav.system_symbol).await?;
            let new_tasks = FleetAdmiral::compute_ship_tasks(admiral, &facts, Arc::clone(&bmc)).await?;
            if let Some((ss, new_task_for_ship, ship_task_requirement)) = new_tasks.iter().find(|(ss, task, _)| ss == &ship.symbol) {
                Ok(NewTaskResult::AssignNewTaskToShip {
                    ship_symbol: ss.clone(),
                    task: new_task_for_ship.clone(),
                    ship_task_requirement: ship_task_requirement.clone(),
                })
            } else {
                Err(anyhow!("No new task for ship found"))
            }
        }
    }
}

pub fn compute_fleet_configs(
    tasks: &[FleetTask],
    fleet_decision_facts: &FleetDecisionFacts,
    shopping_list_in_order: &Vec<(ShipType, FleetTask)>,
) -> Vec<(FleetConfig, FleetTask)> {
    let all_waypoints_of_interest =
        fleet_decision_facts.marketplaces_of_interest.iter().chain(fleet_decision_facts.shipyards_of_interest.iter()).unique().collect_vec();

    tasks
        .into_iter()
        .filter_map(|t| {
            let desired_fleet_config = shopping_list_in_order.iter().filter(|(st, ft)| ft == t).map(|(st, _)| st).cloned().collect_vec();
            let maybe_cfg = match t {
                FleetTask::CollectMarketInfosOnce { system_symbol } => Some(FleetConfig::SystemSpawningCfg(SystemSpawningFleetConfig {
                    system_symbol: system_symbol.clone(),
                    marketplace_waypoints_of_interest: fleet_decision_facts.marketplaces_of_interest.clone(),
                    shipyard_waypoints_of_interest: fleet_decision_facts.shipyards_of_interest.clone(),
                    desired_fleet_config,
                })),
                FleetTask::ObserveAllWaypointsOfSystemWithStationaryProbes { system_symbol } => {
                    Some(FleetConfig::MarketObservationCfg(MarketObservationFleetConfig {
                        system_symbol: system_symbol.clone(),
                        marketplace_waypoints_of_interest: fleet_decision_facts.marketplaces_of_interest.clone(),
                        shipyard_waypoints_of_interest: fleet_decision_facts.shipyards_of_interest.clone(),
                        desired_fleet_config,
                    }))
                }
                FleetTask::ConstructJumpGate { system_symbol } => Some(FleetConfig::ConstructJumpGateCfg(ConstructJumpGateFleetConfig {
                    system_symbol: system_symbol.clone(),
                    jump_gate_waypoint: fleet_decision_facts.construction_site.clone().expect("construction_site").symbol,
                    materialized_supply_chain: None,
                    desired_fleet_config,
                })),
                FleetTask::TradeProfitably { system_symbol } => Some(FleetConfig::TradingCfg(TradingFleetConfig {
                    system_symbol: system_symbol.clone(),
                    materialized_supply_chain: None,
                    desired_fleet_config,
                })),
                FleetTask::MineOres { system_symbol } => Some(FleetConfig::MiningCfg(MiningFleetConfig {
                    system_symbol: system_symbol.clone(),
                    mining_waypoint: WaypointSymbol("TODO add engineered asteroid".to_string()),
                    materials: vec![],
                    delivery_locations: Default::default(),
                    materialized_supply_chain: None,

                    desired_fleet_config,
                })),
                FleetTask::SiphonGases { system_symbol } => Some(FleetConfig::SiphoningCfg(SiphoningFleetConfig {
                    system_symbol: system_symbol.clone(),
                    siphoning_waypoint: WaypointSymbol("TODO add gas giant".to_string()),
                    materials: vec![],
                    delivery_locations: Default::default(),
                    materialized_supply_chain: None,
                    desired_fleet_config,
                })),
            };
            maybe_cfg.map(|cfg| (cfg, t.clone()))
        })
        .collect_vec()
}

pub fn compute_fleet_phase_with_tasks(
    system_symbol: SystemSymbol,
    fleet_decision_facts: &FleetDecisionFacts,
    completed_tasks: &[FleetTaskCompletion],
) -> FleetPhase {
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
        let tasks = [
            CollectMarketInfosOnce {
                system_symbol: system_symbol.clone(),
            },
            ObserveAllWaypointsOfSystemWithStationaryProbes {
                system_symbol: system_symbol.clone(),
            },
        ];

        let frigate_task = tasks[0].clone();
        let probe_task = tasks[1].clone();

        let shopping_list_in_order = vec![
            (ShipType::SHIP_COMMAND_FRIGATE, frigate_task),
            (ShipType::SHIP_PROBE, probe_task),
        ];

        FleetPhase {
            name: FleetPhaseName::InitialExploration,
            shopping_list_in_order,
            tasks: Vec::from(tasks),
        }
    } else if !is_jump_gate_done {
        let tasks = [
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
        ];

        let shipyard_probes = vec![ShipType::SHIP_PROBE].repeat(fleet_decision_facts.shipyards_of_interest.len());
        let construction_fleet = [
            vec![ShipType::SHIP_COMMAND_FRIGATE],
            vec![ShipType::SHIP_LIGHT_HAULER].repeat(4),
        ]
        .concat();

        let mining_fleet = [
            vec![ShipType::SHIP_MINING_DRONE],
            vec![
                ShipType::SHIP_MINING_DRONE,
                ShipType::SHIP_MINING_DRONE,
                ShipType::SHIP_MINING_DRONE,
                ShipType::SHIP_SURVEYOR,
                ShipType::SHIP_LIGHT_HAULER,
            ]
            .repeat(2),
        ]
        .concat();

        let siphoning_fleet = vec![ShipType::SHIP_SIPHON_DRONE].repeat(5);

        let rest_waypoints = diff_waypoint_symbols(&fleet_decision_facts.marketplaces_of_interest, &fleet_decision_facts.shipyards_of_interest);
        let other_probes = vec![ShipType::SHIP_PROBE].repeat(rest_waypoints.len());

        // this is compile-time safe - rust knows the length of arrays and restricts out-of-bounds-access
        let construct_jump_gate_task = tasks[0].clone();
        let probe_observation_task = tasks[1].clone();
        let mining_task = tasks[2].clone();
        let siphoning_task = tasks[3].clone();

        let shopping_list_in_order = shipyard_probes
            .into_iter()
            .map(|ship_type| (ship_type, probe_observation_task.clone()))
            .chain(construction_fleet.into_iter().map(|ship_type| (ship_type, construct_jump_gate_task.clone())))
            .chain(other_probes.into_iter().map(|ship_type| (ship_type, probe_observation_task.clone())))
            .chain(mining_fleet.into_iter().map(|ship_type| (ship_type, mining_task.clone())))
            .chain(siphoning_fleet.into_iter().map(|ship_type| (ship_type, siphoning_task.clone())))
            .collect_vec();

        FleetPhase {
            name: FleetPhaseName::ConstructJumpGate,
            shopping_list_in_order,
            tasks: tasks.into(),
        }
    } else if is_jump_gate_done {
        let tasks = [
            TradeProfitably {
                system_symbol: system_symbol.clone(),
            },
            ObserveAllWaypointsOfSystemWithStationaryProbes {
                system_symbol: system_symbol.clone(),
            },
        ];

        let trade_profitably_task = tasks[0].clone();
        let probe_observation_task = tasks[1].clone();

        let trading_fleet = vec![ShipType::SHIP_LIGHT_HAULER].repeat(4);

        let waypoints_of_interest =
            fleet_decision_facts.marketplaces_of_interest.iter().chain(fleet_decision_facts.shipyards_of_interest.iter()).unique().collect_vec();
        let probe_observation_fleet = vec![ShipType::SHIP_PROBE].repeat(waypoints_of_interest.len());

        let shopping_list_in_order = trading_fleet
            .into_iter()
            .map(|ship_type| (ship_type, probe_observation_task.clone()))
            .chain(probe_observation_fleet.into_iter().map(|ship_type| (ship_type, probe_observation_task.clone())))
            .collect_vec();

        FleetPhase {
            name: FleetPhaseName::TradeProfitably,
            shopping_list_in_order,
            tasks: tasks.into(),
        }
    } else {
        unimplemented!("this shouldn't happen - think harder")
    };

    //     println!(
    //         r#"compute_fleet_tasks:
    // has_construct_jump_gate_task_been_completed: {has_construct_jump_gate_task_been_completed}
    // has_collect_market_infos_once_task_been_completed: {has_collect_market_infos_once_task_been_completed}
    // is_jump_gate_done: {is_jump_gate_done}
    // is_shipyard_exploration_complete: {is_shipyard_exploration_complete}
    // is_marketplace_exploration_complete: {is_marketplace_exploration_complete}
    // has_collected_all_waypoint_details_once: {has_collected_all_waypoint_details_once}
    // tasks: {:?}
    //     "#,
    //         &tasks
    //     );

    tasks
}

fn role_to_ship_type_mapping() -> HashMap<ShipFrameSymbol, ShipType> {
    HashMap::from([
        (ShipFrameSymbol::FRAME_FRIGATE, ShipType::SHIP_COMMAND_FRIGATE),
        (ShipFrameSymbol::FRAME_PROBE, ShipType::SHIP_PROBE),
        (ShipFrameSymbol::FRAME_LIGHT_FREIGHTER, ShipType::SHIP_LIGHT_HAULER),
    ])
}

fn assign_matching_ships(desired_fleet_config: &[ShipType], available_ships: &mut HashMap<ShipSymbol, Ship>) -> HashMap<ShipSymbol, Ship> {
    let mapping: HashMap<ShipFrameSymbol, ShipType> = role_to_ship_type_mapping();

    let mut assigned_ships: Vec<Ship> = vec![];

    for ship_type in desired_fleet_config.iter() {
        let assignable_ships = available_ships
            .iter()
            .filter_map(|(_, s)| {
                let current_ship_type = mapping.get(&s.frame.symbol).expect("role_to_ship_type_mapping");
                (current_ship_type == ship_type).then_some((s.symbol.clone(), current_ship_type.clone(), s.clone()))
            })
            .take(1)
            .collect_vec();

        if assignable_ships.is_empty() {
            break;
        }

        for (assigned_symbol, _, ship) in assignable_ships {
            assigned_ships.push(ship);
            available_ships.remove(&assigned_symbol);
        }
    }
    let ships: HashMap<ShipSymbol, Ship> = assigned_ships.into_iter().map(|ship| (ship.symbol.clone(), ship)).collect();

    ships
}

pub async fn collect_fleet_decision_facts(bmc: Arc<dyn Bmc>, system_symbol: &SystemSymbol) -> Result<FleetDecisionFacts> {
    let ships = bmc.ship_bmc().get_ships(&Ctx::Anonymous, None).await?;
    let agent_info = bmc.agent_bmc().load_agent(&Ctx::Anonymous).await.expect("agent");

    let marketplaces_of_interest: Vec<MarketEntry> = bmc.market_bmc().get_latest_market_data_for_system(&Ctx::Anonymous, &system_symbol).await?;
    let shipyards_of_interest = bmc.shipyard_bmc().get_latest_shipyard_entries_of_system(&Ctx::Anonymous, &system_symbol).await?;

    let marketplace_symbols_of_interest = marketplaces_of_interest.iter().map(|me| me.waypoint_symbol.clone()).collect_vec();
    let marketplaces_to_explore = find_marketplaces_for_exploration(marketplaces_of_interest.clone());

    let shipyard_symbols_of_interest = shipyards_of_interest.iter().map(|db_entry| db_entry.waypoint_symbol.clone()).collect_vec();
    let shipyards_to_explore = find_shipyards_for_exploration(shipyards_of_interest.clone());

    let maybe_construction_site: Option<GetConstructionResponse> =
        bmc.construction_bmc().get_construction_site_for_system(&Ctx::Anonymous, system_symbol.clone()).await?;

    Ok(FleetDecisionFacts {
        marketplaces_of_interest: marketplace_symbols_of_interest.clone(),
        marketplaces_with_up_to_date_infos: diff_waypoint_symbols(&marketplace_symbols_of_interest, &marketplaces_to_explore),
        shipyards_of_interest: shipyard_symbols_of_interest.clone(),
        shipyards_with_up_to_date_infos: diff_waypoint_symbols(&shipyard_symbols_of_interest, &shipyards_to_explore),
        construction_site: maybe_construction_site.map(|resp| resp.data),
        ships,
        materialized_supply_chain: None,
        agent_info,
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

pub fn compute_fleets_with_tasks(
    system_symbol: SystemSymbol,
    completed_tasks: &Vec<FleetTaskCompletion>,
    facts: &FleetDecisionFacts,
    active_fleets: &HashMap<FleetId, Fleet>,
    active_fleet_task_assignments: &HashMap<FleetId, Vec<FleetTask>>,
) -> (Vec<Fleet>, Vec<(FleetId, FleetTask)>, FleetPhase) {
    let fleet_phase = compute_fleet_phase_with_tasks(system_symbol, &facts, &completed_tasks);

    let active_fleets_and_tasks: Vec<(Fleet, (FleetId, FleetTask))> = active_fleets
        .into_iter()
        .map(|(fleet_id, fleet)| {
            let task = active_fleet_task_assignments.get(&fleet_id).cloned().unwrap_or_default().first().cloned().unwrap();
            (fleet.clone(), (fleet_id.clone(), task))
        })
        .collect_vec();

    let new_fleet_configs = compute_fleet_configs(&fleet_phase.tasks, &facts, &fleet_phase.shopping_list_in_order)
        .iter()
        .filter(|(fleet_cfg, fleet_task)| {
            // if we have a fleet already doing the same task, we ignore it
            active_fleets_and_tasks.iter().any(|(_, (_, active_fleet_task))| active_fleet_task == fleet_task).not()
        })
        .cloned()
        .collect_vec();

    let next_fleet_id = active_fleets.keys().into_iter().map(|id| id.0).max().map(|max_id| max_id + 1).unwrap_or(0);

    let new_fleets_with_tasks: Vec<(Fleet, (FleetId, FleetTask))> = new_fleet_configs
        .into_iter()
        .enumerate()
        .map(|(idx, (cfg, task))| create_fleet(cfg.clone(), task.clone(), next_fleet_id + idx as i32).unwrap())
        .collect_vec();

    let (fleets, fleet_tasks): (Vec<Fleet>, Vec<(FleetId, FleetTask)>) = new_fleets_with_tasks.into_iter().chain(active_fleets_and_tasks.into_iter()).unzip();
    (fleets, fleet_tasks, fleet_phase)
}

pub fn create_fleet(super_fleet_config: FleetConfig, fleet_task: FleetTask, id: i32) -> Result<(Fleet, (FleetId, FleetTask))> {
    let id = FleetId(id);
    let mut fleet = Fleet {
        id: id.clone(),
        cfg: super_fleet_config,
    };

    Ok((fleet, (id, fleet_task)))
}

#[derive(Clone, Debug, Serialize, Deserialize, Display)]
pub enum ShipTaskRequirement {
    TradeTicket { trade_ticket: TradeTicket },
    None,
}
