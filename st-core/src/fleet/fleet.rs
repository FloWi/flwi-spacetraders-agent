use crate::behavior_tree::ship_behaviors::ShipAction;
use crate::fleet::construction_fleet::ConstructJumpGateFleet;
use crate::fleet::fleet_runner::FleetRunner;
use crate::fleet::initial_data_collector::load_and_store_initial_data_in_bmcs;
use crate::fleet::market_observation_fleet::MarketObservationFleet;
use crate::fleet::mining_fleet::MiningFleet;
use crate::fleet::siphoning_fleet::SiphoningFleet;
use crate::fleet::supply_chain_test::format_number;
use crate::fleet::system_spawning_fleet::SystemSpawningFleet;
use crate::marketplaces::marketplaces::{find_marketplaces_for_exploration, find_shipyards_for_exploration};
use crate::pagination::fetch_all_pages;
use crate::st_client::StClientTrait;
use crate::transfer_cargo_manager::TransferCargoManager;
use anyhow::{anyhow, Result};
use chrono::Utc;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{ContentArrangement, Table};
use itertools::{all, Itertools};
use pathfinding::num_traits::Zero;
use serde::{Deserialize, Serialize};
use st_domain::budgeting::credits::Credits;
use st_domain::budgeting::treasury_redesign::{
    ActiveTradeRoute, FinanceResult, FinanceTicket, FinanceTicketDetails, FleetBudget, LedgerArchiveTask, ThreadSafeTreasurer,
};
use st_domain::FleetConfig::SystemSpawningCfg;
use st_domain::FleetTask::{ConstructJumpGate, InitialExploration, MineOres, ObserveAllWaypointsOfSystemWithStationaryProbes, SiphonGases, TradeProfitably};
use st_domain::TradeGoodSymbol::{MOUNT_GAS_SIPHON_I, MOUNT_MINING_LASER_I, MOUNT_SURVEYOR_I};
use st_domain::{
    get_exploration_tasks_for_waypoint, trading, ConstructJumpGateFleetConfig, Construction, ExplorationTask, Fleet, FleetConfig, FleetDecisionFacts, FleetId,
    FleetPhase, FleetPhaseName, FleetTask, FleetTaskCompletion, GetConstructionResponse, MarketEntry, MarketObservationFleetConfig, MarketTradeGood,
    MiningFleetConfig, OperationExpenseEvent, Ship, ShipFrameSymbol, ShipPriceInfo, ShipRegistrationRole, ShipSymbol, ShipTask, ShipType, SiphoningFleetConfig,
    StationaryProbeLocation, SystemSpawningFleetConfig, SystemSymbol, TicketId, TradeGoodSymbol, TradingFleetConfig, TransactionActionEvent, Waypoint,
    WaypointSymbol, WaypointType,
};
use st_store::bmc::Bmc;
use st_store::{load_fleet_overview, upsert_fleets_data, Ctx};
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;
use std::ops::Not;
use std::sync::{mpsc, Arc};
use std::time::Duration;
use strum::{Display, IntoEnumIterator};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{event, Level};
use FleetConfig::{ConstructJumpGateCfg, MarketObservationCfg, MiningCfg, SiphoningCfg, TradingCfg};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub enum ShipStatusReport {
    ShipActionCompleted(Ship, ShipAction),
    TransactionCompleted(Ship, TransactionActionEvent, FinanceTicket),
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
    pub treasurer: ThreadSafeTreasurer,
    pub ship_purchase_demand: VecDeque<(ShipType, FleetTask)>,
}

