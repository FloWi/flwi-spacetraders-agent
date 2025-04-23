use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use st_core::accounting::budgeting::{
    FinanceError, FleetBudget, FundingSource, TicketFinancials, TicketStatus, TicketType, TransactionEvent, TransactionGoal, TransactionTicket,
};
use st_core::accounting::credits::Credits;
use st_domain::{FleetId, ShipSymbol, ShipType, TradeGoodSymbol, WaypointSymbol};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;
use uuid::Uuid;
// Import the event-driven finance system types (simplified versions included here)

pub trait EventDrivenFinanceSystem {
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
}

// In-memory implementation of the EventDrivenFinanceSystem trait
struct InMemoryEventFinanceSystem {
    tickets: HashMap<Uuid, TransactionTicket>,
    fleet_budgets: HashMap<FleetId, FleetBudget>,
    treasury: Credits,
    active_tickets_by_vessel: HashMap<ShipSymbol, Uuid>,
}

impl InMemoryEventFinanceSystem {
    fn new(initial_treasury: Credits) -> Self {
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

impl EventDrivenFinanceSystem for InMemoryEventFinanceSystem {
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
}

fn main() -> Result<(), Box<dyn Error>> {
    // Create an in-memory finance system with 1,000,000 credits in treasury
    let mut finance = InMemoryEventFinanceSystem::new(Credits::new(1_000_000));

    // Create some fleets
    let trading_fleet_id = FleetId(1);
    let mining_fleet_id = FleetId(2);
    let market_observation_fleet_id = FleetId(3);

    finance.create_fleet_budget(mining_fleet_id.clone(), Credits::new(300_000))?;
    finance.create_fleet_budget(trading_fleet_id.clone(), Credits::new(500_000))?;
    finance.create_fleet_budget(market_observation_fleet_id.clone(), Credits::new(50_000))?;

    println!("Created fleets with initial budgets:");
    println!("  TRADING_FLEET: {}", finance.get_fleet_budget(&trading_fleet_id)?.available_capital);
    println!("  MINING_FLEET: {}", finance.get_fleet_budget(&mining_fleet_id)?.available_capital);
    println!(
        "  MARKET_OBSERVATION_FLEET: {}",
        finance.get_fleet_budget(&market_observation_fleet_id)?.available_capital
    );

    println!("Created fleets with budgets");

    // Create a trading transaction with multiple goals
    let goals = vec![
        // Goal 1: Purchase goods at X1-YZ45
        TransactionGoal::Purchase {
            good: TradeGoodSymbol::PRECIOUS_STONES,
            target_quantity: 100,
            available_quantity: None,
            acquired_quantity: 0,
            estimated_price: Credits::new(2_000),
            max_acceptable_price: Some(Credits::new(2_200)),
            source_waypoint: WaypointSymbol("X1-YZ45".to_string()),
        },
        // Goal 2: Refuel at X1-YZ46 (optional)
        TransactionGoal::Refuel {
            target_fuel_level: 100,
            current_fuel_level: 60,
            estimated_cost_per_unit: Credits::new(250),
            waypoint: WaypointSymbol("X1-YZ46".to_string()),
            is_optional: true,
        },
        // Goal 3: Sell goods at X1-YZ47
        TransactionGoal::Sell {
            good: TradeGoodSymbol::PRECIOUS_STONES,
            target_quantity: 100,
            sold_quantity: 0,
            estimated_price: Credits::new(2_500),
            min_acceptable_price: Some(Credits::new(2_400)),
            destination_waypoint: WaypointSymbol("X1-YZ47".to_string()),
        },
    ];

    // Create a transaction ticket
    let estimated_completion = Utc::now() + Duration::hours(3);
    let ticket_id = finance.create_ticket(
        TicketType::Trading,
        ShipSymbol("SHIP-1".to_string()),
        &trading_fleet_id,
        &trading_fleet_id,
        &trading_fleet_id,
        goals,
        estimated_completion,
        10.0,
    )?;

    println!("\nCreated trading ticket with ID: {}", ticket_id);

    // Fund the ticket
    finance.fund_ticket(
        ticket_id,
        FundingSource {
            source_fleet: trading_fleet_id.clone(),
            amount: Credits::new(215_000), // Purchase cost + refuel cost buffer
        },
    )?;

    println!("Funded the ticket with 215,000 credits from Trading Fleet");

    // Start executing the ticket
    finance.start_ticket_execution(ticket_id)?;
    println!("Started ticket execution");

    // Manually simulate the execution process
    println!("\n--- Manually simulating the transaction execution ---\n");

    // STEP 1: Ship departs from HQ to the purchase location
    let departure_event = TransactionEvent::WaypointDeparted {
        timestamp: Utc::now(),
        waypoint: WaypointSymbol("HQ".to_string()),
        destination: WaypointSymbol("X1-YZ45".to_string()),
        estimated_arrival: Utc::now() + Duration::minutes(5),
    };
    finance.record_event(ticket_id, departure_event)?;
    println!("Ship departed from HQ heading to X1-YZ45");

    // STEP 2: Ship arrives at purchase location
    let arrival_event = TransactionEvent::WaypointArrived {
        timestamp: Utc::now(),
        waypoint: WaypointSymbol("X1-YZ45".to_string()),
        fuel_level: Some(80),
        cargo_used: Some(0),
        cargo_capacity: Some(100),
    };
    finance.record_event(ticket_id, arrival_event)?;
    println!("Ship arrived at X1-YZ45");

    // STEP 3: Ship observes the market
    let mut market_prices = HashMap::new();
    market_prices.insert(TradeGoodSymbol::PRECIOUS_STONES, Credits::new(2100));

    let mut market_supply = HashMap::new();
    market_supply.insert(TradeGoodSymbol::PRECIOUS_STONES, Credits::new(200));

    // STEP 4: Ship purchases goods
    // The price is acceptable (2100 <= 2200 max acceptable price)
    let purchase_event = TransactionEvent::GoodsPurchased {
        timestamp: Utc::now(),
        waypoint: WaypointSymbol("X1-YZ45".to_string()),
        good: TradeGoodSymbol::PRECIOUS_STONES,
        quantity: 100,
        price_per_unit: Credits::new(2100),
        total_cost: Credits::new(210_000),
    };
    finance.record_event(ticket_id, purchase_event)?;
    println!("Purchased 100 units of PRECIOUS_METALS for 210,000 credits");

    // Display current ticket state
    let current_ticket = finance.get_ticket(ticket_id)?;
    println!("\nCurrent ticket state after purchase:");
    println!("  Spent capital: {}", current_ticket.financials.spent_capital);
    println!("  Purchase goal completed: {}", current_ticket.goals[0].is_completed());

    // STEP 5: Ship departs from purchase location to refuel location
    let departure_event = TransactionEvent::WaypointDeparted {
        timestamp: Utc::now(),
        waypoint: WaypointSymbol("X1-YZ45".to_string()),
        destination: WaypointSymbol("X1-YZ46".to_string()),
        estimated_arrival: Utc::now() + Duration::minutes(3),
    };
    finance.record_event(ticket_id, departure_event)?;
    println!("\nShip departed from X1-YZ45 heading to X1-YZ46 (refueling station)");

    // STEP 6: Ship arrives at refuel location
    let arrival_event = TransactionEvent::WaypointArrived {
        timestamp: Utc::now(),
        waypoint: WaypointSymbol("X1-YZ46".to_string()),
        fuel_level: Some(65), // Reduced from travel
        cargo_used: Some(100),
        cargo_capacity: Some(100),
    };
    finance.record_event(ticket_id, arrival_event)?;
    println!("Ship arrived at X1-YZ46 with fuel level at 65");

    // STEP 7: Decision about refueling
    // The ship has over 50% fuel and refueling is optional, so we decide to skip
    finance.skip_goal(ticket_id, 1, "Sufficient fuel available for the journey".to_string())?;
    println!("Decided to skip refueling as current level (65) is sufficient");

    // STEP 8: Ship departs from refuel location to sell location
    let departure_event = TransactionEvent::WaypointDeparted {
        timestamp: Utc::now(),
        waypoint: WaypointSymbol("X1-YZ46".to_string()),
        destination: WaypointSymbol("X1-YZ47".to_string()),
        estimated_arrival: Utc::now() + Duration::minutes(4),
    };
    finance.record_event(ticket_id, departure_event)?;
    println!("\nShip departed from X1-YZ46 heading to X1-YZ47 (market)");

    // STEP 9: Ship arrives at sell location
    let arrival_event = TransactionEvent::WaypointArrived {
        timestamp: Utc::now(),
        waypoint: WaypointSymbol("X1-YZ47".to_string()),
        fuel_level: Some(50), // Further reduced
        cargo_used: Some(100),
        cargo_capacity: Some(100),
    };
    finance.record_event(ticket_id, arrival_event)?;
    println!("Ship arrived at X1-YZ47");

    // STEP 10: Ship observes the sell market
    let mut sell_prices = HashMap::new();
    sell_prices.insert(TradeGoodSymbol::PRECIOUS_STONES, 2700); // Good price!

    // STEP 11: Ship sells the goods
    // The price is very good (2700 > 2400 min acceptable price)
    let sell_event = TransactionEvent::GoodsSold {
        timestamp: Utc::now(),
        waypoint: WaypointSymbol("X1-YZ47".to_string()),
        good: TradeGoodSymbol::PRECIOUS_STONES,
        quantity: 100,
        price_per_unit: Credits::new(2700),
        total_revenue: Credits::new(270_000),
    };
    finance.record_event(ticket_id, sell_event)?;
    println!("Sold 100 units of PRECIOUS_METALS for 270,000 credits");

    // STEP 12: All goals are now complete, so the ticket should be completed
    // The system should auto-detect this, but we'll explicitly call complete
    finance.complete_ticket(ticket_id)?;
    println!("\nAll goals completed, finalizing transaction");

    // Get the final ticket state
    let final_ticket = finance.get_ticket(ticket_id)?;

    // Show final results
    println!("\nFinal Transaction Summary:");
    println!("  Status: {:?}", final_ticket.status);
    println!("  Total spent: {}", final_ticket.financials.spent_capital);
    println!("  Total earned: {}", final_ticket.financials.earned_revenue);
    println!("  Net profit: {}", final_ticket.financials.current_profit);
    println!("  Operating expenses: {}", final_ticket.financials.operating_expenses);

    println!("{}", final_ticket.generate_event_history());

    // Check updated fleet budget
    let updated_budget = finance.get_fleet_budget(&trading_fleet_id)?;
    println!("\nUpdated Trading Fleet Budget:");
    println!("  Available capital: {}", updated_budget.available_capital);

    println!(
        "  Market Observation Fleet: {}",
        finance.get_fleet_budget(&market_observation_fleet_id)?.available_capital
    );

    // Create a ship purchase ticket
    // Trading fleet will buy a ship for the market observation fleet
    let ship_purchase_goal = TransactionGoal::ShipPurchase {
        ship_type: ShipType::SHIP_LIGHT_HAULER,
        estimated_cost: Credits::new(25_000),
        beneficiary_fleet: market_observation_fleet_id.clone(),
        shipyard_waypoint: WaypointSymbol("X1-SHIPYARD".to_string()),
        has_been_purchased: false,
    };

    let ticket_id = finance.create_ticket(
        TicketType::FleetExpansion,
        ShipSymbol("TRADING-SHIP-1".to_string()), // Ship executing the purchase
        &trading_fleet_id,                        // Fleet owning the ship
        &trading_fleet_id,                        // Fleet initiating the purchase
        &market_observation_fleet_id,             // Fleet benefiting from the purchase
        vec![ship_purchase_goal],
        Utc::now() + Duration::hours(1),
        5.0,
    )?;

    println!("\nCreated ship purchase ticket with ID: {}", ticket_id);

    // Fund the ticket from trading fleet
    finance.fund_ticket(
        ticket_id,
        FundingSource {
            source_fleet: market_observation_fleet_id.clone(),
            amount: Credits::new(25_000),
        },
    )?;

    println!("Funded the ticket with 25,000 credits from Trading Fleet");

    // Start executing the ticket
    finance.start_ticket_execution(ticket_id)?;
    println!("Started ticket execution");

    // --- Manually simulate the execution ---
    println!("\n--- Simulating the ship purchase ---\n");

    // Ship departs from current location to shipyard
    let departure_event = TransactionEvent::WaypointDeparted {
        timestamp: Utc::now(),
        waypoint: WaypointSymbol("TRADING-HQ".to_string()),
        destination: WaypointSymbol("X1-SHIPYARD".to_string()),
        estimated_arrival: Utc::now() + Duration::minutes(10),
    };
    finance.record_event(ticket_id, departure_event)?;
    println!("Trading ship departed from TRADING-HQ heading to X1-SHIPYARD");

    // Ship arrives at shipyard
    let arrival_event = TransactionEvent::WaypointArrived {
        timestamp: Utc::now(),
        waypoint: WaypointSymbol("X1-SHIPYARD".to_string()),
        fuel_level: Some(70),
        cargo_used: Some(0),
        cargo_capacity: Some(100),
    };
    finance.record_event(ticket_id, arrival_event)?;
    println!("Trading ship arrived at X1-SHIPYARD");

    // Purchase the new ship
    let purchase_event = TransactionEvent::ShipPurchased {
        timestamp: Utc::now(),
        waypoint: WaypointSymbol("X1-SHIPYARD".to_string()),
        ship_type: ShipType::SHIP_LIGHT_HAULER,
        ship_id: ShipSymbol("OBSERVER-SHIP-1".to_string()),
        total_cost: Credits::new(25_000),
        beneficiary_fleet: market_observation_fleet_id.clone(),
    };
    finance.record_event(ticket_id, purchase_event)?;
    println!("Purchased LIGHT_HAULER ship for 25,000 credits (ID: OBSERVER-SHIP-1)");

    // Transfer the ship to the observation fleet
    let transfer_event = TransactionEvent::ShipTransferred {
        timestamp: Utc::now(),
        ship_id: ShipSymbol("OBSERVER-SHIP-1".to_string()),
        from_fleet: trading_fleet_id.clone(),
        to_fleet: market_observation_fleet_id.clone(),
    };
    finance.record_event(ticket_id, transfer_event)?;
    println!("Transferred ship OBSERVER-SHIP-1 to MARKET_OBSERVATION_FLEET");

    // Complete the ticket
    finance.complete_ticket(ticket_id)?;
    println!("\nCompleted ship purchase transaction");

    let final_ticket = finance.get_ticket(ticket_id)?;
    println!("{}", final_ticket.generate_event_history());

    // Check final fleet budgets
    let trading_budget = finance.get_fleet_budget(&trading_fleet_id)?;
    let observer_budget = finance.get_fleet_budget(&market_observation_fleet_id)?;

    println!("\nFinal fleet budgets:");
    println!("  Trading Fleet: {}", trading_budget.available_capital);
    println!("  Market Observation Fleet: {}", observer_budget.available_capital);
    println!("  Market Observation Fleet assets: {}", observer_budget.asset_value);

    Ok(())
}
