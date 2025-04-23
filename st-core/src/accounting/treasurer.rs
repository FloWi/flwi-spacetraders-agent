use crate::accounting::budgeting::{
    FinanceError, FleetBudget, FundingSource, TicketFinancials, TicketStatus, TicketType, TransactionEvent, TransactionGoal, TransactionTicket,
};
use crate::accounting::credits::Credits;
use crate::fleet::fleet;
use chrono::{DateTime, Utc};
use st_domain::{Fleet, FleetDecisionFacts, FleetId, FleetPhase, FleetPhaseName, FleetTask, Ship, ShipPriceInfo, ShipSymbol, ShipType, WaypointSymbol};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub trait Treasurer {
    type Error;

    fn create_ticket(
        &mut self,
        ticket_type: TicketType,
        executing_vessel: ShipSymbol,
        executing_fleet: &FleetId,
        initiating_fleet: &FleetId,
        beneficiary_fleet: &FleetId,
        goals: Vec<TransactionGoal>,
        estimated_completion: DateTime<Utc>,
        priority: f64,
    ) -> Result<Uuid, Self::Error>;

    fn get_ticket(&self, id: Uuid) -> Result<TransactionTicket, Self::Error>;

    fn fund_ticket(&mut self, id: Uuid, source: FundingSource) -> Result<(), Self::Error>;

    fn start_ticket_execution(&mut self, id: Uuid) -> Result<(), Self::Error>;

    fn record_event(&mut self, id: Uuid, event: TransactionEvent) -> Result<(), Self::Error>;

    fn update_goal(&mut self, id: Uuid, goal_index: usize, updated_goal: TransactionGoal) -> Result<(), Self::Error>;

    fn skip_goal(&mut self, id: Uuid, goal_index: usize, reason: String) -> Result<(), Self::Error>;

    fn complete_ticket(&mut self, id: Uuid) -> Result<(), Self::Error>;

    fn get_active_ticket_for_vessel(&self, vessel_id: &ShipSymbol) -> Result<Option<TransactionTicket>, Self::Error>;

    fn create_fleet_budget(&mut self, fleet_id: FleetId, initial_capital: Credits) -> Result<(), Self::Error>;

    fn get_fleet_budget(&self, fleet_id: &FleetId) -> Result<FleetBudget, Self::Error>;

    fn redistribute_distribute_fleet_budgets(
        &mut self,
        fleet_phase: &FleetPhase,
        facts: &FleetDecisionFacts,
        fleets: &[Fleet],
        fleet_tasks: &[(FleetId, FleetTask)],
        ship_fleet_assignment: &HashMap<ShipSymbol, FleetId>,
        ship_map: &HashMap<ShipSymbol, Ship>,
        ship_price_info: &ShipPriceInfo,
    ) -> Result<(), Self::Error>;
}

// In-memory implementation of the EventDrivenFinanceSystem trait
pub struct InMemoryTreasurer {
    tickets: HashMap<Uuid, TransactionTicket>,
    fleet_budgets: HashMap<FleetId, FleetBudget>,
    treasury: Credits,
    active_tickets_by_vessel: HashMap<ShipSymbol, Uuid>,
}

impl InMemoryTreasurer {
    pub fn new(initial_treasury: Credits) -> Self {
        Self {
            tickets: HashMap::new(),
            fleet_budgets: HashMap::new(),
            treasury: initial_treasury,
            active_tickets_by_vessel: HashMap::new(),
        }
    }

    fn calculate_required_capital(&self, goals: &[TransactionGoal]) -> Credits {
        let mut required = Credits::new(0);

        for goal in goals {
            match goal {
                TransactionGoal::Purchase {
                    target_quantity,
                    estimated_price,
                    ..
                } => {
                    required += Credits::new(i64::from(*target_quantity) * estimated_price.0);
                }
                TransactionGoal::Refuel {
                    target_fuel_level,
                    current_fuel_level,
                    estimated_cost_per_unit,
                    ..
                } => {
                    let fuel_needed = target_fuel_level - current_fuel_level;
                    required += Credits::new(i64::from(fuel_needed) * estimated_cost_per_unit.0);
                }
                _ => {}
            }
        }

        required
    }

