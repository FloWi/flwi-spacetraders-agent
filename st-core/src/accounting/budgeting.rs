use crate::accounting::credits::Credits;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use st_domain::{FleetId, ShipSymbol, ShipType, TradeGoodSymbol, WaypointSymbol};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransactionGoal {
    Purchase {
        good: TradeGoodSymbol,
        target_quantity: u32,
        available_quantity: Option<u32>,
        acquired_quantity: u32,
        estimated_price: Credits,
        max_acceptable_price: Option<Credits>,
        source_waypoint: WaypointSymbol,
    },

    Sell {
        good: TradeGoodSymbol,
        target_quantity: u32,
        sold_quantity: u32,
        estimated_price: Credits,
        min_acceptable_price: Option<Credits>,
        destination_waypoint: WaypointSymbol,
    },
    Refuel {
        target_fuel_level: u32,
        current_fuel_level: u32,
        estimated_cost_per_unit: Credits,
        waypoint: WaypointSymbol,
        is_optional: bool,
    },
    ShipPurchase {
        ship_type: ShipType,
        estimated_cost: Credits,
        has_been_purchased: bool,
        beneficiary_fleet: FleetId,
        shipyard_waypoint: WaypointSymbol,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub enum TicketType {
    Trading,
    FleetExpansion,
    Construction,
    Exploration,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub enum TicketStatus {
    Planned,
    Funded,
    InProgress,
    Completed,
    Failed { reason: String },
    Cancelled { reason: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FundingSource {
    pub source_fleet: FleetId,
    pub amount: Credits,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TicketFinancials {
    pub required_capital: Credits,
    pub allocated_capital: Credits,
    pub funding_sources: Vec<FundingSource>,
    pub spent_capital: Credits,
    pub earned_revenue: Credits,
    pub current_profit: Credits,
    pub projected_profit: Credits,
    pub operating_expenses: Credits,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TransactionEvent {
    TicketCreated {
        timestamp: DateTime<Utc>,
    },

    TicketFunded {
        timestamp: DateTime<Utc>,
        source: FundingSource,
    },

    ExecutionStarted {
        timestamp: DateTime<Utc>,
    },

    GoodsPurchased {
        timestamp: DateTime<Utc>,
        waypoint: WaypointSymbol,
        good: TradeGoodSymbol,
        quantity: u32,
        price_per_unit: Credits,
        total_cost: Credits,
    },

    GoodsSold {
        timestamp: DateTime<Utc>,
        waypoint: WaypointSymbol,
        good: TradeGoodSymbol,
        quantity: u32,
        price_per_unit: Credits,
        total_revenue: Credits,
    },

    ShipRefueled {
        timestamp: DateTime<Utc>,
        waypoint: WaypointSymbol,
        fuel_added: u32,
        cost_per_unit: Credits,
        total_cost: Credits,
        new_fuel_level: u32,
    },

    GoalSkipped {
        timestamp: DateTime<Utc>,
        goal_index: usize,
        reason: String,
    },

    TicketCompleted {
        timestamp: DateTime<Utc>,
        final_profit: Credits,
    },

    TicketFailed {
        timestamp: DateTime<Utc>,
        reason: String,
    },

    FundsReturned {
        timestamp: DateTime<Utc>,
        unspent_funds_returned: Credits,
        revenue_returned: Credits,
        net_profit: Credits,
    },
    ShipPurchased {
        timestamp: DateTime<Utc>,
        waypoint: WaypointSymbol,
        ship_type: ShipType,
        ship_id: ShipSymbol,
        total_cost: Credits,
        beneficiary_fleet: FleetId,
    },
    ShipTransferred {
        timestamp: DateTime<Utc>,
        ship_id: ShipSymbol,
        from_fleet: FleetId,
        to_fleet: FleetId,
    },
    AssetTransferred {
        timestamp: DateTime<Utc>,
        asset_type: String,
        asset_value: Credits,
        to_fleet: FleetId,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransactionTicket {
    pub id: Uuid,
    pub ticket_type: TicketType,
    pub status: TicketStatus,
    pub executing_vessel: ShipSymbol,
    pub executing_fleet: FleetId,
    pub initiating_fleet: FleetId,
    pub beneficiary_fleet: FleetId,
    pub goals: Vec<TransactionGoal>,
    pub financials: TicketFinancials,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub estimated_completion: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub priority: f64,
    pub event_history: Vec<TransactionEvent>,
    pub metadata: HashMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub enum FundingPurpose {
    Trading,
    FleetExpansion,
    Construction,
    Exploration,
    Contingency,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FleetBudget {
    pub fleet_id: FleetId,
    pub total_capital: Credits,
    pub available_capital: Credits,
    pub operating_reserve: Credits,
    pub earmarked_funds: HashMap<FundingPurpose, Credits>,
    pub asset_value: Credits,
    pub funded_transactions: HashSet<Uuid>,
    pub beneficiary_transactions: HashSet<Uuid>,
    pub executing_transactions: HashSet<Uuid>,
}

#[derive(Debug)]
pub enum FinanceError {
    InsufficientFunds,
    TicketNotFound,
    FleetNotFound,
    FleetAlreadyBudgeted,
    InvalidOperation,
    InvalidState,
    GoalNotFound,
}

impl fmt::Display for FinanceError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InsufficientFunds => write!(f, "Insufficient funds for operation"),
            Self::TicketNotFound => write!(f, "Transaction ticket not found"),
            Self::FleetNotFound => write!(f, "Fleet not found"),
            Self::InvalidOperation => write!(f, "Invalid operation"),
            Self::InvalidState => write!(f, "Invalid state for operation"),
            Self::GoalNotFound => write!(f, "Goal not found"),
            Self::FleetAlreadyBudgeted => write!(f, "Fleet already budgeted"),
        }
    }
}

impl Error for FinanceError {}

impl TransactionTicket {
    pub fn all_required_goals_completed(&self) -> bool {
        self.goals.iter().all(|goal| goal.is_completed() || goal.is_optional())
    }

    pub fn update_from_event(&mut self, event: &TransactionEvent) {
        // Update financials based on events
        match event {
            TransactionEvent::GoodsPurchased { total_cost, .. } => {
                self.financials.spent_capital += *total_cost;
                self.financials.current_profit = self.financials.earned_revenue - self.financials.spent_capital;
            }
            TransactionEvent::GoodsSold { total_revenue, .. } => {
                self.financials.earned_revenue += *total_revenue;
                self.financials.current_profit = self.financials.earned_revenue - self.financials.spent_capital;
            }
            TransactionEvent::ShipRefueled { total_cost, .. } => {
                self.financials.spent_capital += *total_cost;
                self.financials.operating_expenses += *total_cost;
                self.financials.current_profit = self.financials.earned_revenue - self.financials.spent_capital;
            }

            TransactionEvent::TicketCreated { .. } => {}
            TransactionEvent::TicketFunded { .. } => {}
            TransactionEvent::ExecutionStarted { .. } => {}
            TransactionEvent::GoalSkipped { .. } => {}
            TransactionEvent::TicketCompleted { .. } => {}
            TransactionEvent::TicketFailed { .. } => {}
            TransactionEvent::FundsReturned { .. } => {}
            TransactionEvent::ShipPurchased {
                timestamp,
                waypoint,
                ship_type,
                ship_id,
                total_cost,
                beneficiary_fleet,
            } => {
                self.financials.spent_capital += *total_cost;
                self.financials.operating_expenses += *total_cost;
                self.financials.current_profit = self.financials.earned_revenue - self.financials.spent_capital;
            }
            TransactionEvent::ShipTransferred { .. } => {}
            TransactionEvent::AssetTransferred { .. } => {}
        }

        // Update goal progress based on events
        for goal in &mut self.goals {
            goal.update_progress(event);
        }

        // Add event to history
        self.event_history.push(event.clone());
        self.updated_at = Utc::now();
    }

    pub fn generate_event_history(&self) -> String {
        // Show event history

        use std::fmt::Write;

        let mut result = String::new();

        writeln!(&mut result, "\nEvent History:").unwrap();
        for (i, event) in self.event_history.iter().enumerate() {
            match event {
                TransactionEvent::TicketCreated { timestamp } => {
                    writeln!(&mut result, "  {}. Ticket created at {}", i + 1, timestamp).unwrap();
                }
                TransactionEvent::TicketFunded { timestamp, source } => {
                    writeln!(
                        &mut result,
                        "  {}. Ticket funded with {} credits from {:?} at {}",
                        i + 1,
                        source.amount,
                        source.source_fleet,
                        timestamp
                    )
                    .unwrap();
                }
                TransactionEvent::ExecutionStarted { timestamp } => {
                    writeln!(&mut result, "  {}. Execution started at {}", i + 1, timestamp).unwrap();
                }
                TransactionEvent::GoodsPurchased {
                    timestamp,
                    waypoint,
                    good,
                    quantity,
                    price_per_unit,
                    total_cost,
                } => {
                    writeln!(
                        &mut result,
                        "  {}. Purchased {} units of {} at {} credits each (total: {}) at {} at {}",
                        i + 1,
                        quantity,
                        good,
                        price_per_unit,
                        total_cost,
                        waypoint,
                        timestamp
                    )
                    .unwrap();
                }
                TransactionEvent::GoodsSold {
                    timestamp,
                    waypoint,
                    good,
                    quantity,
                    price_per_unit,
                    total_revenue,
                } => {
                    writeln!(
                        &mut result,
                        "  {}. Sold {} units of {} at {} credits each (total: {}) at {} at {}",
                        i + 1,
                        quantity,
                        good,
                        price_per_unit,
                        total_revenue,
                        waypoint,
                        timestamp
                    )
                    .unwrap();
                }
                TransactionEvent::ShipRefueled {
                    timestamp,
                    waypoint,
                    fuel_added,
                    total_cost,
                    ..
                } => {
                    writeln!(
                        &mut result,
                        "  {}. Refueled {} units (cost: {}) at {} at {}",
                        i + 1,
                        fuel_added,
                        total_cost,
                        waypoint,
                        timestamp
                    )
                    .unwrap();
                }
                TransactionEvent::GoalSkipped { timestamp, goal_index, reason } => {
                    writeln!(&mut result, "  {}. Skipped goal {} (reason: {}) at {}", i + 1, goal_index, reason, timestamp).unwrap();
                }
                TransactionEvent::TicketCompleted { timestamp, final_profit } => {
                    writeln!(&mut result, "  {}. Ticket completed with profit of {} at {}", i + 1, final_profit, timestamp).unwrap();
                }
                TransactionEvent::TicketFailed { timestamp, reason } => {
                    eprintln!("  {}. Ticket failed at {}. Reason: {}", i + 1, timestamp, reason);
                }
                TransactionEvent::FundsReturned {
                    timestamp,
                    unspent_funds_returned,
                    revenue_returned,
                    net_profit,
                } => {
                    writeln!(
                        &mut result,
                        "  {}. Funds returned to fleet at {}. Unspent funds: {}; revenue returned: {}; net_profit: {}",
                        i + 1,
                        timestamp,
                        unspent_funds_returned,
                        revenue_returned,
                        net_profit
                    )
                    .unwrap();
                }
                TransactionEvent::ShipPurchased {
                    timestamp,
                    ship_type,
                    ship_id,
                    total_cost,
                    beneficiary_fleet,
                    ..
                } => {
                    writeln!(
                        &mut result,
                        "  {}. Purchased {} ship (ID: {}) for {} credits for fleet {} at {}",
                        i + 1,
                        ship_type,
                        ship_id,
                        total_cost,
                        beneficiary_fleet,
                        timestamp
                    )
                    .unwrap();
                }
                TransactionEvent::ShipTransferred {
                    timestamp,
                    ship_id,
                    from_fleet,
                    to_fleet,
                } => {
                    writeln!(
                        &mut result,
                        "  {}. Transferred ship {} from fleet #{} to fleet #{} at {}",
                        i + 1,
                        ship_id,
                        from_fleet,
                        to_fleet,
                        timestamp
                    )
                    .unwrap();
                }
                TransactionEvent::AssetTransferred {
                    timestamp,
                    asset_type,
                    asset_value,
                    to_fleet,
                } => {
                    writeln!(
                        &mut result,
                        "  {}. Asset transferred. asset_type {} asset_value {} to fleet #{}",
                        i + 1,
                        asset_type,
                        asset_value,
                        to_fleet
                    )
                    .unwrap();
                }
            }
        }
        result
    }
}

impl TransactionGoal {
    pub fn is_completed(&self) -> bool {
        match self {
            Self::Purchase {
                target_quantity,
                acquired_quantity,
                ..
            } => *acquired_quantity >= *target_quantity,

            Self::Sell {
                target_quantity,
                sold_quantity,
                ..
            } => *sold_quantity >= *target_quantity,

            Self::Refuel {
                target_fuel_level,
                current_fuel_level,
                ..
            } => *current_fuel_level >= *target_fuel_level,
            TransactionGoal::ShipPurchase {
                ship_type,
                estimated_cost,
                has_been_purchased,
                beneficiary_fleet,
                shipyard_waypoint: waypoint,
            } => *has_been_purchased,
        }
    }

    pub fn is_optional(&self) -> bool {
        match self {
            Self::Refuel { is_optional, .. } => *is_optional,
            _ => false,
        }
    }

    pub fn get_waypoint(&self) -> &WaypointSymbol {
        match self {
            Self::Purchase { source_waypoint, .. } => source_waypoint,
            Self::Sell { destination_waypoint, .. } => destination_waypoint,
            Self::Refuel { waypoint, .. } => waypoint,
            TransactionGoal::ShipPurchase {
                shipyard_waypoint: waypoint, ..
            } => waypoint,
        }
    }

    pub fn update_progress(&mut self, event: &TransactionEvent) -> bool {
        match (self, event) {
            // Purchase goal progress update
            (
                Self::Purchase {
                    good: goal_good,
                    acquired_quantity,
                    ..
                },
                TransactionEvent::GoodsPurchased { good, quantity, .. },
            ) if goal_good == good => {
                *acquired_quantity += quantity;
                true
            }

            // Sell goal progress update
            (
                Self::Sell {
                    good: goal_good,
                    sold_quantity,
                    ..
                },
                TransactionEvent::GoodsSold { good, quantity, .. },
            ) if goal_good == good => {
                *sold_quantity += quantity;
                true
            }

            // Refuel goal progress update
            (Self::Refuel { current_fuel_level, .. }, TransactionEvent::ShipRefueled { new_fuel_level, .. }) => {
                *current_fuel_level = *new_fuel_level;
                true
            }

            // Ship purchase goal progress update
            (
                Self::ShipPurchase {
                    ship_type: goal_ship_type,
                    beneficiary_fleet: goal_fleet,
                    has_been_purchased,
                    ..
                },
                TransactionEvent::ShipPurchased {
                    ship_type, beneficiary_fleet, ..
                },
            ) if goal_ship_type == ship_type && goal_fleet == beneficiary_fleet => {
                *has_been_purchased = true;
                true
            }

            _ => false,
        }
    }
}
