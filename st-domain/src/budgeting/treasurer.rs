use crate::budgeting::budgeting::{
    FinanceError, FleetBudget, FundingSource, PurchaseShipTransactionGoal, PurchaseTradeGoodsTransactionGoal, SellTradeGoodsTransactionGoal, TicketFinancials,
    TicketStatus, TicketType, TransactionEvent, TransactionGoal, TransactionTicket,
};
use crate::budgeting::credits::Credits;
use crate::{
    Fleet, FleetDecisionFacts, FleetId, FleetPhase, FleetPhaseName, FleetTask, Ship, ShipPriceInfo, ShipSymbol, ShipType, TicketId, TransactionTicketId,
    WaypointSymbol,
};
use chrono::{DateTime, Utc};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::ops::Not;

pub trait Treasurer {
    type Error;

    fn agent_credits(&self) -> Credits;

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
    ) -> Result<TicketId, Self::Error>;

    fn get_ticket(&self, id: TicketId) -> Result<TransactionTicket, Self::Error>;

    fn fund_fleet_for_next_purchase(&mut self, source: FundingSource) -> Result<(), Self::Error>;

    fn fund_ticket(&mut self, id: TicketId, source: FundingSource) -> Result<(), Self::Error>;

    fn start_ticket_execution(&mut self, id: TicketId) -> Result<(), Self::Error>;

    fn record_event(&mut self, id: TicketId, event: TransactionEvent) -> Result<(), Self::Error>;

    fn update_goal(&mut self, id: TicketId, goal_index: usize, updated_goal: TransactionGoal) -> Result<(), Self::Error>;

    fn complete_ticket(&mut self, id: TicketId) -> Result<(), Self::Error>;

    fn get_active_ticket_for_vessel(&self, vessel_id: &ShipSymbol) -> Result<Option<TransactionTicket>, Self::Error>;

    fn get_ship_purchase_tickets(&self) -> Vec<TransactionTicket>;

    fn create_fleet_budget(&mut self, fleet_id: FleetId, initial_capital: Credits, credits: Credits) -> Result<(), Self::Error>;

    fn get_fleet_budget(&self, fleet_id: &FleetId) -> Result<FleetBudget, Self::Error>;

    fn redistribute_distribute_fleet_budgets(
        &mut self,
        fleet_phase: &FleetPhase,
        fleet_tasks: &[(FleetId, FleetTask)],
        ship_fleet_assignment: &HashMap<ShipSymbol, FleetId>,
        ship_price_info: &ShipPriceInfo,
        all_next_ship_purchases: &[(ShipType, FleetTask)],
    ) -> Result<(), Self::Error>;

    fn return_excess_capital_to_treasurer(&mut self, fleet_id: &FleetId) -> Result<(), Self::Error>;

    fn top_up_available_capital(&mut self, fleet_id: &FleetId) -> Result<(), Self::Error>;

    fn give_all_treasury_to_fleet(&mut self, fleet: &FleetId) -> Result<(), Self::Error>;

    fn try_fund_fleet_and_ticket(&mut self, funding_source: FundingSource, ticket_id: TicketId) -> Result<(), Self::Error>;

    fn create_ship_purchase_ticket(
        &mut self,
        ship_type: &ShipType,
        purchasing_ship: &ShipSymbol,
        initiating_fleet: &FleetId,
        beneficiary_fleet: &FleetId,
        executing_fleet: &FleetId,
        estimated_cost: Credits,
        shipyard_waypoint: &WaypointSymbol,
    ) -> Result<TicketId, Self::Error>;
}

// In-memory implementation of the EventDrivenFinanceSystem trait

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InMemoryTreasurer {
    pub tickets: HashMap<TicketId, TransactionTicket>,
    fleet_budgets: HashMap<FleetId, FleetBudget>,
    pub treasury: Credits,
    active_tickets_by_vessel: HashMap<ShipSymbol, TicketId>,
    completed_tickets: HashMap<TicketId, TransactionTicket>,
}

impl InMemoryTreasurer {
    pub fn new(initial_treasury: Credits) -> Self {
        Self {
            tickets: HashMap::new(),
            fleet_budgets: HashMap::new(),
            treasury: initial_treasury,
            active_tickets_by_vessel: HashMap::new(),
            completed_tickets: HashMap::new(),
        }
    }