    fn calculate_projected_profit(&self, goals: &[TransactionGoal]) -> Credits {
        let mut revenue = Credits::new(0);
        let mut costs = Credits::new(0);

        for goal in goals {
            match goal {
                TransactionGoal::Purchase {
                    target_quantity,
                    estimated_price,
                    ..
                } => {
                    costs += Credits::new(i64::from(*target_quantity) * estimated_price.0);
                }
                TransactionGoal::Sell {
                    target_quantity,
                    estimated_price,
                    ..
                } => {
                    revenue += Credits::new(i64::from(*target_quantity) * estimated_price.0);
                }
                TransactionGoal::Refuel {
                    target_fuel_level,
                    current_fuel_level,
                    estimated_cost_per_unit,
                    ..
                } => {
                    let fuel_needed = target_fuel_level - current_fuel_level;
                    costs += Credits::new(i64::from(fuel_needed) * estimated_cost_per_unit.0);
                }
                TransactionGoal::ShipPurchase { estimated_cost, .. } => {
                    costs += *estimated_cost;
                }
            }
        }

        revenue - costs
    }
}

impl Treasurer for InMemoryTreasurer {
    type Error = FinanceError;

    fn create_ticket(
        &mut self,
        ticket_type: TicketType,
        executing_vessel: ShipSymbol,
        executing_fleet: &FleetId,
        initiating_fleet: &FleetId,
        beneficiary_fleet: &FleetId,
        goals: Vec<TransactionGoal>,
        estimated_completion: DateTime<Utc>,
        priority: f64,
    ) -> Result<Uuid, Self::Error> {
        // Check if fleets exist
        if !self.fleet_budgets.contains_key(executing_fleet)
            || !self.fleet_budgets.contains_key(initiating_fleet)
            || !self.fleet_budgets.contains_key(beneficiary_fleet)
        {
            return Err(FinanceError::FleetNotFound);
        }

        let required_capital = self.calculate_required_capital(&goals);
        let projected_profit = self.calculate_projected_profit(&goals);

        let ticket_id = Uuid::new_v4();
        let now = Utc::now();

        let ticket = TransactionTicket {
            id: ticket_id,
            ticket_type,
            status: TicketStatus::Planned,
            executing_vessel: executing_vessel.clone(),
            executing_fleet: executing_fleet.clone(),
            initiating_fleet: initiating_fleet.clone(),
            beneficiary_fleet: beneficiary_fleet.clone(),
            goals,
            financials: TicketFinancials {
                required_capital,
                allocated_capital: Credits::new(0),
                funding_sources: Vec::new(),
                spent_capital: Credits::new(0),
                earned_revenue: Credits::new(0),
                current_profit: Credits::new(0),
                projected_profit,
                operating_expenses: Credits::new(0),
            },
            created_at: now,
            updated_at: now,
            estimated_completion,
            completed_at: None,
            priority,
            current_waypoint: None,
            event_history: vec![TransactionEvent::TicketCreated { timestamp: now }],
            metadata: HashMap::new(),
        };

        self.tickets.insert(ticket_id, ticket);

        // Update fleet budget records to reference this ticket
        if let Some(budget) = self.fleet_budgets.get_mut(executing_fleet) {
            budget.executing_transactions.insert(ticket_id);
        }

        if let Some(budget) = self.fleet_budgets.get_mut(initiating_fleet) {
            if initiating_fleet != executing_fleet {
                budget.beneficiary_transactions.insert(ticket_id);
            }
        }

        if let Some(budget) = self.fleet_budgets.get_mut(beneficiary_fleet) {
            if beneficiary_fleet != executing_fleet && beneficiary_fleet != initiating_fleet {
                budget.beneficiary_transactions.insert(ticket_id);
            }
        }

        Ok(ticket_id)
    }

    fn get_ticket(&self, id: Uuid) -> Result<TransactionTicket, Self::Error> {
        self.tickets.get(&id).cloned().ok_or(FinanceError::TicketNotFound)
    }

