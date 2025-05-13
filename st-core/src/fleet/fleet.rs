use crate::behavior_tree::ship_behaviors::ShipAction;
use crate::fleet::construction_fleet::{ConstructJumpGateFleet, PotentialTradingTask};
use crate::fleet::fleet;
use crate::fleet::fleet_runner::FleetRunner;
use crate::fleet::market_observation_fleet::MarketObservationFleet;
use crate::fleet::supply_chain_test::format_number;
use crate::fleet::system_spawning_fleet::SystemSpawningFleet;
use crate::marketplaces::marketplaces::{find_marketplaces_for_exploration, find_shipyards_for_exploration};
use crate::pagination::fetch_all_pages;
use crate::st_client::StClientTrait;
use anyhow::{anyhow, Result};
use chrono::Utc;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{ContentArrangement, Table};
use itertools::Itertools;
use pathfinding::num_traits::Zero;
use serde::{Deserialize, Serialize};
use st_domain::blackboard_ops::BlackboardOps;
use st_domain::budgeting::budgeting::{
    FinanceError, FleetBudget, FundingSource, TicketStatus, TicketType, TransactionEvent, TransactionGoal, TransactionTicket,
};
use st_domain::budgeting::credits::Credits;
use st_domain::budgeting::treasurer::{InMemoryTreasurer, Treasurer};
use st_domain::FleetConfig::SystemSpawningCfg;
use st_domain::FleetTask::{ConstructJumpGate, InitialExploration, MineOres, ObserveAllWaypointsOfSystemWithStationaryProbes, SiphonGases, TradeProfitably};
use st_domain::ShipRegistrationRole::{Command, Explorer, Interceptor, Refinery, Satellite, Surveyor};
use st_domain::TradeGoodSymbol::{MOUNT_GAS_SIPHON_I, MOUNT_MINING_LASER_I, MOUNT_SURVEYOR_I};
use st_domain::{
    get_exploration_tasks_for_waypoint, trading, Agent, ConstructJumpGateFleetConfig, ExplorationTask, Fleet, FleetConfig, FleetDecisionFacts, FleetId,
    FleetPhase, FleetPhaseName, FleetTask, FleetTaskCompletion, GetConstructionResponse, MarketEntry, MarketObservationFleetConfig, MarketTradeGood,
    MiningFleetConfig, OperationExpenseEvent, PurchaseShipTicketDetails, Ship, ShipFrameSymbol, ShipPriceInfo, ShipRegistrationRole, ShipSymbol, ShipTask,
    ShipType, SiphoningFleetConfig, StationaryProbeLocation, SystemSpawningFleetConfig, SystemSymbol, TicketId, TradeGoodSymbol, TradeTicket,
    TradingFleetConfig, TransactionActionEvent, Waypoint, WaypointSymbol,
};
use st_store::bmc::Bmc;
use st_store::{load_fleet_overview, upsert_fleets_data, Ctx};
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;
use std::ops::Not;
use std::sync::Arc;
use std::time::Duration;
use strum::Display;
use tokio::sync::{Mutex, MutexGuard};
use tracing::{event, Level};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub enum ShipStatusReport {
    ShipActionCompleted(Ship, ShipAction),
    TransactionCompleted(Ship, TransactionActionEvent, TransactionTicket),
    ShipFinishedBehaviorTree(Ship, ShipTask),
    Expense(Ship, OperationExpenseEvent),
}

impl ShipStatusReport {
    pub(crate) fn ship_symbol(&self) -> ShipSymbol {
        match self {
            ShipStatusReport::ShipActionCompleted(s, _) => s.symbol.clone(),
            ShipStatusReport::TransactionCompleted(s, _, _) => s.symbol.clone(),
            ShipStatusReport::ShipFinishedBehaviorTree(s, _) => s.symbol.clone(),
            ShipStatusReport::Expense(s, _) => s.symbol.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FleetAdmiral {
    pub completed_fleet_tasks: Vec<FleetTaskCompletion>,
    pub fleets: HashMap<FleetId, Fleet>,
    pub all_ships: HashMap<ShipSymbol, Ship>,
    pub ship_tasks: HashMap<ShipSymbol, ShipTask>,
    pub fleet_tasks: HashMap<FleetId, Vec<FleetTask>>,
    pub ship_fleet_assignment: HashMap<ShipSymbol, FleetId>,
    pub fleet_phase: FleetPhase,
    pub active_trade_ids: HashMap<ShipSymbol, TicketId>,
    pub stationary_probe_locations: Vec<StationaryProbeLocation>,
    pub treasurer: Arc<Mutex<InMemoryTreasurer>>,
    pub ship_purchase_demand: VecDeque<(ShipType, FleetTask)>,
}

impl FleetAdmiral {
    async fn generate_state_overview(&self) -> String {
        format!(
            r#"
==================================================================            
==                            Fleets                            ==
==================================================================            
        
{}

==================================================================            
==                            Budgets                           ==
==================================================================            
        
{}

==================================================================            
==                             Ships                            ==
==================================================================            
        
{}


        "#,
            self.generate_fleet_table(),
            self.generate_budgets_overview().await,
            self.generate_ships_table(),
        )
    }

    fn generate_fleet_table(&self) -> String {
        let mut table = Table::new();
        table
            .load_preset(UTF8_FULL)
            .force_no_tty()
            .enforce_styling()
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec!["Fleet Id", "Fleet Cfg"]);

        for (fleet_id, fleet) in self.fleets.iter().sorted_by_key(|(id, _)| id.0) {
            table.add_row(vec![fleet_id.0.to_string().as_str(), fleet.cfg.to_string().as_str()]);
        }

        table.to_string()
    }

    async fn generate_budgets_overview(&self) -> String {
        let mut budget_table = Table::new();
        budget_table
            .load_preset(UTF8_FULL)
            .force_no_tty()
            .enforce_styling()
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec![
                "Fleet Id",
                "Fleet Cfg",
                "Available Capital",
                "Total Capital",
                "Operating Reserve",
                "Asset Value",
            ]);

        for (fleet_id, budget) in self
            .get_fleet_budgets()
            .await
            .iter()
            .sorted_by_key(|(id, _)| id.0)
        {
            let fleet = self.fleets.get(fleet_id).unwrap();

            budget_table.add_row(vec![
                fleet_id.0.to_string().as_str(),
                fleet.cfg.to_string().as_str(),
                format_number(budget.available_capital.0 as f64).as_str(),
                format_number(budget.total_capital.0 as f64).as_str(),
                format_number(budget.operating_reserve.0 as f64).as_str(),
                format_number(budget.asset_value.0 as f64).as_str(),
            ]);
        }

        budget_table.add_row(vec![
            "---",
            "Treasury",
            format_number(self.treasurer.lock().await.treasury.0 as f64).as_str(),
            "---",
            "---",
            "---",
        ]);

        // tickets

        let mut tickets_table = Table::new();
        tickets_table
            .load_preset(UTF8_FULL)
            .force_no_tty()
            .enforce_styling()
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec![
                "id",
                "status",
                "initiating_fleet",
                "executing_fleet",
                "beneficiary_fleet",
                "executing_vessel",
                "required_capital",
                "allocated_capital",
            ]);

        for (_, ticket) in self.get_treasurer_tickets().await.iter() {
            tickets_table.add_row(vec![
                ticket.id.0.to_string().as_str(),
                ticket.status.to_string().as_str(),
                ticket.initiating_fleet.0.to_string().as_str(),
                ticket.executing_fleet.0.to_string().as_str(),
                ticket.beneficiary_fleet.0.to_string().as_str(),
                ticket.executing_vessel.0.to_string().as_str(),
                format_number(ticket.financials.required_capital.0 as f64).as_str(),
                format_number(ticket.financials.allocated_capital.0 as f64).as_str(),
            ]);
        }

        format!("Budget Table\n{}\n\nTicket Table\n{}", budget_table.to_string(), tickets_table.to_string())
    }

