use crate::accounting::treasury_redesign::LedgerEntry::*;
use anyhow::anyhow;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use st_domain::budgeting::credits::Credits;
use st_domain::{FleetId, ShipType, TicketId, TradeGoodSymbol, WaypointSymbol};
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FinanceTicket {
    ticket_id: TicketId,
    details: FinanceTicketDetails,
    allocated_credits: Credits,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum FinanceTicketDetails {
    PurchaseTradeGoods {
        waypoint_symbol: WaypointSymbol,
        trade_good: TradeGoodSymbol,
        expected_price_per_unit: Credits,
        quantity: u32,
        expected_total_purchase_price: Credits,
    },
    SellTradeGoods {
        waypoint_symbol: WaypointSymbol,
        trade_good: TradeGoodSymbol,
        expected_price_per_unit: Credits,
        quantity: u32,
        expected_total_sell_price: Credits,
    },
    PurchaseShip {
        ship_type: ShipType,
        expected_purchase_price: Credits,
    },
    RefuelShip {
        expected_price_per_unit: Credits,
        num_fuel_barrels: u32,
        expected_total_purchase_price: Credits,
    },
}

impl FinanceTicketDetails {
    pub fn signum(&self) -> i64 {
        match self {
            FinanceTicketDetails::PurchaseTradeGoods { .. } => -1,
            FinanceTicketDetails::SellTradeGoods { .. } => 1,
            FinanceTicketDetails::PurchaseShip { .. } => -1,
            FinanceTicketDetails::RefuelShip { .. } => -1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum LedgerEntry {
    TreasuryCreated {
        credits: Credits,
    },
    FleetCreated {
        fleet_id: FleetId,
        total_capital: Credits,
    },
    TicketCreated {
        fleet_id: FleetId,
        ticket_details: FinanceTicket,
    },
    TicketCompleted {
        fleet_id: FleetId,
        finance_ticket: FinanceTicket,
        actual_units: u32,
        actual_price_per_unit: Credits,
        total: Credits,
    },
    ExpenseLogged {
        fleet_id: FleetId,
        maybe_ticket_id: Option<TicketId>,
    },
    TransferFundsFromFleetToTreasury {
        fleet_id: FleetId,
        credits: Credits,
    },
    TransferFundsTreasuryToFleet {
        fleet_id: FleetId,
        credits: Credits,
    },
}

#[derive(PartialEq, Debug, Default, Clone, Serialize, Deserialize)]
pub struct FleetBudget2 {
    /// the cash we have at hand - "real money (single source of truth)"
    current_capital: Credits,
    /// funds reserved for tickets (not spent yet) - "virtual money"
    reserved_capital: Credits,
    /// this is the amount of money we need for operating. If we have more at one point, it will be transferred back to the treasury.
    /// "virtual money"
    total_budget: Credits,
    /// the amount of money we'd like to keep - "virtual money"
    expense_reserve: Credits,
}

use clap::parser::ValueSource::DefaultValue;
use std::sync::{Arc, Mutex};

pub struct ThreadSafeTreasurer {
    inner: Arc<Mutex<ImprovedTreasurer>>,
}

impl ThreadSafeTreasurer {
    pub fn new(starting_credits: Credits) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ImprovedTreasurer::new(starting_credits))),
        }
    }

    // Helper method to execute operations on the treasurer
    fn with_treasurer<F, R>(&self, operation: F) -> Result<R>
    where
        F: FnOnce(&mut ImprovedTreasurer) -> Result<R>,
    {
        let mut treasurer = self.inner.lock().map_err(|_| anyhow!("Mutex poisoned"))?;
        operation(&mut treasurer)
    }

    pub fn get_instance(&self) -> Result<ImprovedTreasurer> {
        self.with_treasurer(|t| Ok(t.clone()))
    }

    pub fn get_fleet_budget(&self, fleet_id: &FleetId) -> Result<Option<FleetBudget2>> {
        self.with_treasurer(|t| Ok(t.fleet_budgets.get(fleet_id).cloned()))
    }

    pub fn current_agent_credits(&self) -> Result<Credits> {
        self.with_treasurer(|t| Ok(t.current_agent_credits()))
    }

    pub fn create_fleet(&self, fleet_id: &FleetId, total_capital: Credits) -> Result<()> {
        self.with_treasurer(|t| t.create_fleet(fleet_id, total_capital))
    }

    pub fn transfer_funds_to_fleet_to_top_up_available_capital(&self, fleet_id: &FleetId) -> Result<()> {
        self.with_treasurer(|t| t.transfer_funds_to_fleet_to_top_up_available_capital(fleet_id))
    }

    pub fn create_purchase_trade_goods_ticket(
        &self,
        fleet_id: &FleetId,
        trade_good_symbol: TradeGoodSymbol,
        waypoint_symbol: WaypointSymbol,
        quantity: u32,
        expected_price_per_unit: Credits,
    ) -> Result<FinanceTicket> {
        self.with_treasurer(|t| t.create_purchase_trade_goods_ticket(fleet_id, trade_good_symbol, waypoint_symbol, quantity, expected_price_per_unit))
    }

    pub fn create_sell_trade_goods_ticket(
        &self,
        fleet_id: &FleetId,
        trade_good_symbol: TradeGoodSymbol,
        waypoint_symbol: WaypointSymbol,
        quantity: u32,
        expected_price_per_unit: Credits,
    ) -> Result<FinanceTicket> {
        self.with_treasurer(|t| t.create_sell_trade_goods_ticket(fleet_id, trade_good_symbol, waypoint_symbol, quantity, expected_price_per_unit))
    }

    pub fn complete_ticket(&self, fleet_id: &FleetId, finance_ticket: &FinanceTicket, actual_price_per_unit: Credits) -> Result<()> {
        self.with_treasurer(|t| t.complete_ticket(fleet_id, finance_ticket, actual_price_per_unit))
    }

    pub fn transfer_excess_funds_from_fleet_to_treasury(&self, fleet_id: &FleetId) -> Result<()> {
        self.with_treasurer(|t| t.transfer_excess_funds_from_fleet_to_treasury(fleet_id))
    }

    pub fn ledger_entries(&self) -> Result<VecDeque<LedgerEntry>> {
        self.with_treasurer(|t| Ok(t.ledger_entries.clone()))
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ImprovedTreasurer {
    treasury_fund: Credits,
    ledger_entries: VecDeque<LedgerEntry>,
    fleet_budgets: HashMap<FleetId, FleetBudget2>,
}

impl ImprovedTreasurer {
    pub fn new(starting_credits: Credits) -> Self {
        let mut treasurer = Self {
            treasury_fund: Default::default(),
            ledger_entries: Default::default(),
            fleet_budgets: Default::default(),
        };

        treasurer
            .process_ledger_entry(TreasuryCreated { credits: starting_credits })
            .unwrap();
        treasurer
    }

    pub(crate) fn current_agent_credits(&self) -> Credits {
        self.treasury_fund
            + self
                .fleet_budgets
                .values()
                .map(|budget| budget.current_capital.0)
                .sum::<i64>()
    }

    fn create_fleet(&mut self, fleet_id: &FleetId, total_capital: Credits) -> Result<()> {
        self.process_ledger_entry(FleetCreated {
            fleet_id: fleet_id.clone(),
            total_capital,
        })?;

        Ok(())
    }

    fn transfer_funds_to_fleet_to_top_up_available_capital(&mut self, fleet_id: &FleetId) -> Result<()> {
        if let Some(fleet_budget) = self.fleet_budgets.get(fleet_id) {
            let diff = fleet_budget.total_budget - fleet_budget.current_capital;
            if diff.is_positive() {
                let transfer_sum = self.treasury_fund.min(diff);
                self.process_ledger_entry(TransferFundsTreasuryToFleet {
                    fleet_id: fleet_id.clone(),
                    credits: transfer_sum,
                })
            } else {
                Ok(())
            }
        } else {
            Err(anyhow!("Fleet not found {}", fleet_id))
        }
    }

    pub fn create_purchase_trade_goods_ticket(
        &mut self,
        fleet_id: &FleetId,
        trade_good_symbol: TradeGoodSymbol,
        waypoint_symbol: WaypointSymbol,
        quantity: u32,
        expected_price_per_unit: Credits,
    ) -> Result<FinanceTicket> {
        if let Some(fleet_budget) = self.fleet_budgets.get(fleet_id) {
            let fleet_reserve: Credits = 10_000.into();
            let affordable_units: u32 = ((fleet_budget.current_capital - fleet_reserve).0 / expected_price_per_unit.0) as u32;
            let quantity = affordable_units.min(quantity);
            let total = (expected_price_per_unit.0 * quantity as i64).into();
            let ticket = FinanceTicket {
                ticket_id: Default::default(),
                details: FinanceTicketDetails::PurchaseTradeGoods {
                    waypoint_symbol,
                    trade_good: trade_good_symbol,
                    expected_total_purchase_price: total,
                    quantity: quantity as u32,
                    expected_price_per_unit,
                },
                allocated_credits: total,
            };
            self.process_ledger_entry(TicketCreated {
                fleet_id: fleet_id.clone(),
                ticket_details: ticket.clone(),
            })?;
            Ok(ticket)
        } else {
            Err(anyhow!("Fleet not found {}", fleet_id))
        }
    }

    pub fn create_sell_trade_goods_ticket(
        &mut self,
        fleet_id: &FleetId,
        trade_good_symbol: TradeGoodSymbol,
        waypoint_symbol: WaypointSymbol,
        quantity: u32,
        expected_price_per_unit: Credits,
    ) -> Result<FinanceTicket> {
        let ticket = FinanceTicket {
            ticket_id: Default::default(),
            details: FinanceTicketDetails::SellTradeGoods {
                waypoint_symbol,
                trade_good: trade_good_symbol,
                expected_total_sell_price: expected_price_per_unit * quantity,
                quantity,
                expected_price_per_unit,
            },
            allocated_credits: 0.into(),
        };

        self.process_ledger_entry(TicketCreated {
            fleet_id: fleet_id.clone(),
            ticket_details: ticket.clone(),
        })?;
        Ok(ticket)
    }

    pub fn complete_ticket(&mut self, fleet_id: &FleetId, finance_ticket: &FinanceTicket, actual_price_per_unit: Credits) -> Result<()> {
        let quantity: u32 = match finance_ticket.details {
            FinanceTicketDetails::PurchaseTradeGoods { quantity, .. } => quantity,
            FinanceTicketDetails::SellTradeGoods { quantity, .. } => quantity,
            FinanceTicketDetails::PurchaseShip { .. } => 1,
            FinanceTicketDetails::RefuelShip { num_fuel_barrels, .. } => num_fuel_barrels,
        };

        self.process_ledger_entry(TicketCompleted {
            fleet_id: fleet_id.clone(),
            finance_ticket: finance_ticket.clone(),
            actual_units: quantity,
            actual_price_per_unit,
            total: actual_price_per_unit * quantity * finance_ticket.details.signum(),
        })?;

        self.transfer_excess_funds_from_fleet_to_treasury(&fleet_id)?;

        Ok(())
    }

    fn transfer_excess_funds_from_fleet_to_treasury(&mut self, fleet_id: &FleetId) -> Result<()> {
        if let Some(budget) = self.fleet_budgets.get_mut(&fleet_id) {
            let excess = budget.current_capital - budget.total_budget;
            if excess.is_positive() {
                self.process_ledger_entry(TransferFundsFromFleetToTreasury {
                    fleet_id: fleet_id.clone(),
                    credits: excess,
                })?;
            }
            Ok(())
        } else {
            Err(anyhow!("Fleet {} doesn't exist", fleet_id))
        }
    }

    /// don't do recursive calls here - we want ot keep replayability
    fn process_ledger_entry(&mut self, ledger_entry: LedgerEntry) -> Result<()> {
        match ledger_entry.clone() {
            TreasuryCreated { credits } => {
                self.treasury_fund = credits;
                self.ledger_entries.push_back(ledger_entry);
            }
            FleetCreated { fleet_id, total_capital } => {
                if self.fleet_budgets.contains_key(&fleet_id) {
                    return Err(anyhow!("Fleet budget {} already exists", fleet_id));
                }

                self.fleet_budgets.insert(
                    fleet_id.clone(),
                    FleetBudget2 {
                        current_capital: 0.into(),
                        reserved_capital: 0.into(),
                        total_budget: total_capital,
                        ..Default::default()
                    },
                );
                self.ledger_entries.push_back(ledger_entry);
            }
            TransferFundsTreasuryToFleet { fleet_id, credits } => {
                if let Some(budget) = self.fleet_budgets.get_mut(&fleet_id) {
                    self.treasury_fund -= credits;
                    budget.current_capital += credits;

                    self.ledger_entries.push_back(ledger_entry);
                } else {
                    return Err(anyhow!("Fleet {} doesn't exist", fleet_id));
                }
            }

            TicketCreated { fleet_id, ticket_details } => {
                let allocated_credits = ticket_details.allocated_credits;
                if let Some(budget) = self.fleet_budgets.get_mut(&fleet_id) {
                    if budget.current_capital >= allocated_credits {
                        budget.reserved_capital += allocated_credits;

                        self.ledger_entries.push_back(ledger_entry);
                    } else {
                        return Err(anyhow!(
                            "Insufficient funds. available_capital: {}; allocated_credits: {allocated_credits}",
                            budget.current_capital
                        ));
                    }
                } else {
                    return Err(anyhow!("Fleet {} doesn't exist", fleet_id));
                }
            }
            TicketCompleted {
                fleet_id,
                finance_ticket,
                total,
                ..
            } => {
                if let Some(budget) = self.fleet_budgets.get_mut(&fleet_id) {
                    if finance_ticket.allocated_credits.is_positive() {
                        // clear the reservation
                        budget.reserved_capital -= finance_ticket.allocated_credits;
                    }

                    budget.current_capital += total;

                    self.ledger_entries.push_back(ledger_entry);
                } else {
                    return Err(anyhow!("Fleet {} doesn't exist", fleet_id));
                }
            }
            TransferFundsFromFleetToTreasury { fleet_id, credits } => {
                if let Some(budget) = self.fleet_budgets.get_mut(&fleet_id) {
                    if budget.current_capital < credits {
                        return Err(anyhow!(
                            "Insufficient funds for transfering funds from fleet {fleet_id} to treasury. available_capital: {}; credits_to_transfer: {credits}",
                            budget.current_capital
                        ));
                    } else {
                        budget.current_capital -= credits;
                        self.treasury_fund += credits;
                        self.ledger_entries.push_back(ledger_entry);
                    }
                } else {
                    return Err(anyhow!("Fleet {} doesn't exist", fleet_id));
                }
            }
            ExpenseLogged { .. } => {}
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::accounting::treasury_redesign::LedgerEntry::TransferFundsFromFleetToTreasury;
    use crate::accounting::treasury_redesign::{FleetBudget2, ImprovedTreasurer, LedgerEntry, ThreadSafeTreasurer};
    use anyhow::Result;
    use st_domain::budgeting::credits::Credits;
    use st_domain::{FleetId, TradeGoodSymbol, WaypointSymbol};

    #[test]
    //#[tokio::test] // for accessing runtime-infos with tokio-conso&le
    fn test_fleet_budget_in_trade_cycle() -> Result<()> {
        //Start Fresh with 175k

        let mut treasurer = ThreadSafeTreasurer::new(175_000.into());
        let mut expected_ledger_entries = vec![LedgerEntry::TreasuryCreated { credits: 175_000.into() }];

        assert_eq!(treasurer.current_agent_credits()?, Credits::new(175_000));
        assert_eq!(
            serde_json::to_string_pretty(&treasurer.ledger_entries()?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );

        // Create fleet with 75k total budget

        treasurer.create_fleet(&FleetId(1), Credits::new(75_000))?;
        expected_ledger_entries.push(LedgerEntry::FleetCreated {
            fleet_id: FleetId(1),
            total_capital: 75_000.into(),
        });

        assert_eq!(
            serde_json::to_string_pretty(&treasurer.ledger_entries()?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );
        assert_eq!(treasurer.current_agent_credits()?, Credits::new(175_000));

        assert_eq!(
            treasurer
                .get_fleet_budget(&FleetId(1))?
                .unwrap()
                .current_capital,
            Credits::new(0)
        );

        // transfer 75k from treasurer to fleet budget

        treasurer.transfer_funds_to_fleet_to_top_up_available_capital(&FleetId(1))?;
        expected_ledger_entries.push(LedgerEntry::TransferFundsTreasuryToFleet {
            fleet_id: FleetId(1),
            credits: 75_000.into(),
        });

        assert_eq!(
            serde_json::to_string_pretty(&treasurer.ledger_entries()?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );
        assert_eq!(treasurer.current_agent_credits()?, Credits::new(175_000));
        assert_eq!(
            treasurer
                .get_fleet_budget(&FleetId(1))?
                .unwrap()
                .current_capital,
            Credits::new(75_000)
        );

        // create purchase ticket (reduces available capital of fleet)

        let purchase_ticket = treasurer.create_purchase_trade_goods_ticket(
            &FleetId(1),
            TradeGoodSymbol::ADVANCED_CIRCUITRY,
            WaypointSymbol("FROM".to_string()),
            40,
            Credits(1_000.into()),
        )?;

        assert_eq!(purchase_ticket.allocated_credits, 40_000.into());

        expected_ledger_entries.push(LedgerEntry::TicketCreated {
            fleet_id: FleetId(1),
            ticket_details: purchase_ticket.clone(),
        });

        assert_eq!(
            treasurer.get_fleet_budget(&FleetId(1))?.unwrap(),
            FleetBudget2 {
                current_capital: 75_000.into(),
                reserved_capital: 40_000.into(),
                total_budget: 75_000.into(),
                ..Default::default()
            }
        );
        assert_eq!(
            serde_json::to_string_pretty(&treasurer.ledger_entries()?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );
        assert_eq!(treasurer.current_agent_credits()?, Credits::new(175_000));

        // create sell ticket

        let sell_ticket = treasurer.create_sell_trade_goods_ticket(
            &FleetId(1),
            TradeGoodSymbol::ADVANCED_CIRCUITRY,
            WaypointSymbol("TO".to_string()),
            40,
            Credits(2_000.into()),
        )?;

        assert_eq!(sell_ticket.allocated_credits, 0.into());

        expected_ledger_entries.push(LedgerEntry::TicketCreated {
            fleet_id: FleetId(1),
            ticket_details: sell_ticket.clone(),
        });

        assert_eq!(
            treasurer.get_fleet_budget(&FleetId(1))?.unwrap(),
            FleetBudget2 {
                current_capital: 75_000.into(),
                reserved_capital: 40_000.into(),
                total_budget: 75_000.into(),
                ..Default::default()
            }
        );

        assert_eq!(
            serde_json::to_string_pretty(&treasurer.ledger_entries()?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );
        assert_eq!(treasurer.current_agent_credits()?, Credits::new(175_000));

        // perform purchase (we spent less than expected)
        let purchase_price_per_unit = 900.into(); // a little less than expected
        treasurer.complete_ticket(&FleetId(1), &purchase_ticket, purchase_price_per_unit)?;

        // state before purchase
        // available_capital: 75_000
        //  reserved_capital: 40_000 (for the expected purchase)

        // actual purchase sum is 36_000
        // 175_000 - 36_000 = 139_000

        expected_ledger_entries.push(LedgerEntry::TicketCompleted {
            fleet_id: FleetId(1),
            finance_ticket: purchase_ticket.clone(),
            actual_units: 40,
            actual_price_per_unit: purchase_price_per_unit,
            total: (-36_000).into(),
        });

        assert_eq!(
            treasurer.get_fleet_budget(&FleetId(1))?.unwrap(),
            FleetBudget2 {
                current_capital: 39_000.into(), //75 - 36
                reserved_capital: 0.into(),     // we clear the reservation
                total_budget: 75_000.into(),
                ..Default::default()
            }
        );

        assert_eq!(
            serde_json::to_string_pretty(&treasurer.ledger_entries()?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );

        assert_eq!(treasurer.current_agent_credits()?, Credits::new(139_000));

        let sell_price_per_unit = 2_100.into(); // a little more than expected
        treasurer.complete_ticket(&FleetId(1), &sell_ticket, sell_price_per_unit)?;

        expected_ledger_entries.push(LedgerEntry::TicketCompleted {
            fleet_id: FleetId(1),
            finance_ticket: sell_ticket.clone(),
            actual_units: 40,
            actual_price_per_unit: sell_price_per_unit,
            total: 84_000.into(),
        });

        // the excess cash is immediately transferred back
        // 39k + 84k = 123k
        // budget is 75k
        // ==> 48k

        expected_ledger_entries.push(TransferFundsFromFleetToTreasury {
            fleet_id: FleetId(1),
            credits: 48_000.into(),
        });

        assert_eq!(
            treasurer.get_fleet_budget(&FleetId(1))?.unwrap(),
            FleetBudget2 {
                current_capital: 75_000.into(),
                reserved_capital: 0.into(),
                total_budget: 75_000.into(),
                ..Default::default()
            }
        );
        assert_eq!(treasurer.current_agent_credits()?, Credits::new(223_000));
        assert_eq!(
            serde_json::to_string_pretty(&treasurer.ledger_entries()?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );

        Ok(())
    }
}