    fn fund_ticket(&mut self, id: Uuid, source: FundingSource) -> Result<(), Self::Error> {
        // Get the ticket
        let ticket = self.tickets.get_mut(&id).ok_or(FinanceError::TicketNotFound)?;

        // Check that the fleet exists and has enough funds
        let fleet_budget = self.fleet_budgets.get_mut(&source.source_fleet).ok_or(FinanceError::FleetNotFound)?;

        if fleet_budget.available_capital < source.amount {
            return Err(FinanceError::InsufficientFunds);
        }

        // Update the fleet budget
        fleet_budget.available_capital -= source.amount;
        fleet_budget.funded_transactions.insert(id);

        // Update the ticket
        ticket.financials.allocated_capital += source.amount;
        ticket.financials.funding_sources.push(source.clone());

        // If fully funded, update ticket status
        if ticket.financials.allocated_capital >= ticket.financials.required_capital {
            ticket.status = TicketStatus::Funded;
        }

        // Record the funding event
        let event = TransactionEvent::TicketFunded { timestamp: Utc::now(), source };

        ticket.update_from_event(&event);

        Ok(())
    }

    fn start_ticket_execution(&mut self, id: Uuid) -> Result<(), Self::Error> {
        let ticket = self.tickets.get_mut(&id).ok_or(FinanceError::TicketNotFound)?;

        // Check if ticket is funded
        if ticket.status != TicketStatus::Funded {
            return Err(FinanceError::InvalidState);
        }

        // Record execution started event
        let event = TransactionEvent::ExecutionStarted { timestamp: Utc::now() };

        // Update ticket status
        ticket.status = TicketStatus::InProgress;
        ticket.update_from_event(&event);

        // Track active ticket for this vessel
        self.active_tickets_by_vessel.insert(ticket.executing_vessel.clone(), id);

        Ok(())
    }

    fn record_event(&mut self, id: Uuid, event: TransactionEvent) -> Result<(), Self::Error> {
        let ticket = self.tickets.get_mut(&id).ok_or(FinanceError::TicketNotFound)?;

        // Process the event
        ticket.update_from_event(&event);

        // Check if all required goals are complete after this event
        if ticket.all_required_goals_completed() && ticket.status == TicketStatus::InProgress {
            self.complete_ticket(id)?;
        }

        Ok(())
    }

    fn update_goal(&mut self, id: Uuid, goal_index: usize, updated_goal: TransactionGoal) -> Result<(), Self::Error> {
        let ticket = self.tickets.get_mut(&id).ok_or(FinanceError::TicketNotFound)?;

        if goal_index >= ticket.goals.len() {
            return Err(FinanceError::GoalNotFound);
        }

        ticket.goals[goal_index] = updated_goal;
        ticket.updated_at = Utc::now();

        Ok(())
    }

    fn skip_goal(&mut self, id: Uuid, goal_index: usize, reason: String) -> Result<(), Self::Error> {
        let ticket = self.tickets.get_mut(&id).ok_or(FinanceError::TicketNotFound)?;

        if goal_index >= ticket.goals.len() {
            return Err(FinanceError::GoalNotFound);
        }

        let goal = &ticket.goals[goal_index];

        // Only optional goals can be skipped
        if !goal.is_optional() {
            return Err(FinanceError::InvalidOperation);
        }

        // Skip the goal by force-completing it based on its type
        match &mut ticket.goals[goal_index] {
            TransactionGoal::Purchase {
                target_quantity,
                acquired_quantity,
                ..
            } => {
                *acquired_quantity = *target_quantity;
            }
            TransactionGoal::Sell {
                target_quantity,
                sold_quantity,
                ..
            } => {
                *sold_quantity = *target_quantity;
            }
            TransactionGoal::Refuel {
                target_fuel_level,
                current_fuel_level,
                ..
            } => {
                *current_fuel_level = *target_fuel_level;
            }
            TransactionGoal::ShipPurchase { .. } => {}
        }

        // Record skip event
        let event = TransactionEvent::GoalSkipped {
            timestamp: Utc::now(),
            goal_index,
            reason,
        };

        ticket.update_from_event(&event);

        Ok(())
    }