    pub fn get_fleet_budgets(&self) -> HashMap<FleetId, FleetBudget> {
        self.fleet_budgets.clone()
    }

    pub fn get_fleet_trades_overview(&self) -> HashMap<FleetId, Vec<TransactionTicket>> {
        self.tickets
            .iter()
            .map(|(_, ticket)| (ticket.executing_fleet.clone(), ticket.clone()))
            .into_group_map()
    }

    fn calculate_required_capital(&self, goals: &[TransactionGoal]) -> Credits {
        let mut required = Credits::new(0);

        for goal in goals {
            match goal {
                TransactionGoal::PurchaseTradeGoods(PurchaseTradeGoodsTransactionGoal {
                    target_quantity,
                    estimated_price_per_unit: estimated_price,
                    ..
                }) => {
                    required += Credits::new(i64::from(*target_quantity) * estimated_price.0);
                }

                TransactionGoal::SellTradeGoods(_) => {}
                TransactionGoal::PurchaseShip(PurchaseShipTransactionGoal { estimated_cost, .. }) => {
                    required += estimated_cost.clone();
                }
            }
        }

        required
    }

    fn calculate_projected_profit(&self, goals: &[TransactionGoal]) -> Credits {
        let mut revenue = Credits::new(0);
        let mut costs = Credits::new(0);

        for goal in goals {
            match goal {
                TransactionGoal::PurchaseTradeGoods(PurchaseTradeGoodsTransactionGoal {
                    target_quantity,
                    estimated_price_per_unit: estimated_price,
                    ..
                }) => {
                    costs += Credits::new(i64::from(*target_quantity) * estimated_price.0);
                }
                TransactionGoal::SellTradeGoods(SellTradeGoodsTransactionGoal {
                    target_quantity,
                    estimated_price_per_unit: estimated_price,
                    ..
                }) => {
                    revenue += Credits::new(i64::from(*target_quantity) * estimated_price.0);
                }
                TransactionGoal::PurchaseShip(PurchaseShipTransactionGoal { estimated_cost, .. }) => {
                    costs += *estimated_cost;
                }
            }
        }

        revenue - costs
    }
}

impl Treasurer for InMemoryTreasurer {
    type Error = FinanceError;

