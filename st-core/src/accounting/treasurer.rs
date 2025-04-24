use crate::accounting::budgeting::{
    FinanceError, FleetBudget, FundingSource, TicketFinancials, TicketStatus, TicketType, TransactionEvent, TransactionGoal, TransactionTicket,
};
use crate::accounting::credits::Credits;
use crate::fleet::fleet;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use st_domain::{Fleet, FleetDecisionFacts, FleetId, FleetPhase, FleetPhaseName, FleetTask, Ship, ShipPriceInfo, ShipSymbol, ShipType, WaypointSymbol};
use std::collections::{HashMap, HashSet};
use std::ops::Not;
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

    fn create_fleet_budget(&mut self, fleet_id: FleetId, initial_capital: Credits, credits: Credits) -> Result<(), Self::Error>;

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

    fn give_all_treasury_to_fleet(&mut self, fleet: &FleetId) -> Result<(), Self::Error>;
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

    fn give_all_treasury_to_fleet(&mut self, beneficiary_fleet: &FleetId) -> Result<(), Self::Error> {
        if let Some(beneficiary_fleet_budget) = self.fleet_budgets.get_mut(beneficiary_fleet) {
            beneficiary_fleet_budget.available_capital += self.treasury;
            beneficiary_fleet_budget.total_capital += self.treasury;

            self.treasury = Credits::new(0);
            Ok(())
        } else {
            Err(FinanceError::FleetNotFound)
        }
    }

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

    fn create_fleet_budget(&mut self, fleet_id: FleetId, initial_capital: Credits, operating_reserve: Credits) -> Result<(), Self::Error> {
        // Check if we have enough in treasury
        if initial_capital < operating_reserve {
            return Err(FinanceError::InvalidState);
        }

        if self.treasury < (initial_capital) {
            return Err(FinanceError::InsufficientFunds);
        }

        if self.fleet_budgets.contains_key(&fleet_id) {
            return Err(FinanceError::FleetAlreadyBudgeted);
        }

        // Create new fleet budget
        let budget = FleetBudget {
            fleet_id: fleet_id.clone(),
            total_capital: initial_capital - operating_reserve,
            available_capital: initial_capital,
            operating_reserve,
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

        let fleet_task_lookup = fleet_tasks.iter().map(|(id, fleet_task)| (fleet_task.clone(), id.clone())).collect::<HashMap<_, _>>();

        let market_observation_fleet_id = fleet_tasks
            .iter()
            .find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::ObserveAllWaypointsOfSystemWithStationaryProbes { .. }).then_some(id))
            .unwrap();

        match fleet_phase.name {
            FleetPhaseName::InitialExploration => {
                // we start with one probe and want to keep 50k for trading. Let's try to reserve budget for purchasing one probe per shipyard
                let command_ship_fleet_id =
                    fleet_tasks.iter().find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::CollectMarketInfosOnce { .. }).then_some(id)).unwrap();

                let command_fleet_budget = self.treasury.min(Credits::new(51_000));
                let command_ship_reserve = Credits::new(1_000);
                self.create_fleet_budget(command_ship_fleet_id.clone(), command_fleet_budget, command_ship_reserve)?;

                let rest_budget = self.treasury;
                // let ships_within_budget = ship_price_info.get_all_ship_purchases_within_budget(new_ship_types, rest_budget.0);
                // println!("{} ships are within budget: {:?}", ships_within_budget.len(), &ships_within_budget);
                //
                // let probes_budget = ships_within_budget.iter().map(|(_, _, price)| *price as i64).sum();

                self.create_fleet_budget(market_observation_fleet_id.clone(), rest_budget, Credits::new(0))?;
            }
            FleetPhaseName::ConstructJumpGate => {
                // Strategy:
                // Keep money for trading
                // try to reserve money for ship purchases and assign it to the respective fleets (they'll fund the tickets for the ships)
                // after that, we decide
                // need more ships ?    ==> keep the budget
                // all ships purchased? ==> assign everything to the construction fleet (to fund the jump gate construction)

                let reserve_per_ship = Credits::new(1_000);
                let trading_budget_per_ship = Credits::new(75_000);

                let construction_fleet_id =
                    fleet_tasks.iter().find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::ConstructJumpGate { .. }).then_some(id)).unwrap();

                let fleet_sizes = ship_fleet_assignment.iter().counts_by(|(_ss, fleet_id)| fleet_id.clone());

                let num_trading_ships = fleet_sizes.get(construction_fleet_id).cloned().unwrap_or_default() as i64;
                let trading_budget = Credits::new((trading_budget_per_ship.0 + reserve_per_ship.0) * num_trading_ships);

                let construction_fleet_budget = self.treasury.min(trading_budget);
                let reserve_for_trading_fleet = Credits::new(reserve_per_ship.0 * num_trading_ships);

                self.create_fleet_budget(construction_fleet_id.clone(), construction_fleet_budget, reserve_for_trading_fleet)?;

                let rest_budget = self.treasury;
                let ship_purchases_running_total = ship_price_info.get_running_total_of_all_ship_purchases(new_ship_types);
                let affordable_ships =
                    ship_purchases_running_total.iter().take_while(|(_, _, _, running_total)| (*running_total as i64) < rest_budget.0).collect_vec();

                let ship_purchases_per_fleet = affordable_ships
                    .iter()
                    .zip(tasks_of_new_ships)
                    .map(|((ship_type, wps, price, _), fleet_task)| (fleet_task, *ship_type, wps.clone(), *price))
                    .into_group_map_by(|tup| tup.0.clone())
                    .into_iter()
                    .map(|(fleet_task, entries)| {
                        let fleet_task_total = entries.iter().map(|(_, _, _, p)| p).sum::<u32>();
                        let fleet_id = fleet_task_lookup.get(&fleet_task).unwrap();
                        (fleet_task, fleet_id.clone(), entries, fleet_task_total)
                    })
                    .collect_vec();

                for (_fleet_task, fleet_id, _entries, fleet_task_total) in ship_purchases_per_fleet {
                    self.create_fleet_budget(fleet_id.clone(), Credits::new(fleet_task_total as i64), Credits::new(0))?;
                }

                // we create a budget for the rest of the fleets
                let other_fleets = fleet_tasks.iter().map(|(id, _)| id.clone()).filter(|id| self.fleet_budgets.contains_key(id).not()).collect_vec();
                for other_fleet_id in other_fleets {
                    self.create_fleet_budget(other_fleet_id, Credits::new(0), Credits::new(0))?
                }

                if affordable_ships.is_empty() {
                    self.give_all_treasury_to_fleet(construction_fleet_id)?;
                }
            }
            FleetPhaseName::TradeProfitably => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::accounting::budgeting::{FinanceError, FundingSource, TicketType, TransactionEvent, TransactionGoal};
    use crate::accounting::credits::Credits;
    use crate::accounting::treasurer::{InMemoryTreasurer, Treasurer};
    use crate::fleet::fleet;
    use crate::fleet::fleet::{collect_fleet_decision_facts, compute_fleet_phase_with_tasks, FleetAdmiral};
    use crate::fleet::fleet_runner::FleetRunner;
    use crate::st_client::StClientTrait;
    use crate::universe_server::universe_server::{InMemoryUniverse, InMemoryUniverseClient};
    use anyhow::Result;
    use chrono::{Duration, Utc};
    use itertools::Itertools;
    use st_domain::{Fleet, FleetId, FleetTask, ShipSymbol, ShipType, TradeGoodSymbol, WaypointSymbol};
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
    async fn distribute_budget_among_fleets_based_for_initial_exploration_fleet_phase() -> Result<()> {
        let (bmc, client) = get_test_universe().await;
        let agent = client.get_agent().await?.data;
        let system_symbol = agent.headquarters.system_symbol();

        let mut finance = InMemoryTreasurer::new(Credits::new(agent.credits));

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

        let command_ship_fleet_id =
            fleet_tasks.iter().find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::CollectMarketInfosOnce { .. }).then_some(id)).unwrap();

        let market_observation_fleet_id = fleet_tasks
            .iter()
            .find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::ObserveAllWaypointsOfSystemWithStationaryProbes { .. }).then_some(id))
            .unwrap();

        finance.redistribute_distribute_fleet_budgets(&fleet_phase, &facts, &fleets, &fleet_tasks, &ship_fleet_assignment, &ship_map, &ship_price_info)?;
        let command_fleet_budget = finance.get_fleet_budget(command_ship_fleet_id)?;
        let market_observation_fleet_budget = finance.get_fleet_budget(market_observation_fleet_id)?;

        assert_eq!(Credits::new(50_000), command_fleet_budget.total_capital);
        assert_eq!(Credits::new(1_000), command_fleet_budget.operating_reserve);
        assert_eq!(Credits::new(124_000), market_observation_fleet_budget.total_capital);

        Ok(())
    }

    #[test(tokio::test)]
    //#[tokio::test] // for accessing runtime-infos with tokio-conso&le
    async fn distribute_budget_among_fleets_based_for_create_construction_fleet_phase() -> Result<()> {
        let (bmc, client) = get_test_universe().await;
        let agent = client.get_agent().await?.data;
        let system_symbol = agent.headquarters.system_symbol();

        let mut finance = InMemoryTreasurer::new(Credits::new(agent.credits));

        FleetRunner::load_and_store_initial_data_in_bmcs(Arc::clone(&client), Arc::clone(&bmc)).await.expect("FleetRunner::load_and_store_initial_data");

        let facts = collect_fleet_decision_facts(Arc::clone(&bmc), &system_symbol).await?;

        let marketplaces_of_interest: HashSet<WaypointSymbol> = HashSet::from_iter(facts.marketplaces_of_interest.iter().cloned());
        let shipyards_of_interest: HashSet<WaypointSymbol> = HashSet::from_iter(facts.shipyards_of_interest.iter().cloned());
        let marketplaces_ex_shipyards: Vec<WaypointSymbol> = marketplaces_of_interest.difference(&shipyards_of_interest).cloned().collect_vec();

        let fleet_phase = fleet::create_construction_fleet_phase(&system_symbol, facts.shipyards_of_interest.len(), marketplaces_ex_shipyards.len());

        let (fleets, fleet_tasks): (Vec<Fleet>, Vec<(FleetId, FleetTask)>) =
            fleet::compute_fleets_with_tasks(&facts, &Default::default(), &Default::default(), &fleet_phase);

        let ship_map = facts.ships.iter().map(|s| (s.symbol.clone(), s.clone())).collect();

        let ship_price_info = bmc.shipyard_bmc().get_latest_ship_prices(&Ctx::Anonymous, &system_symbol).await?;

        let ship_fleet_assignment = FleetAdmiral::assign_ships(&fleet_tasks, &ship_map, &fleet_phase.shopping_list_in_order);

        let construction_fleet_id =
            fleet_tasks.iter().find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::ConstructJumpGate { .. }).then_some(id)).unwrap();

        let market_observation_fleet_id = fleet_tasks
            .iter()
            .find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::ObserveAllWaypointsOfSystemWithStationaryProbes { .. }).then_some(id))
            .unwrap();

        let mining_fleet_id = fleet_tasks.iter().find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::MineOres { .. }).then_some(id)).unwrap();

        let siphoning_fleet_id = fleet_tasks.iter().find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::SiphonGases { .. }).then_some(id)).unwrap();

        finance.redistribute_distribute_fleet_budgets(&fleet_phase, &facts, &fleets, &fleet_tasks, &ship_fleet_assignment, &ship_map, &ship_price_info)?;
        let construction_fleet_budget = finance.get_fleet_budget(construction_fleet_id)?;
        let market_observation_fleet_budget = finance.get_fleet_budget(market_observation_fleet_id)?;
        let mining_fleet_budget = finance.get_fleet_budget(mining_fleet_id)?;
        let siphoning_fleet_budget = finance.get_fleet_budget(siphoning_fleet_id)?;

        assert_eq!(Credits::new(75_000), construction_fleet_budget.total_capital);
        assert_eq!(Credits::new(1_000), construction_fleet_budget.operating_reserve);

        assert_eq!(Credits::new(75_000), market_observation_fleet_budget.total_capital); // 3 probes à 25k each (estimated for now, since we don't have accurate marketdata yet)
        assert_eq!(Credits::new(0), market_observation_fleet_budget.operating_reserve);

        assert_eq!(Credits::new(0), mining_fleet_budget.total_capital); // 3 probes à 25k each (estimated for now, since we don't have accurate marketdata yet)
        assert_eq!(Credits::new(0), mining_fleet_budget.operating_reserve);

        assert_eq!(Credits::new(0), siphoning_fleet_budget.total_capital); // 3 probes à 25k each (estimated for now, since we don't have accurate marketdata yet)
        assert_eq!(Credits::new(0), siphoning_fleet_budget.operating_reserve);

        Ok(())
    }

    #[test(tokio::test)]
    async fn distribute_budget_and_execute_trades_for_ship_purchase_in_construction_phase() -> Result<(), anyhow::Error> {
        let (bmc, client) = get_test_universe().await;
        let agent = client.get_agent().await?.data;
        let system_symbol = agent.headquarters.system_symbol();

        // Initialize with 200,000 credits for testing - a reasonable starting amount
        let mut finance = InMemoryTreasurer::new(Credits::new(200_000));

        FleetRunner::load_and_store_initial_data_in_bmcs(Arc::clone(&client), Arc::clone(&bmc)).await.expect("FleetRunner::load_and_store_initial_data");

        let facts = collect_fleet_decision_facts(Arc::clone(&bmc), &system_symbol).await?;

        let marketplaces_of_interest: HashSet<WaypointSymbol> = HashSet::from_iter(facts.marketplaces_of_interest.iter().cloned());
        let shipyards_of_interest: HashSet<WaypointSymbol> = HashSet::from_iter(facts.shipyards_of_interest.iter().cloned());
        let marketplaces_ex_shipyards: Vec<WaypointSymbol> = marketplaces_of_interest.difference(&shipyards_of_interest).cloned().collect_vec();

        // Create a construction fleet phase
        let fleet_phase = fleet::create_construction_fleet_phase(&system_symbol, facts.shipyards_of_interest.len(), marketplaces_ex_shipyards.len());

        let (fleets, fleet_tasks): (Vec<Fleet>, Vec<(FleetId, FleetTask)>) =
            fleet::compute_fleets_with_tasks(&facts, &Default::default(), &Default::default(), &fleet_phase);

        let ship_map = facts.ships.iter().map(|s| (s.symbol.clone(), s.clone())).collect();

        let ship_price_info = bmc.shipyard_bmc().get_latest_ship_prices(&Ctx::Anonymous, &system_symbol).await?;

        let ship_fleet_assignment = FleetAdmiral::assign_ships(&fleet_tasks, &ship_map, &fleet_phase.shopping_list_in_order);

        // Find our fleets
        let construction_fleet_id =
            fleet_tasks.iter().find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::ConstructJumpGate { .. }).then_some(id)).unwrap();

        let mining_fleet_id = fleet_tasks.iter().find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::MineOres { .. }).then_some(id)).unwrap();

        // Distribute the budgets based on fleet phase
        finance.redistribute_distribute_fleet_budgets(&fleet_phase, &facts, &fleets, &fleet_tasks, &ship_fleet_assignment, &ship_map, &ship_price_info)?;

        // Check the initial budgets
        let construction_budget_before = finance.get_fleet_budget(construction_fleet_id)?;
        let mining_budget_before = finance.get_fleet_budget(mining_fleet_id)?;

        println!(
            "Initial construction fleet budget: available={}, total={}",
            construction_budget_before.available_capital, construction_budget_before.total_capital
        );
        println!(
            "Initial mining fleet budget: available={}, total={}",
            mining_budget_before.available_capital, mining_budget_before.total_capital
        );

        // Get a ship to use for execution - just picking the first available ship
        let executing_ship = facts.ships.first().unwrap().symbol.clone();

        // Generate some waypoints for testing
        let source_waypoint = facts.marketplaces_of_interest.first().unwrap().clone();
        let destination_waypoint = facts.marketplaces_of_interest.last().unwrap().clone();

        // Step 1: Execute a profitable trade with the construction fleet
        println!("Executing a profitable trade...");
        let profit = execute_profitable_trade(
            &mut finance,
            &executing_ship,
            construction_fleet_id,
            &source_waypoint,
            &destination_waypoint,
            TradeGoodSymbol::ADVANCED_CIRCUITRY, // High-value good
            50,                                  // Quantity
            Credits::new(500),                   // Buy price
            Credits::new(900),                   // Sell price (80% profit)
        )
        .await?;

        println!("Trade completed with profit: {}", profit);

        // Check the updated budget after trade
        let construction_budget_after_trade = finance.get_fleet_budget(construction_fleet_id)?;
        println!(
            "Construction fleet budget after trade: available={}, total={}",
            construction_budget_after_trade.available_capital, construction_budget_after_trade.total_capital
        );

        // Step 2: Execute a ship purchase for the mining fleet
        println!("Executing a ship purchase...");
        execute_ship_purchase(
            &mut finance,
            &executing_ship,
            construction_fleet_id, // Construction fleet is buying
            mining_fleet_id,       // For the mining fleet
            &facts.shipyards_of_interest.first().unwrap().clone(),
            ShipType::SHIP_MINING_DRONE,
            Credits::new(25_000),
        )
        .await?;

        // Check the updated budgets after ship purchase
        let construction_budget_after_purchase = finance.get_fleet_budget(construction_fleet_id)?;
        let mining_budget_after_purchase = finance.get_fleet_budget(mining_fleet_id)?;

        println!(
            "Construction fleet budget after ship purchase: available={}, total={}",
            construction_budget_after_purchase.available_capital, construction_budget_after_purchase.total_capital
        );
        println!(
            "Mining fleet budget after ship purchase: available={}, total={}, asset_value={}",
            mining_budget_after_purchase.available_capital, mining_budget_after_purchase.total_capital, mining_budget_after_purchase.asset_value
        );

        // Verify the results
        assert!(
            construction_budget_after_trade.total_capital > construction_budget_before.total_capital,
            "Trading should increase the fleet's total capital"
        );

        assert!(
            construction_budget_after_purchase.available_capital < construction_budget_after_trade.available_capital,
            "Ship purchase should reduce available capital"
        );

        assert!(
            mining_budget_after_purchase.asset_value > mining_budget_before.asset_value,
            "Ship purchase should increase the receiving fleet's asset value"
        );

        Ok(())
    }

    // Helper function to execute a profitable trade
    async fn execute_profitable_trade(
        treasurer: &mut InMemoryTreasurer,
        executing_ship: &ShipSymbol,
        executing_fleet: &FleetId,
        source_waypoint: &WaypointSymbol,
        destination_waypoint: &WaypointSymbol,
        good: TradeGoodSymbol,
        quantity: u32,
        buy_price: Credits,
        sell_price: Credits,
    ) -> Result<Credits, FinanceError> {
        // Create a ticket for trading
        let ticket_id = treasurer.create_ticket(
            TicketType::Trading,
            executing_ship.clone(),
            executing_fleet,
            executing_fleet, // Initiating fleet is the same as executing
            executing_fleet, // Beneficiary fleet is the same as executing
            vec![
                // Purchase goal
                TransactionGoal::Purchase {
                    good: good.clone(),
                    target_quantity: quantity,
                    available_quantity: Some(quantity),
                    acquired_quantity: 0,
                    estimated_price: buy_price,
                    max_acceptable_price: Some(buy_price * 2),
                    source_waypoint: source_waypoint.clone(),
                },
                // Sell goal
                TransactionGoal::Sell {
                    good: good.clone(),
                    target_quantity: quantity,
                    sold_quantity: 0,
                    estimated_price: sell_price,
                    min_acceptable_price: Some(sell_price / 2),
                    destination_waypoint: destination_waypoint.clone(),
                },
            ],
            Utc::now() + Duration::hours(1),
            10.0, // High priority
        )?;

        // Fund the ticket
        let required_capital = quantity as i64 * buy_price.0;
        treasurer.fund_ticket(
            ticket_id,
            FundingSource {
                source_fleet: executing_fleet.clone(),
                amount: Credits::new(required_capital),
            },
        )?;

        // Start execution
        treasurer.start_ticket_execution(ticket_id)?;

        // Record purchase event
        let purchase_event = TransactionEvent::GoodsPurchased {
            timestamp: Utc::now(),
            waypoint: source_waypoint.clone(),
            good: good.clone(),
            quantity,
            price_per_unit: buy_price,
            total_cost: Credits::new(quantity as i64 * buy_price.0),
        };
        treasurer.record_event(ticket_id, purchase_event)?;

        // Record sell event
        let sell_event = TransactionEvent::GoodsSold {
            timestamp: Utc::now() + Duration::minutes(10),
            waypoint: destination_waypoint.clone(),
            good,
            quantity,
            price_per_unit: sell_price,
            total_revenue: Credits::new(quantity as i64 * sell_price.0),
        };
        treasurer.record_event(ticket_id, sell_event)?;

        // The ticket should be automatically completed after all goals are fulfilled
        // Let's get the ticket to check the final profit
        let ticket = treasurer.get_ticket(ticket_id)?;
        Ok(ticket.financials.current_profit)
    }

    // Helper function to execute a ship purchase
    async fn execute_ship_purchase(
        treasurer: &mut InMemoryTreasurer,
        executing_ship: &ShipSymbol,
        executing_fleet: &FleetId,
        beneficiary_fleet: &FleetId,
        shipyard_waypoint: &WaypointSymbol,
        ship_type: ShipType,
        estimated_cost: Credits,
    ) -> Result<(), FinanceError> {
        // Create a ticket for ship purchase
        let ticket_id = treasurer.create_ticket(
            TicketType::FleetExpansion,
            executing_ship.clone(),
            executing_fleet,
            executing_fleet,   // Initiating fleet is the same as executing
            beneficiary_fleet, // The fleet that will receive the ship
            vec![TransactionGoal::ShipPurchase {
                ship_type: ship_type.clone(),
                estimated_cost,
                has_been_purchased: false,
                beneficiary_fleet: beneficiary_fleet.clone(),
                shipyard_waypoint: shipyard_waypoint.clone(),
            }],
            Utc::now() + Duration::hours(1),
            5.0, // Medium priority
        )?;

        // Fund the ticket
        treasurer.fund_ticket(
            ticket_id,
            FundingSource {
                source_fleet: executing_fleet.clone(),
                amount: estimated_cost,
            },
        )?;

        // Start execution
        treasurer.start_ticket_execution(ticket_id)?;

        // Record ship purchase event
        let purchase_event = TransactionEvent::ShipPurchased {
            timestamp: Utc::now(),
            waypoint: shipyard_waypoint.clone(),
            ship_type,
            ship_id: ShipSymbol("TEST".to_string()), // Generate a random ship ID
            total_cost: estimated_cost,
            beneficiary_fleet: beneficiary_fleet.clone(),
        };
        treasurer.record_event(ticket_id, purchase_event)?;

        // The ticket should be automatically completed after the goal is fulfilled
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