    fn complete_ticket(&mut self, id: Uuid) -> Result<(), Self::Error> {
        let ticket = self.tickets.get_mut(&id).ok_or(FinanceError::TicketNotFound)?;

        if ticket.completed_at.is_some() {
            return Ok(());
        }

        // Check if all required goals are completed
        if !ticket.all_required_goals_completed() {
            return Err(FinanceError::InvalidState);
        }

        // Mark ticket as completed
        ticket.status = TicketStatus::Completed;
        ticket.completed_at = Some(Utc::now());

        // Calculate financial reconciliation
        let unspent_funds = ticket.financials.allocated_capital - ticket.financials.spent_capital;
        let earned_revenue = ticket.financials.earned_revenue;
        let profit = ticket.financials.current_profit;

        // Record completion event
        let event = TransactionEvent::TicketCompleted {
            timestamp: Utc::now(),
            final_profit: profit,
        };

        ticket.update_from_event(&event);

        // Handle finance reconciliation based on ticket type
        match ticket.ticket_type {
            TicketType::Trading => {
                // For trading tickets, return unspent funds and revenue to the beneficiary fleet
                let beneficiary_fleet = ticket.beneficiary_fleet.clone();

                if let Some(budget) = self.fleet_budgets.get_mut(&beneficiary_fleet) {
                    // Return both unspent allocated funds and earned revenue
                    budget.available_capital += unspent_funds + earned_revenue;

                    // Only add the net profit to the fleet's total capital
                    budget.total_capital += profit;
                }

                // Record this financial reconciliation
                let reconciliation_event = TransactionEvent::FundsReturned {
                    timestamp: Utc::now(),
                    unspent_funds_returned: unspent_funds,
                    revenue_returned: earned_revenue,
                    net_profit: profit,
                };

                let ticket = self.tickets.get_mut(&id).ok_or(FinanceError::TicketNotFound)?;
                ticket.update_from_event(&reconciliation_event);

                println!(
                    "Finance reconciliation: Returned {} unspent funds and {} revenue to fleet {:?}. Net profit: {}",
                    unspent_funds, earned_revenue, beneficiary_fleet, profit
                );
            }

            TicketType::FleetExpansion => {
                // For ship purchases, we need to:
                // 1. Return unspent funds to the funding fleet
                // 2. Add the ship value as an asset to the beneficiary fleet

                // Get the ship value from the spent capital
                let ship_value = ticket.financials.spent_capital;

                // Clone the funding sources to avoid borrow checker issues
                let funding_sources: Vec<FundingSource> = ticket.financials.funding_sources.clone();
                let beneficiary_fleet = ticket.beneficiary_fleet.clone();

                // Return unspent funds to the funding fleet
                for source in funding_sources {
                    if let Some(funding_budget) = self.fleet_budgets.get_mut(&source.source_fleet) {
                        funding_budget.available_capital += unspent_funds;

                        println!(
                            "Finance reconciliation: Returned {} unspent funds to funding fleet {:?}.",
                            unspent_funds, source.source_fleet
                        );
                    }
                }

                // Record the return of unspent funds
                let return_event = TransactionEvent::FundsReturned {
                    timestamp: Utc::now(),
                    unspent_funds_returned: unspent_funds,
                    revenue_returned: Credits::new(0), // No revenue for ship purchases
                    net_profit: -ship_value,           // Negative profit because it's an expense
                };

                let ticket = self.tickets.get_mut(&id).ok_or(FinanceError::TicketNotFound)?;
                ticket.update_from_event(&return_event);

                // Add the ship value as an asset to the beneficiary fleet
                if let Some(beneficiary_budget) = self.fleet_budgets.get_mut(&beneficiary_fleet) {
                    beneficiary_budget.asset_value += ship_value;

                    println!("Asset reconciliation: Added ship worth {} to {:?} fleet assets.", ship_value, beneficiary_fleet);
                }

                // Record the asset transfer
                let asset_event = TransactionEvent::AssetTransferred {
                    timestamp: Utc::now(),
                    asset_type: "SHIP".to_string(),
                    asset_value: ship_value,
                    to_fleet: beneficiary_fleet,
                };

                let ticket = self.tickets.get_mut(&id).ok_or(FinanceError::TicketNotFound)?;
                ticket.update_from_event(&asset_event);
            }

            // Handle other ticket types similarly
            TicketType::Construction | TicketType::Exploration => {
                // Clone the funding sources to avoid borrow checker issues
                let funding_sources: Vec<FundingSource> = ticket.financials.funding_sources.clone();

                // For other types, just return unspent funds to the funding fleet
                for source in funding_sources {
                    if let Some(funding_budget) = self.fleet_budgets.get_mut(&source.source_fleet) {
                        funding_budget.available_capital += unspent_funds;
                    }
                }

                // Record the return of unspent funds
                let return_event = TransactionEvent::FundsReturned {
                    timestamp: Utc::now(),
                    unspent_funds_returned: unspent_funds,
                    revenue_returned: earned_revenue,
                    net_profit: profit,
                };

                let ticket = self.tickets.get_mut(&id).ok_or(FinanceError::TicketNotFound)?;
                ticket.update_from_event(&return_event);
            }
        }

        // Remove from active tickets
        let ticket = self.tickets.get(&id).ok_or(FinanceError::TicketNotFound)?;
        self.active_tickets_by_vessel.remove(&ticket.executing_vessel);

        Ok(())
    }