    fn agent_credits(&self) -> Credits {
        self.treasury
            + self
                .fleet_budgets
                .iter()
                .map(|(_, budget)| budget.available_capital.0 + budget.operating_reserve.0)
                .sum::<i64>()
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
    ) -> Result<TicketId, Self::Error> {
        // Check if fleets exist
        if !self.fleet_budgets.contains_key(executing_fleet)
            || !self.fleet_budgets.contains_key(initiating_fleet)
            || !self.fleet_budgets.contains_key(beneficiary_fleet)
        {
            return Err(FinanceError::FleetNotFound);
        }

        let required_capital = self.calculate_required_capital(&goals);
        let projected_profit = self.calculate_projected_profit(&goals);

        let ticket_id = TicketId::new();
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

    fn get_ticket(&self, id: TicketId) -> Result<TransactionTicket, Self::Error> {
        let either_ticket = self
            .tickets
            .get(&id)
            .cloned()
            .or(self.completed_tickets.get(&id).cloned())
            .ok_or(FinanceError::TicketNotFound);
        match either_ticket {
            Ok(ticket) => Ok(ticket),
            Err(err) => {
                eprintln!("Ticket not found");
                Err(err)
            }
        }
    }

    fn fund_fleet_for_next_purchase(&mut self, source: FundingSource) -> Result<(), Self::Error> {
        // Check that the fleet exists and has enough funds
        let fleet_budget = self
            .fleet_budgets
            .get_mut(&source.source_fleet)
            .ok_or(FinanceError::FleetNotFound)?;

        let diff = source.amount - fleet_budget.available_capital;

        if diff.0 > 0 {
            if self.treasury >= diff {
                fleet_budget.available_capital += diff;
                self.treasury -= diff;
                Ok(())
            } else {
                Err(FinanceError::InsufficientFunds)
            }
        } else {
            // no need to top up the fleet's budget
            Ok(())
        }
    }

    fn fund_ticket(&mut self, id: TicketId, source: FundingSource) -> Result<(), Self::Error> {
        // Get the ticket
        let ticket = self
            .tickets
            .get_mut(&id)
            .ok_or(FinanceError::TicketNotFound)?;

        // Check that the fleet exists and has enough funds
        let fleet_budget = self
            .fleet_budgets
            .get_mut(&source.source_fleet)
            .ok_or(FinanceError::FleetNotFound)?;

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
        if ticket.status != TicketStatus::Funded {
            eprintln!("Ticket wasn't funded");
            return Err(FinanceError::InsufficientFunds);
        }

        Ok(())
    }

    fn start_ticket_execution(&mut self, id: TicketId) -> Result<(), Self::Error> {
        let ticket = self
            .tickets
            .get_mut(&id)
            .ok_or(FinanceError::TicketNotFound)?;

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
        self.active_tickets_by_vessel
            .insert(ticket.executing_vessel.clone(), id);

        Ok(())
    }

    fn record_event(&mut self, id: TicketId, event: TransactionEvent) -> Result<(), Self::Error> {
        let ticket = self
            .tickets
            .get_mut(&id)
            .ok_or(FinanceError::TicketNotFound)?;

        // Process the event
        ticket.update_from_event(&event);

        // Check if all required goals are complete after this event
        if ticket.all_required_goals_completed() && ticket.status == TicketStatus::InProgress {
            self.complete_ticket(id)?;
        }

        Ok(())
    }

    fn update_goal(&mut self, id: TicketId, goal_index: usize, updated_goal: TransactionGoal) -> Result<(), Self::Error> {
        let ticket = self
            .tickets
            .get_mut(&id)
            .ok_or(FinanceError::TicketNotFound)?;

        if goal_index >= ticket.goals.len() {
            return Err(FinanceError::GoalNotFound);
        }

        ticket.goals[goal_index] = updated_goal;
        ticket.updated_at = Utc::now();

        Ok(())
    }

    fn complete_ticket(&mut self, id: TicketId) -> Result<(), Self::Error> {
        if self.completed_tickets.contains_key(&id) {
            return Ok(());
        }

        let ticket = self
            .tickets
            .get_mut(&id)
            .ok_or(FinanceError::TicketNotFound)?;

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
        if unspent_funds.is_negative() {
            eprintln!("unspent_funds.is_negative");
            return Err(FinanceError::InvalidState);
        }
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
                }

                // Record this financial reconciliation
                let reconciliation_event = TransactionEvent::FundsReturned {
                    timestamp: Utc::now(),
                    unspent_funds_returned: unspent_funds,
                    revenue_returned: earned_revenue,
                    net_profit: profit,
                };

                let ticket = self
                    .tickets
                    .get_mut(&id)
                    .ok_or(FinanceError::TicketNotFound)?;
                ticket.update_from_event(&reconciliation_event);

                println!(
                    "Finance reconciliation: Returned {} unspent funds and {} revenue to fleet {:?}. Net profit: {}",
                    unspent_funds, earned_revenue, beneficiary_fleet, profit
                );
            }

            TicketType::ShipPurchase => {
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

                let ticket = self
                    .tickets
                    .get_mut(&id)
                    .ok_or(FinanceError::TicketNotFound)?;
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

                let ticket = self
                    .tickets
                    .get_mut(&id)
                    .ok_or(FinanceError::TicketNotFound)?;
                ticket.update_from_event(&asset_event);
            }