impl FleetAdmiral {
    pub async fn redistribute_distribute_fleet_budgets(&self, ship_price_info: &ShipPriceInfo, system_symbol: &SystemSymbol) -> Result<()> {
        if self.treasurer.get_active_tickets().await?.is_empty().not() {
            event!(Level::WARN, message = "called redistribute_fleet_budgets with active_tickets");
        }

        let treasury_credits_before_rebalancing = self.agent_info_credits().await;
        let treasury_overview_before_rebalancing = self.generate_budgets_overview().await;

        event!(
            Level::INFO,
            message = "Fleet Budgets before rebalancing",
            treasury_credits = treasury_credits_before_rebalancing.0
        );

        //FIXME: make sure this works with multiple systems
        self.treasurer.remove_all_fleets().await?;

        match self.fleet_phase.name {
            FleetPhaseName::InitialExploration => {
                let spawning_fleet = self
                    .get_fleet_executing_fleet_task(&InitialExploration {
                        system_symbol: system_symbol.clone(),
                    })
                    .unwrap();
                let market_observation_fleet = self
                    .get_fleet_executing_fleet_task(&ObserveAllWaypointsOfSystemWithStationaryProbes {
                        system_symbol: system_symbol.clone(),
                    })
                    .unwrap();
                self.treasurer
                    .create_fleet(&spawning_fleet, 25_000.into())
                    .await?;
                self.treasurer
                    .transfer_funds_to_fleet_to_top_up_available_capital(&spawning_fleet)
                    .await?;
                self.treasurer
                    .create_fleet(&market_observation_fleet, 0.into())
                    .await?;
            }
            FleetPhaseName::ConstructJumpGate => {
                let construction_fleet = self
                    .get_fleet_executing_fleet_task(&ConstructJumpGate {
                        system_symbol: system_symbol.clone(),
                    })
                    .unwrap();
                let market_observation_fleet = self
                    .get_fleet_executing_fleet_task(&ObserveAllWaypointsOfSystemWithStationaryProbes {
                        system_symbol: system_symbol.clone(),
                    })
                    .unwrap();

                let siphoning_fleet = self
                    .get_fleet_executing_fleet_task(&SiphonGases {
                        system_symbol: system_symbol.clone(),
                    })
                    .unwrap();

                let mining_fleet = self
                    .get_fleet_executing_fleet_task(&MineOres {
                        system_symbol: system_symbol.clone(),
                    })
                    .unwrap();

                let number_of_traders = self.get_ships_of_fleet_id(&construction_fleet).len() as u32;
                let budget_per_trader: Credits = 75_000.into();
                let reserve_per_trader: Credits = 1_000.into();

                self.treasurer
                    .create_fleet(&construction_fleet, budget_per_trader * number_of_traders)
                    .await?;

                self.treasurer
                    .transfer_funds_to_fleet_to_top_up_available_capital(&construction_fleet)
                    .await?;

                self.treasurer
                    .set_new_operating_reserve(&construction_fleet, reserve_per_trader * number_of_traders)
                    .await?;

                self.treasurer
                    .create_fleet(&market_observation_fleet, 0.into())
                    .await?;

                self.treasurer
                    .create_fleet(&siphoning_fleet, 5_000.into())
                    .await?;
                self.treasurer
                    .set_new_operating_reserve(&siphoning_fleet, 5_000.into())
                    .await?;
                self.treasurer
                    .transfer_funds_to_fleet_to_top_up_available_capital(&siphoning_fleet)
                    .await?;

                self.treasurer
                    .create_fleet(&mining_fleet, 5_000.into())
                    .await?;
                self.treasurer
                    .set_new_operating_reserve(&mining_fleet, 5_000.into())
                    .await?;
                self.treasurer
                    .transfer_funds_to_fleet_to_top_up_available_capital(&mining_fleet)
                    .await?;
            }
            FleetPhaseName::TradeProfitably => {}
        }

        let treasury_credits_after_rebalancing = self.agent_info_credits().await;
        let treasury_overview_after_rebalancing = self.generate_budgets_overview().await;
        event!(
            Level::INFO,
            message = "Fleet Budgets after rebalancing",
            fleet_count = self.fleets.len(),
            treasury_credits = treasury_credits_after_rebalancing.0
        );
        println!(
            r#"============================================================================================
Fleet Budgets before rebalancing

{}

============================================================================================
Fleet Budgets after rebalancing

{}
"#,
            treasury_overview_before_rebalancing, treasury_overview_after_rebalancing,
        );

        if treasury_credits_before_rebalancing != treasury_credits_after_rebalancing {
            event!(
                Level::ERROR,
                message = "error during rebalancing of fleet budgets - the agent credits differ",
                treasury_credits_before_rebalancing = treasury_credits_before_rebalancing.0,
                treasury_credits_after_rebalancing = treasury_credits_after_rebalancing.0
            );

            eprintln!(
                "Json entries of all ledger entries:\n{}",
                serde_json::to_string(&self.treasurer.get_ledger_entries().await?).unwrap_or_default()
            )
        }

        Ok(())
    }

    pub(crate) async fn adjust_fleet_budget_after_ship_purchase(admiral: &FleetAdmiral, new_ship: &Ship, fleet_id: &FleetId) -> Result<()> {
        let fleet = admiral
            .fleets
            .get(fleet_id)
            .ok_or(anyhow!("Fleet id not found"))?;
        let budget = admiral.treasurer.get_fleet_budget(fleet_id).await?;
        let all_ships_purchased = admiral.ship_purchase_demand.is_empty();
        let ships_of_fleet = admiral.get_ships_of_fleet(fleet);

        let (new_total_capital, new_operating_reserve) = Self::calc_required_operating_capital_for_fleet(fleet, &ships_of_fleet);
        let construction_budget = Credits::new(1_000_000);

        let new_total_capital = match fleet.cfg {
            SystemSpawningCfg(_) => 0.into(),
            MarketObservationCfg(_) => 0.into(),
            SiphoningCfg(_) => 0.into(),
            MiningCfg(_) => 0.into(),
            TradingCfg(_) => new_total_capital,
            ConstructJumpGateCfg(_) => {
                if all_ships_purchased {
                    new_total_capital + construction_budget
                } else {
                    new_total_capital
                }
            }
        };

        if new_total_capital > budget.budget || new_operating_reserve > budget.operating_reserve {
            event!(
                Level::INFO,
                message = "Increasing fleet budget after ship purchase and trying to top up available capital",
                new_ship = new_ship.symbol.0,
                new_ship_type = new_ship.frame.symbol.to_string(),
                fleet_id = fleet_id.0,
                fleet = fleet.cfg.to_string(),
                old_total_capital = budget.budget.0,
                new_total_capital = new_total_capital.0
            );
            admiral
                .treasurer
                .set_fleet_budget(fleet_id, new_total_capital)
                .await?;

            admiral
                .treasurer
                .set_new_operating_reserve(fleet_id, new_operating_reserve)
                .await?;

            admiral
                .treasurer
                .transfer_funds_to_fleet_to_top_up_available_capital(fleet_id)
                .await?;
        }

        Ok(())
    }