    fn get_active_ticket_for_vessel(&self, vessel_id: &ShipSymbol) -> Result<Option<TransactionTicket>, Self::Error> {
        if let Some(ticket_id) = self.active_tickets_by_vessel.get(vessel_id) {
            Ok(Some(self.get_ticket(*ticket_id)?))
        } else {
            Ok(None)
        }
    }

    fn create_fleet_budget(&mut self, fleet_id: FleetId, initial_capital: Credits) -> Result<(), Self::Error> {
        // Check if we have enough in treasury
        if self.treasury < initial_capital {
            return Err(FinanceError::InsufficientFunds);
        }

        // Create new fleet budget
        let budget = FleetBudget {
            fleet_id: fleet_id.clone(),
            total_capital: initial_capital,
            available_capital: initial_capital,
            operating_reserve: Credits::new(0),
            earmarked_funds: HashMap::new(),
            asset_value: Credits::new(0),
            funded_transactions: HashSet::new(),
            beneficiary_transactions: HashSet::new(),
            executing_transactions: HashSet::new(),
        };

        // Deduct from treasury
        self.treasury -= initial_capital;

        // Store budget
        self.fleet_budgets.insert(fleet_id, budget);

        Ok(())
    }

    fn get_fleet_budget(&self, fleet_id: &FleetId) -> Result<FleetBudget, Self::Error> {
        self.fleet_budgets.get(fleet_id).cloned().ok_or(FinanceError::FleetNotFound)
    }