    fn generate_ships_table(&self) -> String {
        let mut table = Table::new();
        table
            .load_preset(UTF8_FULL)
            .force_no_tty()
            .enforce_styling()
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec![
                "Fleet Id",
                "Fleet Cfg",
                "ship_symbol",
                "frame",
                "cargo units",
                "cargo capacity",
                "ship_task",
            ]);

        for (ship_symbol, ship) in self.all_ships.iter().sorted_by_key(|(id, _)| id.0.clone()) {
            let ship_task = self
                .ship_tasks
                .get(ship_symbol)
                .map(|t| t.to_string())
                .unwrap_or("---".to_string());
            let fleet_id = self.ship_fleet_assignment.get(ship_symbol).unwrap();
            let fleet = self.fleets.get(fleet_id).unwrap();

            table.add_row(vec![
                fleet_id.0.to_string().as_str(),
                fleet.cfg.to_string().as_str(),
                ship_symbol.0.as_str(),
                ship.frame.symbol.to_string().as_str(),
                ship.cargo.units.to_string().as_str(),
                ship.cargo.capacity.to_string().as_str(),
                ship_task.as_str(),
            ]);
        }

        table.to_string()
    }

    pub(crate) async fn get_trades_of_fleet(&self, fleet: &Fleet) -> Vec<TransactionTicket> {
        let treasurer = self.treasurer.lock().await;
        self.ship_fleet_assignment
            .iter()
            .filter_map(|(ship_symbol, fleet_id)| {
                if fleet_id == &fleet.id {
                    self.active_trade_ids
                        .get(ship_symbol)
                        .and_then(|ticket_id| treasurer.get_ticket(ticket_id.clone()).ok())
                } else {
                    None
                }
            })
            .collect_vec()
    }

    pub(crate) async fn get_fleet_trades_overview(&self) -> HashMap<FleetId, Vec<TransactionTicket>> {
        let treasurer = self.treasurer.lock().await;
        treasurer.get_fleet_trades_overview()
    }

    pub(crate) async fn get_fleet_budgets(&self) -> HashMap<FleetId, FleetBudget> {
        let treasurer = self.treasurer.lock().await;
        treasurer.get_fleet_budgets()
    }

    pub(crate) async fn get_treasurer_tickets(&self) -> HashMap<TicketId, TransactionTicket> {
        let treasurer = self.treasurer.lock().await;
        treasurer.tickets.clone()
    }

    pub fn get_next_ship_purchase(&self) -> Option<(ShipType, FleetTask)> {
        self.ship_purchase_demand.iter().peekable().next().cloned()
    }

    pub(crate) fn get_ship_tasks_of_fleet(&self, fleet: &Fleet) -> Vec<(ShipSymbol, ShipTask)> {
        let tasks = self
            .get_ships_of_fleet(fleet)
            .iter()
            .flat_map(|ss| {
                self.get_task_of_ship(&ss.symbol)
                    .map(|st| (ss.symbol.clone(), st.clone()))
                    .into_iter()
            })
            .collect_vec();
        tasks
    }

    pub async fn run_fleets(fleet_admiral: Arc<Mutex<FleetAdmiral>>, client: Arc<dyn StClientTrait>, bmc: Arc<dyn Bmc>) -> Result<()> {
        event!(Level::INFO, "Running fleets");

        FleetRunner::run_fleets(Arc::clone(&fleet_admiral), Arc::clone(&client), bmc, Duration::from_secs(5)).await?;

        Ok(())
    }

    pub async fn report_ship_action_completed(&mut self, ship_status_report: &ShipStatusReport, bmc: Arc<dyn Bmc>) -> Result<()> {
        match ship_status_report {
            ShipStatusReport::ShipActionCompleted(ship, ship_action) => {
                let maybe_fleet = self.get_fleet_of_ship(&ship.symbol);
                let fleet_tasks: Vec<FleetTask> = maybe_fleet
                    .map(|fleet_id| self.get_tasks_of_fleet(&fleet_id.id))
                    .unwrap_or_default();
                let maybe_ship_task = self.get_task_of_ship(&ship.symbol);
                if let Some((fleet, ship_task)) = maybe_fleet.zip(maybe_ship_task) {
                    let fleet_decision_facts: FleetDecisionFacts = collect_fleet_decision_facts(Arc::clone(&bmc), &ship.nav.system_symbol).await?;
                    match &fleet.cfg {
                        SystemSpawningCfg(cfg) => {
                            if let Some(task_complete) =
                                SystemSpawningFleet::check_for_task_completion(ship_task, fleet, &fleet_tasks, cfg, &fleet_decision_facts)
                            {
                                let uncompleted_tasks = fleet_tasks
                                    .iter()
                                    .filter(|&ft| ft != &task_complete.task)
                                    .cloned()
                                    .collect_vec();

                                event!(
                                    Level::INFO,
                                    message = "FleetTaskCompleted",
                                    ship = ship.symbol.0,
                                    fleet_id = fleet.id.0,
                                    task = task_complete.task.to_string()
                                );
                                self.fleet_tasks.insert(fleet.id.clone(), uncompleted_tasks);
                                self.completed_fleet_tasks.push(task_complete.clone());
                                bmc.fleet_bmc()
                                    .save_completed_fleet_task(&Ctx::Anonymous, &task_complete)
                                    .await?;
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
            ShipStatusReport::Expense(ship, operation_expense_event) => {
                let fleet_id = self.ship_fleet_assignment.get(&ship.symbol).unwrap();

                self.report_expense_transaction_to_treasurer(fleet_id, operation_expense_event)
                    .await;

                let agent_credits_from_response = match operation_expense_event {
                    OperationExpenseEvent::RefueledShip { response } => response.data.agent.credits,
                };
                let total_price = match operation_expense_event {
                    OperationExpenseEvent::RefueledShip { response } => response.data.transaction.total_price,
                };
                let new_credits = self.agent_info_credits().await.0;

                if agent_credits_from_response != new_credits {
                    event!(
                        Level::WARN,
                            "Agent Credits differ from our expectation!\nExpected Agent Credits: {new_credits}\n Actual Agent Credits: {agent_credits_from_response}"
                              );
                }

                // fixme: store agent_credits
                // bmc.agent_bmc()
                //     .store_agent(&Ctx::Anonymous, &self.agent_info)
                //     .await?;

                event!(Level::INFO, "Refueled ship. Total Price: {}; New Agent Credits: {}", &total_price, new_credits,);

                Ok(())
            }

            ShipStatusReport::TransactionCompleted(ship, transaction_event, updated_trade_ticket) => {
                self.mark_transaction_completed_to_treasurer(transaction_event, &updated_trade_ticket, &ship.symbol)
                    .await;

                let is_complete = updated_trade_ticket.is_complete();
                bmc.trade_bmc()
                    .upsert_ticket(&Ctx::Anonymous, &ship.symbol, &updated_trade_ticket.id, updated_trade_ticket, is_complete)
                    .await?;

                //FIXME: agent-credits need to update

                Ok(())
            }
            ShipStatusReport::ShipFinishedBehaviorTree(ship, task) => {
                event!(
                    Level::INFO,
                    message = "Ship finished behavior tree",
                    ship = ship.symbol.0,
                    task = task.to_string(),
                );
                Ok(())
            }
        }
    }

    pub fn get_fleet_of_ship(&self, ship_symbol: &ShipSymbol) -> Option<&Fleet> {
        self.ship_fleet_assignment
            .get(ship_symbol)
            .and_then(|fleet_id| self.fleets.get(fleet_id))
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
        bmc.agent_bmc()
            .store_agent(&Ctx::Anonymous, &agent.data)
            .await?;

        match Self::load_admiral(Arc::clone(&bmc)).await? {
            None => {
                println!("loading admiral failed - creating a new one");
                let admiral = Self::create(Arc::clone(&bmc), system_symbol, Arc::clone(&client)).await?;
                upsert_fleets_data(
                    Arc::clone(&bmc),
                    &Ctx::Anonymous,
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

    pub async fn initialize_treasurer(
        bmc: Arc<dyn Bmc>,
        fleet_phase: &FleetPhase,
        ship_fleet_assignment: &HashMap<ShipSymbol, FleetId>,
        all_ships: &HashMap<ShipSymbol, Ship>,
        fleets: &HashMap<FleetId, Fleet>,
        fleet_tasks: &HashMap<FleetId, Vec<FleetTask>>,
    ) -> Result<InMemoryTreasurer> {
        let agent_info = bmc.agent_bmc().load_agent(&Ctx::Anonymous).await?;
        let system_symbol = agent_info.headquarters.system_symbol();
        let facts = collect_fleet_decision_facts(bmc.clone(), &system_symbol).await?;

        let ship_price_info = bmc
            .shipyard_bmc()
            .get_latest_ship_prices(&Ctx::Anonymous, &system_symbol)
            .await?;
        let all_next_ship_purchases = get_all_next_ship_purchases(&all_ships, &fleet_phase);

        let fleets = fleets.values().cloned().collect_vec();
        let fleet_tasks = fleet_tasks
            .iter()
            .map(|(fleet_id, tasks)| (fleet_id.clone(), tasks.first().cloned().unwrap()))
            .collect_vec();

        let mut treasurer = InMemoryTreasurer::new(agent_info.credits.into());
        treasurer.redistribute_distribute_fleet_budgets(&fleet_phase, &fleet_tasks, &ship_fleet_assignment, &ship_price_info, &all_next_ship_purchases)?;

        Ok(treasurer)
    }

    async fn load_admiral(bmc: Arc<dyn Bmc>) -> Result<Option<Self>> {
        let overview = load_fleet_overview(Arc::clone(&bmc), &Ctx::Anonymous).await?;

        if overview.fleets.is_empty() || overview.all_ships.is_empty() {
            Ok(None)
        } else {
            // fixme: needs to be aware of multiple systems
            let all_ships = overview.all_ships.values().cloned().collect_vec();
            let ship_map: HashMap<ShipSymbol, Ship> = all_ships
                .iter()
                .map(|s| (s.symbol.clone(), s.clone()))
                .collect();

            let system_symbol = all_ships.first().cloned().unwrap().nav.system_symbol;

            // recompute ship-tasks and persist them. Might have been outdated since last agent restart
            let facts = collect_fleet_decision_facts(Arc::clone(&bmc), &system_symbol).await?;
            let fleet_phase = compute_fleet_phase_with_tasks(system_symbol.clone(), &facts, &overview.completed_fleet_tasks);
            let (fleets, fleet_tasks) = compute_fleets_with_tasks(&facts, &overview.fleets, &overview.fleet_task_assignments, &fleet_phase);
            let fleet_map: HashMap<FleetId, Fleet> = fleets.iter().map(|f| (f.id.clone(), f.clone())).collect();
            let fleet_task_map: HashMap<FleetId, Vec<FleetTask>> = fleet_tasks
                .iter()
                .map(|(fleet_id, task)| (fleet_id.clone(), vec![task.clone()]))
                .collect();

            let ship_fleet_assignment = Self::assign_ships(&fleet_tasks, &ship_map, &fleet_phase.shopping_list_in_order);

            let agent_info = bmc.agent_bmc().load_agent(&Ctx::Anonymous).await?;

            let ships = overview
                .all_ships
                .into_iter()
                .filter(|(ss, ship)| {
                    overview
                        .stationary_probe_locations
                        .iter()
                        .any(|spl| ss == &spl.probe_ship_symbol)
                        .not()
                })
                .collect();

            let treasurer = Self::initialize_treasurer(bmc.clone(), &fleet_phase, &ship_fleet_assignment, &ships, &fleet_map, &fleet_task_map).await?;
            let current_ship_demands = get_all_next_ship_purchases(&ship_map, &fleet_phase);

            let mut admiral = Self {
                completed_fleet_tasks: overview.completed_fleet_tasks.clone(),
                fleets: fleet_map,
                all_ships: ships,
                ship_tasks: overview.ship_tasks,
                fleet_tasks: fleet_task_map,
                ship_fleet_assignment,
                fleet_phase,
                //FIXME
                active_trade_ids: Default::default(),
                stationary_probe_locations: overview.stationary_probe_locations,
                treasurer: Arc::new(Mutex::new(treasurer)),
                ship_purchase_demand: VecDeque::from(current_ship_demands),
            };

            // let new_ship_tasks = Self::compute_ship_tasks(&mut admiral, &facts, Arc::clone(&bmc)).await?;
            // Self::assign_ship_tasks(&mut admiral, new_ship_tasks);

            upsert_fleets_data(
                Arc::clone(&bmc),
                &Ctx::Anonymous,
                &admiral.fleets,
                &admiral.fleet_tasks,
                &admiral.ship_fleet_assignment,
                &admiral.ship_tasks,
            )
            .await?;

            Ok(Some(admiral))
        }
    }

    pub async fn create(bmc: Arc<dyn Bmc>, system_symbol: SystemSymbol, client: Arc<dyn StClientTrait>) -> Result<Self> {
        let ships = bmc.ship_bmc().get_ships(&Ctx::Anonymous, None).await?;
        let stationary_probe_locations = bmc
            .ship_bmc()
            .get_stationary_probes(&Ctx::Anonymous)
            .await?;

        let facts = collect_fleet_decision_facts(Arc::clone(&bmc), &system_symbol).await?;

        let ships = if ships.is_empty() {
            let ships: Vec<Ship> = fetch_all_pages(|p| client.list_ships(p)).await?;
            bmc.ship_bmc()
                .upsert_ships(&Ctx::Anonymous, &ships, Utc::now())
                .await?;
            ships
        } else {
            ships
        };

        let non_probe_ships = ships
            .iter()
            .filter(|ship| {
                stationary_probe_locations
                    .iter()
                    .any(|spl| ship.symbol == spl.probe_ship_symbol)
                    .not()
            })
            .cloned()
            .collect_vec();

        let ship_map: HashMap<ShipSymbol, Ship> = non_probe_ships
            .into_iter()
            .map(|s| (s.symbol.clone(), s))
            .collect();

        let completed_tasks = bmc
            .fleet_bmc()
            .load_completed_fleet_tasks(&Ctx::Anonymous)
            .await?;

        let fleet_phase = compute_fleet_phase_with_tasks(system_symbol.clone(), &facts, &completed_tasks);

        let (fleets, fleet_tasks) = compute_fleets_with_tasks(&facts, &HashMap::new(), &HashMap::new(), &fleet_phase);

        let ship_fleet_assignment = Self::assign_ships(&fleet_tasks, &ship_map, &fleet_phase.shopping_list_in_order);

        let fleet_map: HashMap<FleetId, Fleet> = fleets.iter().map(|f| (f.id.clone(), f.clone())).collect();
        let fleet_task_map: HashMap<FleetId, Vec<FleetTask>> = fleet_tasks
            .iter()
            .cloned()
            .map(|(fleet_id, task)| (fleet_id, vec![task]))
            .collect();

        let agent_info = client.get_agent().await?.data;
        bmc.agent_bmc()
            .store_agent(&Ctx::Anonymous, &agent_info)
            .await?;

        let treasurer = Self::initialize_treasurer(bmc.clone(), &fleet_phase, &ship_fleet_assignment, &ship_map, &fleet_map, &fleet_task_map).await?;

        let current_ship_demands = get_all_next_ship_purchases(&ship_map, &fleet_phase);

        let ship_prices = bmc
            .shipyard_bmc()
            .get_latest_ship_prices(&Ctx::Anonymous, &system_symbol)
            .await?;

        let mut admiral = Self {
            completed_fleet_tasks: completed_tasks,
            fleets: fleet_map,
            all_ships: ship_map,
            fleet_tasks: fleet_task_map,
            ship_tasks: Default::default(),
            ship_fleet_assignment,
            fleet_phase,
            active_trade_ids: Default::default(),
            stationary_probe_locations,
            treasurer: Arc::new(Mutex::new(treasurer)),
            ship_purchase_demand: VecDeque::from(current_ship_demands),
        };

        upsert_fleets_data(
            Arc::clone(&bmc),
            &Ctx::Anonymous,
            &admiral.fleets,
            &admiral.fleet_tasks,
            &admiral.ship_fleet_assignment,
            &admiral.ship_tasks,
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
        ship_purchase_tickets: &[TransactionTicket],
        fleet_trades: &HashMap<FleetId, Vec<TransactionTicket>>,
        fleet_budgets: &HashMap<FleetId, FleetBudget>,
        treasurer: &mut InMemoryTreasurer,
    ) -> Result<Vec<(ShipSymbol, ShipTask)>> {
        let mut new_ship_tasks: HashMap<ShipSymbol, ShipTask> = HashMap::new();

        for (fleet_id, fleet) in admiral.fleets.clone().iter() {
            let ships_of_fleet: Vec<&Ship> = admiral.get_ships_of_fleet(fleet);

            let trades_of_fleet = fleet_trades.get(&fleet_id).cloned().unwrap_or_default();
            let fleet_budget = fleet_budgets.get(&fleet_id).cloned().expect("fleet budget");

            // assign ship tasks for ship purchases if necessary
            for ship in ships_of_fleet.iter() {
                // ship has no task
                let ship_has_no_active_task = admiral.ship_tasks.contains_key(&ship.symbol).not();

                if ship_has_no_active_task {
                    // we have a ship purchase ticket with this ship assigned
                    if let Some(ship_purchase_ticket) = ship_purchase_tickets
                        .iter()
                        .find(|t| t.executing_vessel == ship.symbol && t.status == TicketStatus::Funded)
                    {
                        new_ship_tasks.insert(
                            ship.symbol.clone(),
                            ShipTask::Trade {
                                ticket_id: ship_purchase_ticket.id.clone(),
                            },
                        );
                    }
                }
            }

            let unassigned_ships_of_fleet = ships_of_fleet
                .iter()
                .filter(|s| {
                    let has_new_task = new_ship_tasks.contains_key(&s.symbol);
                    let has_already_assigned_task = admiral.ship_tasks.contains_key(&s.symbol);
                    has_new_task.not() && has_already_assigned_task.not()
                })
                .cloned()
                .collect_vec();

            match &fleet.cfg {
                FleetConfig::SystemSpawningCfg(cfg) => {
                    let ship_tasks = SystemSpawningFleet::compute_ship_tasks(admiral, cfg, fleet, facts, &unassigned_ships_of_fleet)?;
                    for (ss, task) in ship_tasks {
                        new_ship_tasks.insert(ss, task);
                    }
                }
                FleetConfig::MarketObservationCfg(cfg) => {
                    let ship_tasks = MarketObservationFleet::compute_ship_tasks(admiral, cfg, &unassigned_ships_of_fleet)?;
                    for (ss, task) in ship_tasks {
                        new_ship_tasks.insert(ss, task);
                    }
                }
                FleetConfig::ConstructJumpGateCfg(cfg) => {
                    let potential_trading_tasks = ConstructJumpGateFleet::compute_ship_tasks(
                        admiral,
                        cfg,
                        fleet,
                        facts,
                        &latest_market_data,
                        &ship_prices,
                        &waypoints,
                        &unassigned_ships_of_fleet,
                        &trades_of_fleet,
                        &fleet_budget,
                    )?;

                    for (potential_trading_task) in potential_trading_tasks {
                        let ticket_id = treasurer.create_ticket(
                            TicketType::Trading,
                            potential_trading_task.ship_symbol.clone(),
                            &fleet_id,
                            &fleet_id,
                            &fleet_id,
                            potential_trading_task.to_trading_goals(),
                            Utc::now(),
                            1.0,
                        )?;

                        if let Err(e) = treasurer.fund_ticket(
                            ticket_id.clone(),
                            FundingSource {
                                source_fleet: fleet_id.clone(),
                                amount: potential_trading_task.total_purchase_price(),
                            },
                        ) {
                            eprintln!("Unable to fund new trading ticket. Reason: {e:?}")
                            // FIXME - remove ticket
                        }

                        new_ship_tasks.insert(potential_trading_task.ship_symbol.clone(), ShipTask::Trade { ticket_id });
                    }
                }
                FleetConfig::TradingCfg(cfg) => (),
                FleetConfig::MiningCfg(cfg) => (),
                FleetConfig::SiphoningCfg(cfg) => (),
            }
        }

        if new_ship_tasks.is_empty() {
            event!(Level::WARN, message = "no new tasks for ships computed - this should not happen");
        }

        Ok(new_ship_tasks.into_iter().collect_vec())
    }

    pub(crate) async fn compute_ship_tasks(admiral: &mut FleetAdmiral, facts: &FleetDecisionFacts, bmc: Arc<dyn Bmc>) -> Result<Vec<(ShipSymbol, ShipTask)>> {
        let system_symbol = facts.agent_info.headquarters.system_symbol();

        let waypoints = bmc
            .system_bmc()
            .get_waypoints_of_system(&Ctx::Anonymous, &system_symbol)
            .await?;

        let ship_prices = bmc
            .shipyard_bmc()
            .get_latest_ship_prices(&Ctx::Anonymous, &system_symbol)
            .await?;

        let latest_market_data = bmc
            .market_bmc()
            .get_latest_market_data_for_system(&Ctx::Anonymous, &system_symbol)
            .await?;

        admiral.try_create_ship_purchase_ticket(&ship_prices).await;

        let fleet_trades = admiral.get_fleet_trades_overview().await;
        let fleet_budgets = admiral.get_fleet_budgets().await;

        let new_tasks = {
            let mut treasurer_guard = admiral.treasurer.lock().await;

            let ship_purchase_tickets = treasurer_guard.get_ship_purchase_tickets();

            Self::pure_compute_ship_tasks(
                admiral,
                facts,
                latest_market_data,
                ship_prices,
                waypoints,
                &ship_purchase_tickets,
                &fleet_trades,
                &fleet_budgets,
                &mut treasurer_guard,
            )?
        };

        if new_tasks.is_empty() {
            let overview = admiral.generate_state_overview().await;
            println!("No new tasks calculated. Current overview: \n{}", overview);
        }
        Ok(new_tasks)
    }

    pub(crate) fn assign_ship_tasks(admiral: &mut FleetAdmiral, ship_tasks: Vec<(ShipSymbol, ShipTask)>) -> () {
        for (ship_symbol, ship_task) in ship_tasks {
            admiral.ship_tasks.insert(ship_symbol, ship_task);
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
        admiral
            .stationary_probe_locations
            .push(stationary_probe_location);
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
                let desired_fleet_config = fleet_shopping_list
                    .iter()
                    .filter(|(st, ft)| ft == fleet_task)
                    .map(|(st, _)| st)
                    .cloned()
                    .collect_vec();

                let mut available_ships = all_ships.clone();
                let assigned_ships_for_fleet = assign_matching_ships(&desired_fleet_config, &mut available_ships);
                assigned_ships_for_fleet
                    .into_keys()
                    .map(|sym| (sym, fleet_id.clone()))
            })
            .collect::<HashMap<_, _>>()
    }

    pub(crate) fn get_ships_of_fleet(&self, fleet: &Fleet) -> Vec<&Ship> {
        self.ship_fleet_assignment
            .iter()
            .filter_map(|(ship_symbol, fleet_id)| {
                if fleet_id == &fleet.id {
                    self.all_ships.get(ship_symbol)
                } else {
                    None
                }
            })
            .collect_vec()
    }

    pub(crate) fn calc_required_operating_capital_for_fleet(&self, fleet: &Fleet, ships: &[&Ship]) -> Credits {
        let num_fuel_consuming_ships = ships.iter().filter(|s| s.fuel.capacity > 0).count() as u32;
        let num_trading_ships = ships.iter().filter(|s| s.cargo.capacity > 0).count() as u32;

        let required_fuel_budget = Credits::new(1_000) * num_fuel_consuming_ships;
        let required_trading_budget = Credits::new(75_000) * num_trading_ships;

        let operating_capital = match fleet.cfg {
            FleetConfig::TradingCfg(_) => required_fuel_budget + required_trading_budget,
            FleetConfig::ConstructJumpGateCfg(_) => required_fuel_budget + required_trading_budget,
            FleetConfig::MiningCfg(_) => required_fuel_budget,
            FleetConfig::SiphoningCfg(_) => required_fuel_budget,
            FleetConfig::MarketObservationCfg(_) => required_fuel_budget,
            SystemSpawningCfg(_) => required_fuel_budget,
        };
        operating_capital
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
            let ship_symbols: Vec<_> = self
                .get_ships_of_fleet(&fleet)
                .iter()
                .map(|ship| ship.symbol.clone())
                .collect();

            for symbol in ship_symbols {
                self.ship_fleet_assignment.remove(&symbol);
            }
        }
    }

    async fn try_create_ship_purchase_ticket(&mut self, ship_prices: &ShipPriceInfo) -> () {
        //println!("Overview before creation of ship purchase ticket:\n{}", self.generate_state_overview().await);
        match self.create_ship_purchase_ticket(ship_prices).await {
            Ok(_) => {}
            Err(_) => {}
        }
        //println!("Overview after creation of ship purchase ticket:\n{}", self.generate_state_overview().await);
    }

    async fn create_ship_purchase_ticket(&mut self, ship_prices: &ShipPriceInfo) -> Result<()> {
        let mut treasurer = self.treasurer.lock().await;

        let (ship_type, fleet_task) = self
            .ship_purchase_demand
            .pop_front()
            .ok_or(anyhow!("No ship purchase demands available"))?;

        let maybe_existing_ship_purchase_ticket = treasurer
            .get_ship_purchase_tickets()
            .iter()
            .find_map(|t| match t.ticket_type {
                TicketType::ShipPurchase => t.get_incomplete_goals().iter().find_map(|g| match g {
                    TransactionGoal::PurchaseShip(p) => (p.ship_type == ship_type).then_some(t.clone()),
                    _ => None,
                }),
                _ => None,
            });

        if let Some(ticket) = maybe_existing_ship_purchase_ticket {
            // put ticket back - we are already purchasing a ticket (might not have completed funding yet)
            self.ship_purchase_demand
                .push_front((ship_type.clone(), fleet_task));

            if ticket.status == TicketStatus::Funded {
                event!(
                    Level::INFO,
                    message = "There's already a funded ship purchase for this ship_type. No Op",
                    ship_type = ship_type.to_string(),
                );
            } else {
                let funding_source = FundingSource {
                    source_fleet: ticket.initiating_fleet.clone(),
                    amount: ticket.financials.required_capital - ticket.financials.allocated_capital,
                };
                treasurer.try_fund_fleet_and_ticket(funding_source.clone(), ticket.id.clone())?;
            }

            return Ok(());
        }

        let purchasing_ship = self
            .get_ship_purchaser(&ship_type, &fleet_task, ship_prices)
            .ok_or(anyhow!("No suitable purchasing ship found for {ship_type}"))?;

        let beneficiary_fleet = self
            .get_fleet_executing_fleet_task(&fleet_task)
            .ok_or(anyhow!("No fleet found executing task {fleet_task:?}"))?;

        let initiating_fleet = beneficiary_fleet.clone();

        let executing_fleet = self
            .get_fleet_of_ship(&purchasing_ship)
            .map(|f| f.id.clone())
            .ok_or(anyhow!("Ship {} not assigned to any fleet", purchasing_ship))?;

        let (_, (shipyard_wps, price)) = ship_prices
            .get_best_purchase_location(&ship_type)
            .ok_or(anyhow!("No shipyard found selling {ship_type}"))?;

        let ship_price = ((price as f64 * 1.02) as i64).into();
        let funding_source = FundingSource {
            source_fleet: beneficiary_fleet.clone(),
            amount: ship_price,
        };

        let funding_result: Result<TicketId, FinanceError> = {
            let ticket_id: TicketId = treasurer.create_ship_purchase_ticket(
                &ship_type,
                &purchasing_ship,
                &initiating_fleet,
                &beneficiary_fleet,
                &executing_fleet,
                ship_price,
                &shipyard_wps,
            )?;

            treasurer.try_fund_fleet_and_ticket(funding_source.clone(), ticket_id.clone())?;
            Ok(ticket_id)
        };

        match funding_result {
            Ok(ticket_id) => {
                event!(
                    Level::INFO,
                    message = "Funded ship purchase",
                    ship_type = ship_type.to_string(),
                    purchasing_ship = purchasing_ship.0,
                    beneficiary_fleet = beneficiary_fleet.0,
                    executing_fleet = executing_fleet.0,
                    shipyard_wps = shipyard_wps.0,
                    price = ship_price.0,
                    ticket_id = ticket_id.0.to_string(),
                );
                Ok(())
            }
            Err(err) => {
                self.ship_purchase_demand
                    .push_front((ship_type, fleet_task));
                event!(
                    Level::INFO,
                    message = "Unable to fund ship purchase - removing ticket again.",
                    error = err.to_string(),
                    ship_type = ship_type.to_string(),
                    purchasing_ship = purchasing_ship.0,
                    beneficiary_fleet = beneficiary_fleet.0,
                    executing_fleet = executing_fleet.0,
                    shipyard_wps = shipyard_wps.0,
                    price = ship_price.0,
                );

                Err(anyhow!(err))
            }
        }
    }

    fn get_ship_purchaser(&self, ship_type: &ShipType, for_fleet_task: &FleetTask, ship_prices: &ShipPriceInfo) -> Option<ShipSymbol> {
        let system_symbol = match for_fleet_task {
            FleetTask::InitialExploration { system_symbol } => system_symbol,
            FleetTask::ObserveAllWaypointsOfSystemWithStationaryProbes { system_symbol } => system_symbol,
            FleetTask::ConstructJumpGate { system_symbol } => system_symbol,
            FleetTask::TradeProfitably { system_symbol } => system_symbol,
            FleetTask::MineOres { system_symbol } => system_symbol,
            FleetTask::SiphonGases { system_symbol } => system_symbol,
        };

        let purchase_map: Vec<(ShipType, WaypointSymbol, u32, u32)> = ship_prices.get_running_total_of_all_ship_purchases(vec![ship_type.clone()]);

        let maybe_result = if let Some(&(_, ref wps, price, _)) = purchase_map.first() {
            let purchase_candidates = self
                .stationary_probe_locations
                .iter()
                .find(|spl| spl.waypoint_symbol == wps.clone())
                .map(|spl| spl.probe_ship_symbol.clone())
                .or_else(|| {
                    self.find_spawning_ship_for_system(system_symbol)
                        .or_else(|| {
                            self.all_ships
                                .values()
                                .find(|s| s.frame.symbol == ShipFrameSymbol::FRAME_FRIGATE)
                                .map(|s| s.symbol.clone())
                        })
                })
                .into_iter()
                .collect_vec();

            purchase_candidates.first().cloned()
        } else {
            None
        };

        match maybe_result {
            None => {
                event!(Level::WARN, "unable to find ship_purchaser. This should not happen.");
                None
            }
            Some(result) => Some(result),
        }
    }

    fn find_spawning_ship_for_system(&self, spawning_system_symbol: &SystemSymbol) -> Option<ShipSymbol> {
        if let Some((fleet_id, _)) = self.fleet_tasks.iter().find(|(id, tasks)| {
            tasks
                .iter()
                .any(|ft| matches!(ft, InitialExploration {system_symbol} if system_symbol == spawning_system_symbol))
        }) {
            self.fleets.get(fleet_id).and_then(|fleet| {
                self.get_ships_of_fleet(&fleet)
                    .first()
                    .map(|&s| s.symbol.clone())
            })
        } else {
            None
        }
    }

    fn get_fleet_executing_fleet_task(&self, fleet_task: &FleetTask) -> Option<FleetId> {
        self.fleet_tasks
            .iter()
            .find_map(|(id, tasks)| tasks.contains(fleet_task).then_some(id.clone()))
    }

    pub async fn agent_info_credits(&self) -> Credits {
        self.treasurer.lock().await.agent_credits()
    }

    async fn report_expense_transaction_to_treasurer(&self, fleet_id: &FleetId, operation_expense_event: &OperationExpenseEvent) {
        let mut guard = self.treasurer.lock().await;
        match operation_expense_event {
            OperationExpenseEvent::RefueledShip { response } => {
                let event = TransactionEvent::ShipRefueled {
                    timestamp: response.data.transaction.timestamp.clone(),
                    waypoint: response.data.transaction.waypoint_symbol.clone(),
                    fuel_barrels_purchased: response.data.transaction.units as u32,
                    cost_per_unit: response.data.transaction.price_per_unit.into(),
                    total_cost: response.data.transaction.total_price.into(),
                    new_fuel_level: response.data.fuel.current as u32,
                };

                match guard.record_expense(fleet_id, &response.data.transaction.ship_symbol, event) {
                    Ok(_) => {}
                    Err(_) => {}
                }
            }
        }
    }

    async fn mark_transaction_completed_to_treasurer(
        &mut self,
        transaction_event: &TransactionActionEvent,
        ticket: &TransactionTicket,
        ship_symbol: &ShipSymbol,
    ) {
        let mut guard = self.treasurer.lock().await;

        let fleet_id: &FleetId = self.ship_fleet_assignment.get(ship_symbol).unwrap();

        guard
            .complete_ticket(ticket.id.clone())
            .expect("complete_ticket");

        if ticket.ticket_type == TicketType::Trading {
            guard
                .return_excess_capital_to_treasurer(fleet_id)
                .expect("return_excess_capital_to_treasurer");
        }
        self.active_trade_ids.remove(ship_symbol);
        let calculated_agent_credits = guard.agent_credits();
        let agent_credits_from_response = transaction_event
            .maybe_updated_agent_credits()
            .unwrap_or_default();
        if calculated_agent_credits != agent_credits_from_response.into() {
            event!(
                Level::WARN,
                message = "agent credits differ after reporting transaction as completed",
                calculated_agent_credits = calculated_agent_credits.0,
                agent_credits_from_response
            );
            guard.agent_credits(); // Hello, breakpoint
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
    },
    RegisterWaypointForPermanentObservation {
        ship_symbol: ShipSymbol,
        waypoint_symbol: WaypointSymbol,
        exploration_tasks: Vec<ExplorationTask>,
    },
}

pub async fn recompute_tasks_after_ship_finishing_behavior_tree(
    admiral: &mut FleetAdmiral,
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
            let waypoints = bmc
                .system_bmc()
                .get_waypoints_of_system(&Ctx::Anonymous, &waypoint_symbol.system_symbol())
                .await?;
            let waypoint = waypoints
                .iter()
                .find(|wp| &wp.symbol == waypoint_symbol)
                .unwrap();
            Ok(NewTaskResult::RegisterWaypointForPermanentObservation {
                ship_symbol: ship.symbol.clone(),
                waypoint_symbol: waypoint_symbol.clone(),
                exploration_tasks: get_exploration_tasks_for_waypoint(waypoint),
            })
        }
        ShipTask::ObserveAllWaypointsOnce { .. } => Ok(NewTaskResult::DismantleFleets {
            fleets_to_dismantle: vec![admiral
                .ship_fleet_assignment
                .get(&ship.symbol)
                .unwrap()
                .clone()],
        }),
        ShipTask::Trade { .. } | ShipTask::PrepositionShipForTrade { .. } => {
            let facts = collect_fleet_decision_facts(Arc::clone(&bmc), &ship.nav.system_symbol).await?;
            let ship_prices = bmc
                .shipyard_bmc()
                .get_latest_ship_prices(&Ctx::Anonymous, &ship.nav.system_symbol)
                .await?;

            admiral.try_create_ship_purchase_ticket(&ship_prices).await;

            let new_tasks = FleetAdmiral::compute_ship_tasks(admiral, &facts, Arc::clone(&bmc)).await?;
            if let Some((ss, new_task_for_ship)) = new_tasks.iter().find(|(ss, task)| ss == &ship.symbol) {
                Ok(NewTaskResult::AssignNewTaskToShip {
                    ship_symbol: ss.clone(),
                    task: new_task_for_ship.clone(),
                })
            } else {
                // No new tasks found. Computing again for debugging

                println!("{}", FleetAdmiral::generate_state_overview(admiral).await);
                let new_tasks = FleetAdmiral::compute_ship_tasks(admiral, &facts, Arc::clone(&bmc)).await?;

                Err(anyhow!(
                    "No new task for this ship {} found after finishing task {}",
                    &ship.symbol,
                    finished_task
                ))
            }
        }
    }
}

pub fn compute_fleet_configs(
    tasks: &[FleetTask],
    fleet_decision_facts: &FleetDecisionFacts,
    shopping_list_in_order: &Vec<(ShipType, FleetTask)>,
) -> Vec<(FleetConfig, FleetTask)> {
    let all_waypoints_of_interest = fleet_decision_facts
        .marketplaces_of_interest
        .iter()
        .chain(fleet_decision_facts.shipyards_of_interest.iter())
        .unique()
        .collect_vec();

    tasks
        .iter()
        .filter_map(|t| {
            let desired_fleet_config = shopping_list_in_order
                .iter()
                .filter(|(st, ft)| ft == t)
                .map(|(st, _)| st)
                .cloned()
                .collect_vec();
            let maybe_cfg = match t {
                FleetTask::InitialExploration { system_symbol } => Some(FleetConfig::SystemSpawningCfg(SystemSpawningFleetConfig {
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
                    jump_gate_waypoint: fleet_decision_facts
                        .construction_site
                        .clone()
                        .expect("construction_site")
                        .symbol,
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

    let has_construct_jump_gate_task_been_completed = completed_tasks
        .iter()
        .any(|t| matches!(&t.task, ConstructJumpGate { system_symbol }));

    let has_collect_market_infos_once_task_been_completed = completed_tasks
        .iter()
        .any(|t| matches!(&t.task, InitialExploration { system_symbol }));

    let is_jump_gate_done = fleet_decision_facts
        .construction_site
        .clone()
        .map(|cs| cs.is_complete)
        .unwrap_or(false)
        || has_construct_jump_gate_task_been_completed;

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

    let marketplace_waypoints_ex_shipyards = diff_waypoint_symbols(&fleet_decision_facts.marketplaces_of_interest, &fleet_decision_facts.shipyards_of_interest);

    let num_shipyards_of_interest = fleet_decision_facts.shipyards_of_interest.len();
    let num_marketplaces_ex_shipyards = marketplace_waypoints_ex_shipyards.len();

    let waypoints_of_interest = fleet_decision_facts
        .marketplaces_of_interest
        .iter()
        .chain(fleet_decision_facts.shipyards_of_interest.iter())
        .unique()
        .collect_vec();
    let num_waypoints_of_interest = waypoints_of_interest.len();

    let fleet_phase = if !has_collected_all_waypoint_details_once {
        create_initial_exploration_fleet_phase(&system_symbol, num_shipyards_of_interest)
    } else if !is_jump_gate_done {
        create_construction_fleet_phase(&system_symbol, num_shipyards_of_interest, num_marketplaces_ex_shipyards)
    } else if is_jump_gate_done {
        create_trade_profitably_fleet_phase(system_symbol, num_waypoints_of_interest)
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

    fleet_phase
}

pub fn create_trade_profitably_fleet_phase(system_symbol: SystemSymbol, num_waypoints_of_interest: usize) -> FleetPhase {
    let tasks = [
        TradeProfitably {
            system_symbol: system_symbol.clone(),
        },
        ObserveAllWaypointsOfSystemWithStationaryProbes {
            system_symbol: system_symbol.clone(),
        },
    ];

    let probe_observation_task = tasks[1].clone();

    let trading_fleet = [ShipType::SHIP_LIGHT_HAULER].repeat(4);

    let probe_observation_fleet = [ShipType::SHIP_PROBE].repeat(num_waypoints_of_interest);

    let shopping_list_in_order = trading_fleet
        .into_iter()
        .map(|ship_type| (ship_type, probe_observation_task.clone()))
        .chain(
            probe_observation_fleet
                .into_iter()
                .map(|ship_type| (ship_type, probe_observation_task.clone())),
        )
        .collect_vec();

    FleetPhase {
        name: FleetPhaseName::TradeProfitably,
        shopping_list_in_order,
        tasks: tasks.into(),
    }
}

pub fn create_initial_exploration_fleet_phase(system_symbol: &SystemSymbol, num_shipyards_of_interest: usize) -> FleetPhase {
    let tasks = [
        InitialExploration {
            system_symbol: system_symbol.clone(),
        },
        ObserveAllWaypointsOfSystemWithStationaryProbes {
            system_symbol: system_symbol.clone(),
        },
    ];

    let frigate_task = tasks[0].clone();
    let probe_observation_task = tasks[1].clone();

    let shipyard_probes = [ShipType::SHIP_PROBE].repeat(num_shipyards_of_interest);

    let shopping_list_in_order = vec![(ShipType::SHIP_COMMAND_FRIGATE, frigate_task)]
        .into_iter()
        .chain(
            shipyard_probes
                .into_iter()
                .map(|ship_type| (ship_type, probe_observation_task.clone())),
        )
        .collect_vec();

    FleetPhase {
        name: FleetPhaseName::InitialExploration,
        shopping_list_in_order,
        tasks: Vec::from(tasks),
    }
}

pub fn create_construction_fleet_phase(system_symbol: &SystemSymbol, num_shipyards_of_interest: usize, num_marketplaces_ex_shipyards: usize) -> FleetPhase {
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

    let shipyard_probes = [ShipType::SHIP_PROBE].repeat(num_shipyards_of_interest);
    let construction_fleet = [vec![ShipType::SHIP_COMMAND_FRIGATE], [ShipType::SHIP_LIGHT_HAULER].repeat(4)].concat();

    let mining_fleet = [
        vec![ShipType::SHIP_MINING_DRONE],
        [
            ShipType::SHIP_MINING_DRONE,
            ShipType::SHIP_MINING_DRONE,
            ShipType::SHIP_MINING_DRONE,
            ShipType::SHIP_SURVEYOR,
            ShipType::SHIP_LIGHT_HAULER,
        ]
        .repeat(2),
    ]
    .concat();

    let siphoning_fleet = [ShipType::SHIP_SIPHON_DRONE].repeat(5);

    let other_probes = [ShipType::SHIP_PROBE].repeat(num_marketplaces_ex_shipyards);

    // this is compile-time safe - rust knows the length of arrays and restricts out-of-bounds-access
    let construct_jump_gate_task = tasks[0].clone();
    let probe_observation_task = tasks[1].clone();
    let mining_task = tasks[2].clone();
    let siphoning_task = tasks[3].clone();

    let shopping_list_in_order = shipyard_probes
        .into_iter()
        .map(|ship_type| (ship_type, probe_observation_task.clone()))
        .chain(
            other_probes
                .into_iter()
                .map(|ship_type| (ship_type, probe_observation_task.clone())),
        )
        .chain(
            construction_fleet
                .into_iter()
                .map(|ship_type| (ship_type, construct_jump_gate_task.clone())),
        )
        .chain(
            mining_fleet
                .into_iter()
                .map(|ship_type| (ship_type, mining_task.clone())),
        )
        .chain(
            siphoning_fleet
                .into_iter()
                .map(|ship_type| (ship_type, siphoning_task.clone())),
        )
        .collect_vec();

    FleetPhase {
        name: FleetPhaseName::ConstructJumpGate,
        shopping_list_in_order,
        tasks: tasks.into(),
    }
}

fn get_ship_type_of_ship(ship: &Ship) -> Result<ShipType> {
    use ShipRegistrationRole::*;
    use ShipType::*;

    let is_miner = || {
        ship.mounts.iter().any(|m| {
            m.symbol
                .starts_with(MOUNT_MINING_LASER_I.to_string().as_str())
        })
    };
    let is_siphoner = || {
        ship.mounts.iter().any(|m| {
            m.symbol
                .starts_with(MOUNT_GAS_SIPHON_I.to_string().as_str())
        })
    };
    let is_surveyor = || {
        ship.mounts
            .iter()
            .any(|m| m.symbol.starts_with(MOUNT_SURVEYOR_I.to_string().as_str()))
    };
    let is_refining_freighter = || ship.registration.role == Refinery;

    match &ship.frame.symbol {
        ShipFrameSymbol::FRAME_PROBE => Ok(SHIP_PROBE),
        ShipFrameSymbol::FRAME_EXPLORER => Ok(SHIP_EXPLORER),
        ShipFrameSymbol::FRAME_MINER => Ok(SHIP_ORE_HOUND),
        ShipFrameSymbol::FRAME_DRONE if is_miner() => Ok(SHIP_MINING_DRONE),
        ShipFrameSymbol::FRAME_DRONE if is_siphoner() => Ok(SHIP_SIPHON_DRONE),
        ShipFrameSymbol::FRAME_DRONE if is_surveyor() => Ok(SHIP_SURVEYOR),
        ShipFrameSymbol::FRAME_DRONE => Err(anyhow!(
            "Unknown mapping from FRAME_DRONE to ShipType (none of those is true: is_miner, is_siphoner, is_surveyor)"
        )),
        ShipFrameSymbol::FRAME_SHUTTLE => Ok(SHIP_LIGHT_SHUTTLE),
        ShipFrameSymbol::FRAME_LIGHT_FREIGHTER => Ok(SHIP_LIGHT_HAULER),
        ShipFrameSymbol::FRAME_FRIGATE => Ok(SHIP_COMMAND_FRIGATE),
        ShipFrameSymbol::FRAME_INTERCEPTOR => Ok(SHIP_INTERCEPTOR),
        ShipFrameSymbol::FRAME_HEAVY_FREIGHTER if is_refining_freighter() => Ok(SHIP_REFINING_FREIGHTER),
        ShipFrameSymbol::FRAME_HEAVY_FREIGHTER => Ok(SHIP_HEAVY_FREIGHTER),
        ShipFrameSymbol::FRAME_BULK_FREIGHTER => Ok(SHIP_BULK_FREIGHTER),
        ShipFrameSymbol::FRAME_CARRIER => Err(anyhow!("Unknown mapping from FRAME_CARRIER to ShipType")),
        ShipFrameSymbol::FRAME_CRUISER => Err(anyhow!("Unknown mapping from FRAME_CRUISER to ShipType")),
        ShipFrameSymbol::FRAME_TRANSPORT => Err(anyhow!("Unknown mapping from FRAME_TRANSPORT to ShipType")),
        ShipFrameSymbol::FRAME_DESTROYER => Err(anyhow!("Unknown mapping from FRAME_DESTROYER to ShipType")),
        ShipFrameSymbol::FRAME_RACER => Err(anyhow!("Unknown mapping from FRAME_RACER to ShipType")),
        ShipFrameSymbol::FRAME_FIGHTER => Err(anyhow!("Unknown mapping from FRAME_FIGHTER to ShipType")),
    }
}

fn assign_matching_ships(desired_fleet_config: &[ShipType], available_ships: &mut HashMap<ShipSymbol, Ship>) -> HashMap<ShipSymbol, Ship> {
    let mut assigned_ships: Vec<Ship> = vec![];

    for ship_type in desired_fleet_config.iter() {
        let assignable_ships = available_ships
            .iter()
            .filter_map(|(_, s)| {
                let current_ship_type = get_ship_type_of_ship(&s).expect("role_to_ship_type_mapping");
                (&current_ship_type == ship_type).then_some((s.symbol.clone(), current_ship_type, s.clone()))
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
    let ships: HashMap<ShipSymbol, Ship> = assigned_ships
        .into_iter()
        .map(|ship| (ship.symbol.clone(), ship))
        .collect();

    ships
}

pub async fn collect_fleet_decision_facts(bmc: Arc<dyn Bmc>, system_symbol: &SystemSymbol) -> Result<FleetDecisionFacts> {
    let ships = bmc.ship_bmc().get_ships(&Ctx::Anonymous, None).await?;
    let agent_info = bmc
        .agent_bmc()
        .load_agent(&Ctx::Anonymous)
        .await
        .expect("agent");

    let marketplaces_of_interest: Vec<MarketEntry> = bmc
        .market_bmc()
        .get_latest_market_data_for_system(&Ctx::Anonymous, system_symbol)
        .await?;
    let shipyards_of_interest = bmc
        .shipyard_bmc()
        .get_latest_shipyard_entries_of_system(&Ctx::Anonymous, system_symbol)
        .await?;

    let marketplace_symbols_of_interest = marketplaces_of_interest
        .iter()
        .map(|me| me.waypoint_symbol.clone())
        .collect_vec();
    let marketplaces_to_explore = find_marketplaces_for_exploration(marketplaces_of_interest.clone());

    let shipyard_symbols_of_interest = shipyards_of_interest
        .iter()
        .map(|db_entry| db_entry.waypoint_symbol.clone())
        .collect_vec();
    let shipyards_to_explore = find_shipyards_for_exploration(shipyards_of_interest.clone());

    let maybe_construction_site: Option<GetConstructionResponse> = bmc
        .construction_bmc()
        .get_construction_site_for_system(&Ctx::Anonymous, system_symbol.clone())
        .await?;

    let supply_chain = bmc
        .supply_chain_bmc()
        .get_supply_chain(&Ctx::Anonymous)
        .await?
        .unwrap();

    let agent = bmc
        .agent_bmc()
        .load_agent(&Ctx::Anonymous)
        .await
        .expect("agent");
    let headquarters_waypoint = agent.headquarters;

    let market_data = bmc
        .market_bmc()
        .get_latest_market_data_for_system(&Ctx::Anonymous, &headquarters_waypoint.system_symbol())
        .await
        .expect("market_data");

    let market_data: Vec<(WaypointSymbol, Vec<MarketTradeGood>)> = trading::to_trade_goods_with_locations(&market_data);

    let maybe_construction_site = bmc
        .construction_bmc()
        .get_construction_site_for_system(&Ctx::Anonymous, headquarters_waypoint.system_symbol())
        .await
        .expect("construction_site");

    let waypoints_of_system = bmc
        .system_bmc()
        .get_waypoints_of_system(&Ctx::Anonymous, &headquarters_waypoint.system_symbol())
        .await
        .expect("waypoints");

    let waypoint_map: HashMap<WaypointSymbol, &Waypoint> = waypoints_of_system
        .iter()
        .map(|wp| (wp.symbol.clone(), wp))
        .collect::<HashMap<_, _>>();

    let marketplaces_with_up_to_date_infos = diff_waypoint_symbols(&marketplace_symbols_of_interest, &marketplaces_to_explore);

    let all_market_data_available = marketplaces_with_up_to_date_infos.len() == marketplace_symbols_of_interest.len();

    let materialized_supply_chain = if all_market_data_available {
        // Only create a materialized chain once we have all market-data
        let materialized_chain = st_domain::supply_chain::materialize_supply_chain(
            headquarters_waypoint.system_symbol(),
            &supply_chain,
            &market_data,
            &waypoint_map,
            &maybe_construction_site,
        );
        Some(materialized_chain)
    } else {
        None
    };

    Ok(FleetDecisionFacts {
        marketplaces_of_interest: marketplace_symbols_of_interest.clone(),
        marketplaces_with_up_to_date_infos: marketplaces_with_up_to_date_infos,
        shipyards_of_interest: shipyard_symbols_of_interest.clone(),
        shipyards_with_up_to_date_infos: diff_waypoint_symbols(&shipyard_symbols_of_interest, &shipyards_to_explore),
        construction_site: maybe_construction_site.map(|resp| resp.data),
        ships,
        materialized_supply_chain,
        agent_info,
    })
}
pub fn diff_waypoint_symbols(waypoints_of_interest: &[WaypointSymbol], already_explored: &[WaypointSymbol]) -> Vec<WaypointSymbol> {
    let set2: HashSet<_> = already_explored.iter().collect();

    waypoints_of_interest
        .iter()
        .filter(|item| !set2.contains(item))
        .cloned()
        .collect()
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
    facts: &FleetDecisionFacts,
    active_fleets: &HashMap<FleetId, Fleet>,
    active_fleet_task_assignments: &HashMap<FleetId, Vec<FleetTask>>,
    fleet_phase: &FleetPhase,
) -> (Vec<Fleet>, Vec<(FleetId, FleetTask)>) {
    let active_fleets_and_tasks: Vec<(Fleet, (FleetId, FleetTask))> = active_fleets
        .iter()
        .map(|(fleet_id, fleet)| {
            let task = active_fleet_task_assignments
                .get(fleet_id)
                .cloned()
                .unwrap_or_default()
                .first()
                .cloned()
                .unwrap();
            (fleet.clone(), (fleet_id.clone(), task))
        })
        .collect_vec();

    let new_fleet_configs = compute_fleet_configs(&fleet_phase.tasks, facts, &fleet_phase.shopping_list_in_order)
        .iter()
        .filter(|(fleet_cfg, fleet_task)| {
            // if we have a fleet already doing the same task, we ignore it
            active_fleets_and_tasks
                .iter()
                .any(|(_, (_, active_fleet_task))| active_fleet_task == fleet_task)
                .not()
        })
        .cloned()
        .collect_vec();

    let next_fleet_id = active_fleets
        .keys()
        .map(|id| id.0)
        .max()
        .map(|max_id| max_id + 1)
        .unwrap_or(0);

    let new_fleets_with_tasks: Vec<(Fleet, (FleetId, FleetTask))> = new_fleet_configs
        .into_iter()
        .enumerate()
        .map(|(idx, (cfg, task))| create_fleet(cfg.clone(), task.clone(), next_fleet_id + idx as i32).unwrap())
        .collect_vec();

    let (fleets, fleet_tasks): (Vec<Fleet>, Vec<(FleetId, FleetTask)>) = new_fleets_with_tasks
        .into_iter()
        .chain(active_fleets_and_tasks)
        .unzip();
    (fleets, fleet_tasks)
}

pub fn create_fleet(super_fleet_config: FleetConfig, fleet_task: FleetTask, id: i32) -> Result<(Fleet, (FleetId, FleetTask))> {
    let id = FleetId(id);
    let fleet = Fleet {
        id: id.clone(),
        cfg: super_fleet_config,
    };

    Ok((fleet, (id, fleet_task)))
}

pub fn get_next_ship_purchase(ship_map: &HashMap<ShipSymbol, Ship>, fleet_phase: &FleetPhase) -> Option<(ShipType, FleetTask)> {
    get_all_next_ship_purchases(ship_map, fleet_phase)
        .first()
        .cloned()
}

pub fn get_all_next_ship_purchases(ship_map: &HashMap<ShipSymbol, Ship>, fleet_phase: &FleetPhase) -> Vec<(ShipType, FleetTask)> {
    let mut current_ship_types: HashMap<ShipType, u32> = HashMap::new();
    let mut purchases = Vec::new();

    // Count current ships by type
    for (_, s) in ship_map.iter() {
        let ship_type = get_ship_type_of_ship(&s).expect(format!("role_to_ship_type_mapping for ShipFrameSymbol {}", &s.frame.symbol.to_string()).as_str());
        current_ship_types
            .entry(ship_type)
            .and_modify(|counter| *counter += 1)
            .or_insert(1);
    }

    // Create a mutable copy to track remaining ships
    let mut remaining_ships = current_ship_types.clone();

    // Check each item in the shopping list
    for (ship_type, fleet_task) in fleet_phase.shopping_list_in_order.iter() {
        let num_of_ships_left = remaining_ships.get(ship_type).unwrap_or(&0);
        if num_of_ships_left.is_zero() {
            // Need to purchase this ship
            purchases.push((*ship_type, fleet_task.clone()));
        } else {
            // We already have this ship - decrement the count
            remaining_ships
                .entry(*ship_type)
                .and_modify(|counter| *counter -= 1);
        }
    }

    purchases
}

#[derive(Clone, Debug, Serialize, Deserialize, Display)]
pub enum ShipTaskRequirement {
    TradeTicket {
        trade_ticket: TradeTicket,
        first_purchase_location: WaypointSymbol,
    },
    None,
}