    pub(crate) fn calc_required_operating_capital_for_fleet(fleet: &Fleet, ships: &[&Ship]) -> (Credits, Credits) {
        let num_fuel_consuming_ships = ships.iter().filter(|s| s.fuel.capacity > 0).count() as u32;
        let num_trading_ships = ships.iter().filter(|s| s.cargo.capacity > 0).count() as u32;

        let required_fuel_budget = Credits::new(1_000) * num_fuel_consuming_ships;
        let required_trading_budget = Credits::new(75_000) * num_trading_ships;

        let (total_capital, operating_reserve) = match fleet.cfg {
            TradingCfg(_) => (required_trading_budget, required_fuel_budget),
            ConstructJumpGateCfg(_) => (required_trading_budget, required_fuel_budget),
            MiningCfg(_) => (Credits::new(0), required_fuel_budget),
            SiphoningCfg(_) => (Credits::new(0), required_fuel_budget),
            MarketObservationCfg(_) => (Credits::new(0), required_fuel_budget),
            SystemSpawningCfg(_) => (Credits::new(0), required_fuel_budget),
        };
        (total_capital, operating_reserve)
    }

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
                format!("{} (#{})", fleet.cfg.to_string(), fleet.id.0).as_str(),
                format_number(budget.current_capital.0 as f64).as_str(),
                format_number(budget.budget.0 as f64).as_str(),
                format_number(budget.operating_reserve.0 as f64).as_str(),
            ]);
        }

        budget_table.add_row(vec![
            "---",
            "Treasury",
            format_number(self.treasurer.get_current_treasury_fund().await.unwrap().0 as f64).as_str(),
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
                "initiating_fleet",
                "executing_vessel",
                "fleet_of_executing_ship",
                "type",
                "allocated_credits",
            ]);

        for (_, ticket) in self.get_treasurer_tickets().await.iter() {
            let fleet_str = self
                .fleets
                .get(&ticket.fleet_id)
                .map(|f| format!("{} (#{})", f.cfg.to_string(), f.id.0))
                .unwrap_or("---".to_string());

            let fleet_of_executing_ship_str = self
                .get_fleet_of_ship(&ticket.ship_symbol)
                .map(|f| format!("{} (#{})", f.cfg.to_string(), f.id.0))
                .unwrap_or("---".to_string());

            tickets_table.add_row(vec![
                ticket.ticket_id.0.to_string().as_str(),
                fleet_str.as_str(),
                ticket.ship_symbol.0.to_string().as_str(),
                fleet_of_executing_ship_str.as_str(),
                ticket.details.to_string().as_str(),
                ticket.allocated_credits.to_string().as_str(),
            ]);
        }

        format!("Budget Table\n{}\n\nTicket Table\n{}", budget_table, tickets_table)
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

    pub(crate) async fn get_fleet_tickets(&self) -> HashMap<FleetId, Vec<FinanceTicket>> {
        self.treasurer.get_fleet_tickets().await.unwrap_or_default()
    }

    pub(crate) async fn get_fleet_budgets(&self) -> HashMap<FleetId, FleetBudget> {
        self.treasurer.get_fleet_budgets().await.unwrap_or_default()
    }

    pub(crate) async fn get_treasurer_tickets(&self) -> HashMap<TicketId, FinanceTicket> {
        self.treasurer
            .get_active_tickets()
            .await
            .unwrap_or_default()
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

    pub async fn run_fleets(
        fleet_admiral: Arc<Mutex<FleetAdmiral>>,
        client: Arc<dyn StClientTrait>,
        bmc: Arc<dyn Bmc>,
        transfer_cargo_manager: Arc<TransferCargoManager>,
        treasurer_archiver_join_handle: JoinHandle<()>,
    ) -> Result<()> {
        event!(Level::INFO, "Running fleets");

        FleetRunner::run_fleets(
            Arc::clone(&fleet_admiral),
            Arc::clone(&client),
            bmc,
            Arc::clone(&transfer_cargo_manager),
            Duration::from_secs(5),
            treasurer_archiver_join_handle,
        )
        .await?;

        Ok(())
    }

    pub async fn report_ship_action_completed(&mut self, ship_status_report: &ShipStatusReport, bmc: Arc<dyn Bmc>, messages_in_queue: usize) -> Result<()> {
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
                        MarketObservationCfg(_) => {}
                        TradingCfg(_) => {}
                        ConstructJumpGateCfg(_) => {}
                        MiningCfg(_) => {}
                        SiphoningCfg(_) => {}
                    }
                    Ok(())
                } else {
                    Ok(())
                }
            }
            ShipStatusReport::Expense(ship, operation_expense_event) => {
                //FIXME: implement expense report to treasurer
                Ok(())
            }

            ShipStatusReport::TransactionCompleted(ship, transaction_event, updated_trade_ticket) => {
                // should be obsolete and be handled directly by ship
                Ok(())
            }
            ShipStatusReport::ShipFinishedBehaviorTree(ship, task) => {
                event!(
                    Level::DEBUG,
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

    pub async fn load_or_create(bmc: Arc<dyn Bmc>, system_symbol: SystemSymbol, client: Arc<dyn StClientTrait>) -> Result<(Self, JoinHandle<()>)> {
        //make sure we have up-to-date agent info
        let agent = client.get_agent().await?;
        bmc.agent_bmc()
            .store_agent(&Ctx::Anonymous, &agent.data)
            .await?;

        match Self::load_admiral(Arc::clone(&bmc)).await? {
            None => {
                event!(Level::INFO, "loading admiral failed - creating a new one");
                load_and_store_initial_data_in_bmcs(Arc::clone(&client), Arc::clone(&bmc)).await?;

                let (admiral, treasurer_join_handle) = Self::create(Arc::clone(&bmc), system_symbol, Arc::clone(&client)).await?;
                upsert_fleets_data(
                    Arc::clone(&bmc),
                    &Ctx::Anonymous,
                    &admiral.fleets,
                    &admiral.fleet_tasks,
                    &admiral.ship_fleet_assignment,
                    &admiral.ship_tasks,
                )
                .await?;
                Ok((admiral, treasurer_join_handle))
            }
            Some((admiral, treasurer_archiver_join_handle)) => Ok((admiral, treasurer_archiver_join_handle)),
        }
    }

    pub async fn initialize_treasurer(bmc: Arc<dyn Bmc>) -> Result<(ThreadSafeTreasurer, JoinHandle<()>)> {
        let agent_info = bmc.agent_bmc().load_agent(&Ctx::Anonymous).await?;

        let (archive_task_sender, mut task_receiver) = tokio::sync::mpsc::unbounded_channel::<LedgerArchiveTask>();

        // Spawn the archiver task and return its handle
        let archiver_handle = tokio::spawn({
            let bmc = bmc.clone();
            async move {
                while let Some(task) = task_receiver.recv().await {
                    let result = bmc
                        .ledger_bmc()
                        .archive_ledger_entry(&Ctx::Anonymous, &task.entry)
                        .await;
                    let _ = task.response_sender.send(result);
                }
            }
        });

        let treasurer = ThreadSafeTreasurer::new(agent_info.credits.into(), archive_task_sender).await;

        Ok((treasurer, archiver_handle))
    }

    async fn load_admiral(bmc: Arc<dyn Bmc>) -> Result<Option<(Self, JoinHandle<()>)>> {
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

            let (treasurer, treasurer_archiver_join_handle) = Self::initialize_treasurer(bmc.clone()).await?;
            let current_ship_demands = get_all_next_ship_purchases(&ship_map, &fleet_phase);

            let admiral = Self {
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
                treasurer: treasurer.clone(),
                ship_purchase_demand: VecDeque::from(current_ship_demands),
            };

            let ship_prices = bmc
                .shipyard_bmc()
                .get_latest_ship_prices(&Ctx::Anonymous, &system_symbol)
                .await?;

            admiral
                .redistribute_distribute_fleet_budgets(&ship_prices, &system_symbol)
                .await?;

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

            Ok(Some((admiral, treasurer_archiver_join_handle)))
        }
    }

    pub async fn create(bmc: Arc<dyn Bmc>, system_symbol: SystemSymbol, client: Arc<dyn StClientTrait>) -> Result<(Self, JoinHandle<()>)> {
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

        let (treasurer, treasurer_archiver_join_handle) = Self::initialize_treasurer(bmc.clone()).await?;

        let current_ship_demands = get_all_next_ship_purchases(&ship_map, &fleet_phase);

        let ship_prices = bmc
            .shipyard_bmc()
            .get_latest_ship_prices(&Ctx::Anonymous, &system_symbol)
            .await?;

        let admiral = Self {
            completed_fleet_tasks: completed_tasks,
            fleets: fleet_map,
            all_ships: ship_map,
            fleet_tasks: fleet_task_map,
            ship_tasks: Default::default(),
            ship_fleet_assignment,
            fleet_phase,
            active_trade_ids: Default::default(),
            stationary_probe_locations,
            treasurer,
            ship_purchase_demand: VecDeque::from(current_ship_demands),
        };

        admiral
            .redistribute_distribute_fleet_budgets(&ship_prices, &agent_info.headquarters.system_symbol())
            .await?;

        upsert_fleets_data(
            Arc::clone(&bmc),
            &Ctx::Anonymous,
            &admiral.fleets,
            &admiral.fleet_tasks,
            &admiral.ship_fleet_assignment,
            &admiral.ship_tasks,
        )
        .await?;

        Ok((admiral, treasurer_archiver_join_handle))
    }

    pub(crate) async fn pure_compute_ship_tasks(
        admiral: &FleetAdmiral,
        facts: &FleetDecisionFacts,
        latest_market_data: Vec<MarketEntry>,
        ship_prices: ShipPriceInfo,
        waypoints: Vec<Waypoint>,
        active_tickets: &[FinanceTicket],
        fleet_budgets: &HashMap<FleetId, FleetBudget>,
        active_trade_routes: &HashSet<ActiveTradeRoute>,
    ) -> Result<Vec<(ShipSymbol, ShipTask)>> {
        let mut new_ship_tasks: HashMap<ShipSymbol, ShipTask> = HashMap::new();

        let active_ship_purchase_ticket_by_ship: HashMap<ShipSymbol, TicketId> = active_tickets
            .iter()
            .filter_map(|t| match t.details {
                FinanceTicketDetails::PurchaseShip(_) => Some((t.ship_symbol.clone(), t.ticket_id)),
                FinanceTicketDetails::PurchaseTradeGoods(_) => None,
                FinanceTicketDetails::SellTradeGoods(_) => None,
                FinanceTicketDetails::RefuelShip(_) => None,
                FinanceTicketDetails::SupplyConstructionSite(_) => None,
            })
            .collect();

        for (fleet_id, fleet) in admiral.fleets.clone().iter() {
            let ships_of_fleet: Vec<&Ship> = admiral.get_ships_of_fleet(fleet);

            let fleet_budget = fleet_budgets
                .get(fleet_id)
                .cloned()
                .unwrap_or_else(|| panic!("Budget for fleet {} (#{}) not found", fleet.cfg, fleet.id));

            // assign ship tasks for ship purchases if necessary
            for ship in ships_of_fleet.iter() {
                // ship has no task
                let ship_has_no_active_task = admiral.ship_tasks.contains_key(&ship.symbol).not();

                if ship_has_no_active_task {
                    // we have a ship purchase ticket with this ship assigned

                    if let Some(ship_purchase_ticket_id) = active_ship_purchase_ticket_by_ship.get(&ship.symbol) {
                        if let Some(ship_purchase_ticket) = admiral
                            .treasurer
                            .get_ticket(ship_purchase_ticket_id)
                            .await
                            .ok()
                        {
                            new_ship_tasks.insert(
                                ship.symbol.clone(),
                                ShipTask::Trade {
                                    tickets: vec![ship_purchase_ticket.clone()],
                                },
                            );
                        }
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

            let computed_new_tasks = match &fleet.cfg {
                SystemSpawningCfg(cfg) => SystemSpawningFleet::compute_ship_tasks(admiral, cfg, fleet, facts, &unassigned_ships_of_fleet)?,
                MarketObservationCfg(cfg) => MarketObservationFleet::compute_ship_tasks(admiral, cfg, &unassigned_ships_of_fleet)?,
                ConstructJumpGateCfg(cfg) => {
                    let potential_trading_tasks = ConstructJumpGateFleet::compute_ship_tasks(
                        admiral,
                        cfg,
                        fleet,
                        facts,
                        &latest_market_data,
                        &ship_prices,
                        &waypoints,
                        &unassigned_ships_of_fleet,
                        active_trade_routes,
                        &fleet_budget,
                    )
                    .await?;

                    // local mutability, because you can't run async code inside iterator chains.
                    // TODO: make this function pure again, by removing the treasurer... calls
                    let mut new_construction_fleet_tasks = HashMap::new();

                    for potential_construction_task in potential_trading_tasks.iter() {
                        let purchase_details = potential_construction_task.create_purchase_ticket_details();

                        if let Some(ship) = admiral
                            .all_ships
                            .get(&potential_construction_task.ship_symbol)
                        {
                            if ship.cargo.capacity - ship.cargo.units < purchase_details.quantity as i32 {
                                println!("cargo doesn't fit");
                            }
                        }

                        let maybe_purchase_ticket = admiral
                            .treasurer
                            .create_purchase_trade_goods_ticket(
                                fleet_id,
                                purchase_details.trade_good,
                                purchase_details.waypoint_symbol,
                                potential_construction_task.ship_symbol.clone(),
                                purchase_details.quantity,
                                purchase_details.expected_price_per_unit,
                            )
                            .await
                            .ok()
                            .filter(|pt| pt.details.get_units() > 0);

                        let maybe_sell_ticket = if let Some(purchase_ticket) = &maybe_purchase_ticket {
                            // we might not have been able to afford purchasing _all_ units
                            let affordable_units = purchase_ticket.details.get_units();

                            let sell_or_delivery_details = potential_construction_task.create_sell_or_deliver_ticket_details();

                            match sell_or_delivery_details {
                                FinanceTicketDetails::RefuelShip(_) => None,
                                FinanceTicketDetails::PurchaseShip(_) => None,
                                FinanceTicketDetails::PurchaseTradeGoods(_) => None,
                                FinanceTicketDetails::SellTradeGoods(d) => admiral
                                    .treasurer
                                    .create_sell_trade_goods_ticket(
                                        fleet_id,
                                        d.trade_good,
                                        d.waypoint_symbol,
                                        potential_construction_task.ship_symbol.clone(),
                                        affordable_units,
                                        d.expected_price_per_unit,
                                        Some(purchase_ticket.ticket_id),
                                    )
                                    .await
                                    .ok(),
                                FinanceTicketDetails::SupplyConstructionSite(d) => admiral
                                    .treasurer
                                    .create_delivery_construction_material_ticket(
                                        fleet_id,
                                        d.trade_good,
                                        d.waypoint_symbol,
                                        potential_construction_task.ship_symbol.clone(),
                                        affordable_units,
                                        Some(purchase_ticket.ticket_id),
                                    )
                                    .await
                                    .ok(),
                            }
                        } else {
                            None
                        };

                        if let Some((pt, st)) = maybe_purchase_ticket.zip(maybe_sell_ticket) {
                            new_construction_fleet_tasks.insert(potential_construction_task.ship_symbol.clone(), ShipTask::Trade { tickets: vec![pt, st] });
                        }
                    }

                    new_construction_fleet_tasks
                }
                TradingCfg(cfg) => Default::default(),
                MiningCfg(cfg) => MiningFleet::compute_ship_tasks(admiral, cfg, fleet, facts, &unassigned_ships_of_fleet)?,
                SiphoningCfg(cfg) => SiphoningFleet::compute_ship_tasks(admiral, cfg, fleet, facts, &unassigned_ships_of_fleet)?,
            };

            for (ss, task) in computed_new_tasks {
                new_ship_tasks.insert(ss, task);
            }
        }

        let all_ship_symbols = admiral.all_ships.keys().cloned().collect::<HashSet<_>>();
        let already_assigned_ship_symbols = admiral.ship_tasks.keys().cloned().collect::<HashSet<_>>();
        let newly_assigned_ship_symbols = new_ship_tasks.keys().cloned().collect::<HashSet<_>>();
        let ships_with_tasks = already_assigned_ship_symbols
            .union(&newly_assigned_ship_symbols)
            .cloned()
            .collect::<HashSet<_>>();

        let ships_without_task = all_ship_symbols
            .difference(&ships_with_tasks)
            .collect::<HashSet<_>>();

        if ships_without_task.is_empty().not() {
            event!(
                Level::WARN,
                message = "Some ships are missing tasks after pure_compute_ship_tasks",
                num_ships_without_task = ships_without_task.len(),
                num_already_assigned_ship_symbols = already_assigned_ship_symbols.len(),
                num_newly_assigned_ship_symbols = newly_assigned_ship_symbols.len(),
                num_ships_with_tasks = ships_with_tasks.len(),
                ships_without_task = ships_without_task
                    .into_iter()
                    .map(|ss| ss.0.clone())
                    .join(", "),
            );
        }

        if new_ship_tasks
            .iter()
            .any(|(ss, t)| ss == &ShipSymbol("FLWI_TEST-1".to_string()) && matches!(t, ShipTask::ObserveWaypointDetails { .. }))
        {
            eprintln!("command ship got assigned ShipTask::ObserveWaypointDetails - this should not happen");
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

        // increase construction budget to 20M after all ships have been purchased
        if admiral.ship_purchase_demand.is_empty() {
            if admiral.fleet_phase.name == FleetPhaseName::ConstructJumpGate {
                if let Some((construction_fleet_id, _)) = admiral
                    .fleets
                    .iter()
                    .find(|(id, fleet)| matches!(fleet.cfg, ConstructJumpGateCfg(_)))
                {
                    if let Ok(construction_fleet_budget) = admiral
                        .treasurer
                        .get_fleet_budget(construction_fleet_id)
                        .await
                    {
                        let budget_during_construction_phase_after_ship_purchases = 20_000_000.into();
                        if construction_fleet_budget.current_capital != budget_during_construction_phase_after_ship_purchases {
                            admiral
                                .treasurer
                                .set_fleet_budget(construction_fleet_id, budget_during_construction_phase_after_ship_purchases)
                                .await?;

                            admiral
                                .treasurer
                                .transfer_funds_to_fleet_to_top_up_available_capital(construction_fleet_id)
                                .await?;
                        }
                    }
                }
            }
        } else {
            admiral.try_create_ship_purchase_ticket(&ship_prices).await;
        }

        let fleet_budgets = admiral.get_fleet_budgets().await;

        let new_tasks = {
            let active_tickets = admiral
                .treasurer
                .get_active_tickets()
                .await?
                .values()
                .cloned()
                .collect_vec();
            let active_trade_routes = admiral.treasurer.get_active_trade_routes().await?;

            // not pure anymore, since it creates the tickets
            Self::pure_compute_ship_tasks(
                admiral,
                facts,
                latest_market_data,
                ship_prices,
                waypoints,
                &active_tickets,
                &fleet_budgets,
                &HashSet::from_iter(active_trade_routes.iter().cloned()),
            )
            .await?
        };

        if new_tasks.is_empty() {
            let overview = admiral.generate_state_overview().await;
            println!("No new tasks calculated. Current overview: \n{}", overview);
        }
        Ok(new_tasks)
    }

    pub(crate) fn assign_ship_tasks(admiral: &mut FleetAdmiral, ship_tasks: Vec<(ShipSymbol, ShipTask)>) {
        for (ship_symbol, ship_task) in ship_tasks {
            admiral.ship_tasks.insert(ship_symbol, ship_task);
        }
    }

    pub(crate) async fn dismantle_fleets(admiral: &mut FleetAdmiral, fleets_to_dismantle: Vec<FleetId>) -> Result<()> {
        let treasury_credits_before_dismantling = admiral.agent_info_credits().await;
        let treasury_overview_before_dismantling = admiral.generate_budgets_overview().await;
        let ledger_json_before_dismantling = serde_json::to_string(&admiral.treasurer.get_ledger_entries().await?).unwrap_or_default();

        event!(
            Level::INFO,
            message = "Fleet Budgets before dismantling fleets",
            treasury_credits = treasury_credits_before_dismantling.0
        );

        for fleet_id in fleets_to_dismantle {
            admiral.mark_fleet_tasks_as_complete(&fleet_id);
            admiral.remove_ships_from_fleet(&fleet_id);
            admiral.fleets.remove(&fleet_id);
            admiral.treasurer.remove_fleet(&fleet_id).await?;
        }

        let treasury_credits_after_dismantling = admiral.agent_info_credits().await;
        let treasury_overview_after_dismantling = admiral.generate_budgets_overview().await;
        let ledger_json_after_dismantling = serde_json::to_string(&admiral.treasurer.get_ledger_entries().await?).unwrap_or_default();

        event!(
            Level::INFO,
            message = "Fleet Budgets after dismantling fleets",
            treasury_credits = treasury_credits_after_dismantling.0
        );

        println!(
            r#"============================================================================================
Fleet Budgets before dismantling

{}

============================================================================================
Fleet Budgets after dismantling

{}
"#,
            treasury_overview_before_dismantling, treasury_overview_after_dismantling,
        );

        if treasury_credits_before_dismantling != treasury_credits_after_dismantling {
            event!(
                Level::ERROR,
                message = "error during dismantling of fleet budgets - the agent credits differ",
                treasury_credits_before_dismantling = treasury_credits_before_dismantling.0,
                treasury_credits_after_dismantling = treasury_credits_after_dismantling.0
            );

            eprintln!(
                r#"Json entries of all ledger entries before dismantling the fleets:\n{}

Json entries of all ledger entries after dismantling the fleets:\n{}
                "#,
                ledger_json_before_dismantling, ledger_json_after_dismantling,
            );
            panic!("Hello, breakpoint");
        }

        Ok(())
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
        self.get_ships_of_fleet_id(&fleet.id)
    }

    pub(crate) fn get_ships_of_fleet_id(&self, fleet_id: &FleetId) -> Vec<&Ship> {
        self.ship_fleet_assignment
            .iter()
            .filter_map(|(ship_symbol, id)| {
                if fleet_id == id {
                    self.all_ships.get(ship_symbol)
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

    async fn try_create_ship_purchase_ticket(&mut self, ship_prices: &ShipPriceInfo) {
        //println!("Overview before creation of ship purchase ticket:\n{}", self.generate_state_overview().await);
        if let Ok(_) = self.create_ship_purchase_ticket(ship_prices).await {}
        //println!("Overview after creation of ship purchase ticket:\n{}", self.generate_state_overview().await);
    }

    async fn create_ship_purchase_ticket(&mut self, ship_prices: &ShipPriceInfo) -> Result<()> {
        let treasurer = self.treasurer.clone();

        let (ship_type, fleet_task) = self
            .ship_purchase_demand
            .pop_front()
            .ok_or(anyhow!("No ship purchase demands available"))?;

        let maybe_existing_ship_purchase_ticket = treasurer
            .get_active_tickets()
            .await?
            .values()
            .find_map(|t| match &t.details {
                FinanceTicketDetails::PurchaseShip(p) => (p.ship_type == ship_type).then_some(t.clone()),
                _ => None,
            });

        if let Some(ship_purchase_ticket) = maybe_existing_ship_purchase_ticket {
            // put ticket back - we are already purchasing a ship
            self.ship_purchase_demand
                .push_front((ship_type, fleet_task));

            event!(
                Level::INFO,
                message = "There's already an ongoing ship purchase for this ship_type. No Op",
                executing_vessel = ship_purchase_ticket.ship_symbol.to_string(),
                ship_type = ship_type.to_string(),
            );

            //Early return - no op
            return Ok(());
        }

        let (_, (shipyard_wps, price)) = ship_prices
            .get_best_purchase_location(&ship_type)
            .ok_or(anyhow!("No shipyard found selling {ship_type}"))?;

        let ship_price = ((price as f64 * 1.02) as i64).into();

        let beneficiary_fleet = self
            .get_fleet_executing_fleet_task(&fleet_task)
            .ok_or(anyhow!("No fleet found executing task {fleet_task:?}"))?;

        let financing_result = if treasurer
            .get_fleet_budget(&beneficiary_fleet)
            .await?
            .available_capital()
            < ship_price
        {
            let finance_result: FinanceResult = treasurer
                .try_finance_purchase_for_fleet(&beneficiary_fleet, ship_price)
                .await?;

            match finance_result {
                FinanceResult::FleetAlreadyHadSufficientFunds => {
                    event!(
                        Level::INFO,
                        message = "No need to transfer funds to fleet to finance purchase of ship. Already enough funds available",
                        ship_type = ship_type.to_string(),
                    );
                    Ok(())
                }
                FinanceResult::TransferSuccessful { transfer_sum } => {
                    event!(
                        Level::INFO,
                        message = "Transferred funds to fleet to finance purchase of ship",
                        ship_type = ship_type.to_string(),
                        transfer_sum = transfer_sum.to_string()
                    );
                    Ok(())
                }
                FinanceResult::TransferFailed { missing } => {
                    let message = format!("Fleet has insufficient funds and we're unable to finance ship purchase. Missing: {}", missing);
                    event!(Level::DEBUG, message, ship_type = ship_type.to_string(),);
                    Err(anyhow!(message))
                }
            }
        } else {
            Ok(())
        };

        // if we can't finance the whole thing, we push the entry back to the front of the queue
        if let Err(_) = financing_result {
            self.ship_purchase_demand
                .push_front((ship_type, fleet_task));

            return Ok(());
        }

        let purchasing_ship = self
            .get_ship_purchaser(&ship_type, &fleet_task, ship_prices, &shipyard_wps)
            .ok_or(anyhow!("No suitable purchasing ship found for {ship_type}"))?;

        let executing_fleet = self
            .get_fleet_of_ship(&purchasing_ship)
            .map(|f| f.id.clone())
            .ok_or(anyhow!("Ship {} not assigned to any fleet", purchasing_ship))?;

        let create_ticket_result: Result<FinanceTicket> = {
            let ticket: FinanceTicket = treasurer
                .create_ship_purchase_ticket(&beneficiary_fleet, ship_type, ship_price, shipyard_wps.clone(), purchasing_ship.clone())
                .await?;

            Ok(ticket)
        };

        match create_ticket_result {
            Ok(ticket) => {
                event!(
                    Level::INFO,
                    message = "Created ship purchase ticket",
                    ship_type = ship_type.to_string(),
                    purchasing_ship = purchasing_ship.0,
                    beneficiary_fleet = beneficiary_fleet.0,
                    executing_fleet = executing_fleet.0,
                    shipyard_wps = shipyard_wps.0,
                    price = ship_price.0,
                    ticket_id = ticket.ticket_id.to_string(),
                );
                Ok(())
            }

            Err(err) => {
                self.ship_purchase_demand
                    .push_front((ship_type, fleet_task));
                event!(
                    Level::INFO,
                    message = "Unable to create ship purchase ticket - this can be switched to DEBUG once everything's working",
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

    fn get_ship_purchaser(
        &self,
        ship_type: &ShipType,
        for_fleet_task: &FleetTask,
        ship_prices: &ShipPriceInfo,
        shipyard_wps: &WaypointSymbol,
    ) -> Option<ShipSymbol> {
        let system_symbol = match for_fleet_task {
            InitialExploration { system_symbol } => system_symbol,
            ObserveAllWaypointsOfSystemWithStationaryProbes { system_symbol } => system_symbol,
            ConstructJumpGate { system_symbol } => system_symbol,
            TradeProfitably { system_symbol } => system_symbol,
            MineOres { system_symbol } => system_symbol,
            SiphonGases { system_symbol } => system_symbol,
        };

        let purchase_candidates = self
            .stationary_probe_locations
            .iter()
            .find(|spl| spl.waypoint_symbol == shipyard_wps.clone())
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

        let maybe_result = purchase_candidates.first().cloned();

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
                self.get_ships_of_fleet(fleet)
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
        self.treasurer.get_current_agent_credits().await.unwrap()
    }

    async fn mark_transaction_completed_to_treasurer(&mut self, ship_symbol: &ShipSymbol) {
        self.active_trade_ids.remove(ship_symbol);
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

                event!(
                    Level::WARN,
                    message = "No new task for ship found after finishing task",
                    ship = ship.symbol.0.clone(),
                    finished_task = finished_task.to_string()
                );

                Err(anyhow!(
                    "No new task for this ship {} found after finishing task {}",
                    &ship.symbol,
                    finished_task
                ))
            }
        }
        ShipTask::SiphonCarboHydratesAtWaypoint {
            siphoning_waypoint,
            delivery_locations,
            demanded_goods,
        } => {
            // Keep doing, what you're doing
            Ok(NewTaskResult::AssignNewTaskToShip {
                ship_symbol: ship.symbol.clone(),
                task: ShipTask::SiphonCarboHydratesAtWaypoint {
                    siphoning_waypoint: siphoning_waypoint.clone(),
                    delivery_locations: delivery_locations.clone(),
                    demanded_goods: demanded_goods.clone(),
                },
            })
        }
        ShipTask::SurveyMiningSite { .. } => {
            event!(
                Level::WARN,
                "The ShipTask::SurveyMiningSite task ended - this shouldn't happen, since it's an infinite task"
            );
            Ok(NewTaskResult::AssignNewTaskToShip {
                ship_symbol: ship.symbol.clone(),
                task: finished_task.clone(),
            })
        }
        ShipTask::MineMaterialsAtWaypoint { .. } => Ok(NewTaskResult::AssignNewTaskToShip {
            ship_symbol: ship.symbol.clone(),
            task: finished_task.clone(),
        }),

        ShipTask::HaulMiningGoods { .. } => Ok(NewTaskResult::AssignNewTaskToShip {
            ship_symbol: ship.symbol.clone(),
            task: finished_task.clone(),
        }),
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
                InitialExploration { system_symbol } => Some(SystemSpawningCfg(SystemSpawningFleetConfig {
                    system_symbol: system_symbol.clone(),
                    marketplace_waypoints_of_interest: fleet_decision_facts.marketplaces_of_interest.clone(),
                    shipyard_waypoints_of_interest: fleet_decision_facts.shipyards_of_interest.clone(),
                    desired_fleet_config,
                })),
                ObserveAllWaypointsOfSystemWithStationaryProbes { system_symbol } => Some(MarketObservationCfg(MarketObservationFleetConfig {
                    system_symbol: system_symbol.clone(),
                    marketplace_waypoints_of_interest: fleet_decision_facts.marketplaces_of_interest.clone(),
                    shipyard_waypoints_of_interest: fleet_decision_facts.shipyards_of_interest.clone(),
                    desired_fleet_config,
                })),
                ConstructJumpGate { system_symbol } => Some(ConstructJumpGateCfg(ConstructJumpGateFleetConfig {
                    system_symbol: system_symbol.clone(),
                    jump_gate_waypoint: fleet_decision_facts
                        .construction_site
                        .clone()
                        .expect("construction_site")
                        .symbol,
                    desired_fleet_config,
                })),
                TradeProfitably { system_symbol } => Some(TradingCfg(TradingFleetConfig {
                    system_symbol: system_symbol.clone(),
                    materialized_supply_chain: None,
                    desired_fleet_config,
                })),
                MineOres { system_symbol } => Some(MiningCfg(MiningFleetConfig {
                    system_symbol: system_symbol.clone(),
                    mining_waypoint: fleet_decision_facts.engineered_asteroid.clone(),
                    desired_fleet_config,
                })),
                SiphonGases { system_symbol } => Some(SiphoningCfg(SiphoningFleetConfig {
                    system_symbol: system_symbol.clone(),
                    siphoning_waypoint: fleet_decision_facts.gas_giant.clone(),
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

    if !has_collected_all_waypoint_details_once {
        create_initial_exploration_fleet_phase(&system_symbol, num_shipyards_of_interest)
    } else if !is_jump_gate_done {
        create_construction_fleet_phase(&system_symbol, num_shipyards_of_interest, num_marketplaces_ex_shipyards)
    } else if is_jump_gate_done {
        create_trade_profitably_fleet_phase(system_symbol, num_waypoints_of_interest)
    } else {
        unimplemented!("this shouldn't happen - think harder")
    }
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
            ShipType::SHIP_LIGHT_HAULER,
            ShipType::SHIP_MINING_DRONE,
            ShipType::SHIP_MINING_DRONE,
            ShipType::SHIP_MINING_DRONE,
            ShipType::SHIP_SURVEYOR,
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

    let is_miner = || ship.mounts.iter().any(|m| m.symbol.is_mining_laser());
    let is_siphoner = || ship.mounts.iter().any(|m| m.symbol.is_gas_siphon());
    let is_surveyor = || ship.mounts.iter().any(|m| m.symbol.is_surveyor());
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
                let current_ship_type = get_ship_type_of_ship(s).expect("role_to_ship_type_mapping");
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
        )?;
        Some(materialized_chain)
    } else {
        None
    };

    let gas_giant = waypoints_of_system
        .iter()
        .find(|wp| wp.r#type == WaypointType::GAS_GIANT)
        .unwrap()
        .clone()
        .symbol;
    let engineered_asteroid = waypoints_of_system
        .iter()
        .find(|wp| wp.r#type == WaypointType::ENGINEERED_ASTEROID)
        .unwrap()
        .clone()
        .symbol;

    Ok(FleetDecisionFacts {
        marketplaces_of_interest: marketplace_symbols_of_interest.clone(),
        marketplaces_with_up_to_date_infos,
        shipyards_of_interest: shipyard_symbols_of_interest.clone(),
        shipyards_with_up_to_date_infos: diff_waypoint_symbols(&shipyard_symbols_of_interest, &shipyards_to_explore),
        construction_site: maybe_construction_site,
        ships,
        materialized_supply_chain,
        agent_info,
        gas_giant,
        engineered_asteroid,
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
        let ship_type = get_ship_type_of_ship(s).unwrap_or_else(|_| panic!("role_to_ship_type_mapping for ShipFrameSymbol {}", &s.frame.symbol.to_string()));
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