    fn redistribute_distribute_fleet_budgets(
        &mut self,
        fleet_phase: &FleetPhase,
        facts: &FleetDecisionFacts,
        fleets: &[Fleet],
        fleet_tasks: &[(FleetId, FleetTask)],
        ship_fleet_assignment: &HashMap<ShipSymbol, FleetId>,
        ship_map: &HashMap<ShipSymbol, Ship>,
        ship_price_info: &ShipPriceInfo,
    ) -> Result<(), Self::Error> {
        for (_, budget) in self.fleet_budgets.iter_mut() {
            // TODO: clean up properly (cancel and clear tickets etc)
            self.treasury += budget.total_capital
        }
        self.fleet_budgets.clear();

        let all_next_ship_purchases = fleet::get_all_next_ship_purchases(ship_map, fleet_phase);
        let (new_ship_types, tasks_of_new_ships): (Vec<_>, Vec<_>) = all_next_ship_purchases.iter().cloned().unzip();

        let market_observation_fleet_id = fleet_tasks
            .iter()
            .find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::ObserveAllWaypointsOfSystemWithStationaryProbes { .. }).then_some(id))
            .unwrap();

        match fleet_phase.name {
            FleetPhaseName::InitialExploration => {
                // we start with one probe and want to keep 50k for trading. Let's try to reserve budget for purchasing one probe per shipyard
                let command_ship_fleet_id =
                    fleet_tasks.iter().find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::CollectMarketInfosOnce { .. }).then_some(id)).unwrap();

                let command_fleet_budget = self.treasury.min(Credits::new(50_000));
                self.create_fleet_budget(command_ship_fleet_id.clone(), command_fleet_budget)?;

                let rest_budget = self.treasury;
                // let ships_within_budget = ship_price_info.get_all_ship_purchases_within_budget(new_ship_types, rest_budget.0);
                // println!("{} ships are within budget: {:?}", ships_within_budget.len(), &ships_within_budget);
                //
                // let probes_budget = ships_within_budget.iter().map(|(_, _, price)| *price as i64).sum();

                self.create_fleet_budget(market_observation_fleet_id.clone(), rest_budget)?;
            }
            FleetPhaseName::ConstructJumpGate => {}
            FleetPhaseName::TradeProfitably => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::accounting::credits::Credits;
    use crate::accounting::treasurer::{InMemoryTreasurer, Treasurer};
    use crate::fleet::fleet;
    use crate::fleet::fleet::{collect_fleet_decision_facts, compute_fleet_phase_with_tasks, FleetAdmiral};
    use crate::fleet::fleet_runner::FleetRunner;
    use crate::st_client::StClientTrait;
    use crate::universe_server::universe_server::{InMemoryUniverse, InMemoryUniverseClient};
    use anyhow::Result;
    use itertools::Itertools;
    use st_domain::{Fleet, FleetId, FleetTask, WaypointSymbol};
    use st_store::bmc::jump_gate_bmc::InMemoryJumpGateBmc;
    use st_store::bmc::ship_bmc::{InMemoryShips, InMemoryShipsBmc};
    use st_store::bmc::{Bmc, InMemoryBmc};
    use st_store::shipyard_bmc::InMemoryShipyardBmc;
    use st_store::trade_bmc::InMemoryTradeBmc;
    use st_store::{
        Ctx, InMemoryAgentBmc, InMemoryConstructionBmc, InMemoryFleetBmc, InMemoryMarketBmc, InMemoryStatusBmc, InMemorySupplyChainBmc, InMemorySystemsBmc,
    };
    use std::collections::HashSet;
    use std::sync::Arc;
    use test_log::test;

    #[test(tokio::test)]
    //#[tokio::test] // for accessing runtime-infos with tokio-conso&le
    async fn distribute_budget_among_fleets_based_on_fleet_phase() -> Result<()> {
        let mut finance = InMemoryTreasurer::new(Credits::new(1_000_000));

        let (bmc, client) = get_test_universe().await;
        let system_symbol = client.get_agent().await?.data.headquarters.system_symbol();

        FleetRunner::load_and_store_initial_data_in_bmcs(Arc::clone(&client), Arc::clone(&bmc)).await.expect("FleetRunner::load_and_store_initial_data");

        let facts = collect_fleet_decision_facts(Arc::clone(&bmc), &system_symbol).await?;

        let marketplaces_of_interest: HashSet<WaypointSymbol> = HashSet::from_iter(facts.marketplaces_of_interest.iter().cloned());
        let shipyards_of_interest: HashSet<WaypointSymbol> = HashSet::from_iter(facts.shipyards_of_interest.iter().cloned());
        let marketplaces_ex_shipyards: Vec<WaypointSymbol> = marketplaces_of_interest.difference(&shipyards_of_interest).cloned().collect_vec();

        let fleet_phase = fleet::create_initial_exploration_fleet_phase(&system_symbol, shipyards_of_interest.len());
        // let fleet_phase = fleet::create_construction_fleet_phase(&system_symbol, facts.shipyards_of_interest.len(), marketplaces_ex_shipyards.len());

        let (fleets, fleet_tasks): (Vec<Fleet>, Vec<(FleetId, FleetTask)>) =
            fleet::compute_fleets_with_tasks(&facts, &Default::default(), &Default::default(), &fleet_phase);

        let ship_map = facts.ships.iter().map(|s| (s.symbol.clone(), s.clone())).collect();

        let ship_price_info = bmc.shipyard_bmc().get_latest_ship_prices(&Ctx::Anonymous, &system_symbol).await?;

        let ship_fleet_assignment = FleetAdmiral::assign_ships(&fleet_tasks, &ship_map, &fleet_phase.shopping_list_in_order);

        finance.redistribute_distribute_fleet_budgets(&fleet_phase, &facts, &fleets, &fleet_tasks, &ship_fleet_assignment, &ship_map, &ship_price_info)?;

        assert_eq!(facts.ships.len(), 2);

        Ok(())
    }

    async fn get_test_universe() -> (Arc<dyn Bmc>, Arc<dyn StClientTrait>) {
        let in_memory_universe = InMemoryUniverse::from_snapshot("tests/assets/universe_snapshot.json").expect("InMemoryUniverse::from_snapshot");

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

        (bmc, client)
    }
}