            // Handle other ticket types similarly
            TicketType::DeliverConstructionMaterial | TicketType::Exploration => {
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

                let ticket = self
                    .tickets
                    .get_mut(&id)
                    .ok_or(FinanceError::TicketNotFound)?;
                ticket.update_from_event(&return_event);
            }
        }

        // Remove from active tickets
        let ticket = self
            .tickets
            .get(&id)
            .cloned()
            .ok_or(FinanceError::TicketNotFound)?;
        self.active_tickets_by_vessel
            .remove(&ticket.executing_vessel);
        self.tickets.remove(&id);
        self.completed_tickets.insert(id, ticket);

        Ok(())
    }

    fn get_active_ticket_for_vessel(&self, vessel_id: &ShipSymbol) -> Result<Option<TransactionTicket>, Self::Error> {
        if let Some(ticket_id) = self.active_tickets_by_vessel.get(vessel_id) {
            Ok(Some(self.get_ticket(*ticket_id)?))
        } else {
            Ok(None)
        }
    }

    fn get_ship_purchase_tickets(&self) -> Vec<TransactionTicket> {
        self.tickets
            .iter()
            .filter_map(|(_, t)| match &t {
                TransactionTicket { ticket_type, .. } => match ticket_type {
                    TicketType::ShipPurchase => true,
                    TicketType::Trading => false,
                    TicketType::DeliverConstructionMaterial => false,
                    TicketType::Exploration => false,
                }
                .then_some(t.clone()),
            })
            .collect_vec()
    }

    fn create_fleet_budget(&mut self, fleet_id: FleetId, funding_capital: Credits, operating_reserve: Credits) -> Result<(), Self::Error> {
        // Check if we have enough in treasury
        if funding_capital < operating_reserve {
            return Err(FinanceError::InvalidState);
        }

        if self.treasury < (funding_capital) {
            return Err(FinanceError::InsufficientFunds);
        }

        if self.fleet_budgets.contains_key(&fleet_id) {
            return Err(FinanceError::FleetAlreadyBudgeted);
        }

        // Create new fleet budget
        let budget = FleetBudget {
            fleet_id: fleet_id.clone(),
            total_capital: funding_capital - operating_reserve,
            available_capital: funding_capital - operating_reserve,
            operating_reserve,
            earmarked_funds: HashMap::new(),
            asset_value: Credits::new(0),
            funded_transactions: HashSet::new(),
            beneficiary_transactions: HashSet::new(),
            executing_transactions: HashSet::new(),
        };

        // Deduct from treasury
        self.treasury -= funding_capital;

        // Store budget
        self.fleet_budgets.insert(fleet_id, budget);

        Ok(())
    }

    fn get_fleet_budget(&self, fleet_id: &FleetId) -> Result<FleetBudget, Self::Error> {
        self.fleet_budgets
            .get(fleet_id)
            .cloned()
            .ok_or(FinanceError::FleetNotFound)
    }

    fn redistribute_distribute_fleet_budgets(
        &mut self,
        fleet_phase: &FleetPhase,
        fleet_tasks: &[(FleetId, FleetTask)],
        ship_fleet_assignment: &HashMap<ShipSymbol, FleetId>,
        ship_price_info: &ShipPriceInfo,
        all_next_ship_purchases: &[(ShipType, FleetTask)],
    ) -> Result<(), Self::Error> {
        for (_, budget) in self.fleet_budgets.iter_mut() {
            // TODO: clean up properly (cancel and clear tickets etc)
            self.treasury += budget.available_capital
        }
        self.fleet_budgets.clear();

        let (new_ship_types, tasks_of_new_ships): (Vec<_>, Vec<_>) = all_next_ship_purchases.iter().cloned().unzip();

        let fleet_task_lookup = fleet_tasks
            .iter()
            .map(|(id, fleet_task)| (fleet_task.clone(), id.clone()))
            .collect::<HashMap<_, _>>();

        let market_observation_fleet_id = fleet_tasks
            .iter()
            .find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::ObserveAllWaypointsOfSystemWithStationaryProbes { .. }).then_some(id))
            .unwrap();

        match fleet_phase.name {
            FleetPhaseName::InitialExploration => {
                let command_ship_fleet_id = fleet_tasks
                    .iter()
                    .find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::InitialExploration { .. }).then_some(id))
                    .unwrap();

                // plenty of budget for fuel on initial round
                let command_fleet_budget = self.treasury.min(Credits::new(25_000));
                let command_ship_reserve = Credits::new(25_000);
                self.create_fleet_budget(command_ship_fleet_id.clone(), command_fleet_budget, command_ship_reserve)?;

                let other_fleets = fleet_tasks
                    .iter()
                    .map(|(id, _)| id.clone())
                    .filter(|id| self.fleet_budgets.contains_key(id).not())
                    .collect_vec();

                for other_fleet_id in other_fleets {
                    self.create_fleet_budget(other_fleet_id, Credits::new(0), Credits::new(0))?
                }
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

                let construction_fleet_id = fleet_tasks
                    .iter()
                    .find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::ConstructJumpGate { .. }).then_some(id))
                    .unwrap();

                let fleet_sizes = ship_fleet_assignment
                    .iter()
                    .counts_by(|(_ss, fleet_id)| fleet_id.clone());

                let num_trading_ships = fleet_sizes
                    .get(construction_fleet_id)
                    .cloned()
                    .unwrap_or_default() as i64;
                let trading_budget = Credits::new((trading_budget_per_ship.0 + reserve_per_ship.0) * num_trading_ships);

                let construction_fleet_budget = self.treasury.min(trading_budget);
                let reserve_for_trading_fleet = Credits::new(reserve_per_ship.0 * num_trading_ships);

                self.create_fleet_budget(construction_fleet_id.clone(), construction_fleet_budget, reserve_for_trading_fleet)?;

                let rest_budget = self.treasury;
                let ship_purchases_running_total = ship_price_info.get_running_total_of_all_ship_purchases(new_ship_types);
                let affordable_ships = ship_purchases_running_total
                    .iter()
                    .take_while(|(_, _, _, running_total)| (*running_total as i64) < rest_budget.0)
                    .collect_vec();

                // we create a budget for the rest of the fleets
                let other_fleets = fleet_tasks
                    .iter()
                    .map(|(id, _)| id.clone())
                    .filter(|id| self.fleet_budgets.contains_key(id).not())
                    .collect_vec();
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

    fn return_excess_capital_to_treasurer(&mut self, fleet_id: &FleetId) -> Result<(), Self::Error> {
        let fleet_budget = self
            .fleet_budgets
            .get_mut(&fleet_id)
            .ok_or(FinanceError::FleetNotFound)?;

        let excess = fleet_budget.available_capital - fleet_budget.total_capital;

        if excess.is_positive() {
            self.treasury += excess;
            fleet_budget.available_capital -= excess;
        }
        Ok(())
    }

    fn top_up_available_capital(&mut self, fleet_id: &FleetId) -> Result<(), Self::Error> {
        let fleet_budget = self
            .fleet_budgets
            .get_mut(&fleet_id)
            .ok_or(FinanceError::FleetNotFound)?;

        let diff = fleet_budget.total_capital - fleet_budget.available_capital;

        if diff.is_positive() {
            let affordable_sum = self.treasury.min(diff);
            self.treasury -= affordable_sum;
            fleet_budget.available_capital += diff;
        }

        Ok(())
    }

    fn give_all_treasury_to_fleet(&mut self, beneficiary_fleet: &FleetId) -> Result<(), Self::Error> {
        if let Some(beneficiary_fleet_budget) = self.fleet_budgets.get_mut(beneficiary_fleet) {
            beneficiary_fleet_budget.available_capital += self.treasury;

            self.treasury = Credits::new(0);
            Ok(())
        } else {
            Err(FinanceError::FleetNotFound)
        }
    }

    fn try_fund_fleet_and_ticket(&mut self, funding_source: FundingSource, ticket_id: TicketId) -> Result<(), Self::Error> {
        match self.fund_fleet_for_next_purchase(funding_source.clone()) {
            Ok(_) => {
                // Fleet has sufficient funds
                match self.fund_ticket(ticket_id, funding_source) {
                    Ok(_) => Ok(()),
                    Err(err) => Err(err),
                }
            }
            Err(err) => Err(err),
        }
    }

    fn create_ship_purchase_ticket(
        &mut self,
        ship_type: &ShipType,
        purchasing_ship: &ShipSymbol,
        initiating_fleet: &FleetId,
        beneficiary_fleet: &FleetId,
        executing_fleet: &FleetId,
        estimated_cost: Credits,
        shipyard_waypoint: &WaypointSymbol,
    ) -> Result<TicketId, Self::Error> {
        let ticket_id = self.create_ticket(
            TicketType::ShipPurchase,
            purchasing_ship.clone(),
            executing_fleet,
            initiating_fleet,
            beneficiary_fleet,
            vec![TransactionGoal::PurchaseShip(PurchaseShipTransactionGoal {
                id: TransactionTicketId::new(),
                ship_type: *ship_type,
                estimated_cost,
                has_been_purchased: false,
                beneficiary_fleet: beneficiary_fleet.clone(),
                shipyard_waypoint: shipyard_waypoint.clone(),
            })],
            Default::default(),
            0.0,
        )?;

        Ok(ticket_id)
    }
}
