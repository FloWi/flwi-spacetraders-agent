use crate::accounting::treasury_redesign::LedgerEntry::*;
use anyhow::Result;
use anyhow::{anyhow, Error};
use clap::parser::ValueSource::DefaultValue;
use itertools::all;
use st_domain::budgeting::credits::Credits;
use st_domain::{FleetId, ShipType, TicketId, TradeGood, TradeGoodSymbol, WaypointSymbol};
use std::collections::{HashMap, VecDeque};

enum Messages {
    PurchasedTradeGoods(TicketId),
    SoldTradeGoods(TicketId),
    SuppliedConstructionSite(TicketId),
    PurchasedShip(TicketId),
}

#[derive(Clone, Debug, PartialEq)]
struct FinanceTicket {
    ticket_id: TicketId,
    details: FinanceTicketDetails,
}
#[derive(Clone, Debug, PartialEq)]
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

#[derive(Clone, Debug, PartialEq)]
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
        allocated_credits: Credits,
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

struct FleetBudget2 {
    available_capital: Credits,
    total_capital: Credits,
}

struct Treasurer2 {
    treasury_fund: Credits,
    ledger_entries: VecDeque<LedgerEntry>,
    fleet_budgets: HashMap<FleetId, FleetBudget2>,
}

impl Treasurer2 {
    pub(crate) fn new(starting_credits: Credits) -> Self {
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
                .map(|budget| budget.available_capital.0)
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
            let diff = fleet_budget.total_capital - fleet_budget.available_capital;
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

    pub(crate) fn create_purchase_trade_goods_ticket(
        &mut self,
        fleet_id: &FleetId,
        trade_good_symbol: TradeGoodSymbol,
        waypoint_symbol: WaypointSymbol,
        quantity: u32,
        expected_price_per_unit: Credits,
    ) -> Result<FinanceTicket> {
        if let Some(fleet_budget) = self.fleet_budgets.get(fleet_id) {
            let fleet_reserve: Credits = 10_000.into();
            let affordable_units: u32 = ((fleet_budget.available_capital - fleet_reserve).0 / expected_price_per_unit.0) as u32;
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
            };
            self.process_ledger_entry(TicketCreated {
                fleet_id: fleet_id.clone(),
                ticket_details: ticket.clone(),
                allocated_credits: total,
            })?;
            Ok(ticket)
        } else {
            Err(anyhow!("Fleet not found {}", fleet_id))
        }
    }

    pub fn complete_ticket(&mut self, fleet_id: &FleetId, finance_ticket: &FinanceTicket, actual_price_per_unit: Credits) -> Result<()> {
        let signed_quantity: i32 = match finance_ticket.details {
            FinanceTicketDetails::PurchaseTradeGoods { quantity, .. } => quantity as i32,
            FinanceTicketDetails::SellTradeGoods { quantity, .. } => -(quantity as i32),
            FinanceTicketDetails::PurchaseShip { .. } => -1,
            FinanceTicketDetails::RefuelShip { num_fuel_barrels, .. } => -(num_fuel_barrels as i32),
        };

        let total = actual_price_per_unit * signed_quantity;

        self.process_ledger_entry(TicketCompleted {
            fleet_id: fleet_id.clone(),
            finance_ticket: finance_ticket.clone(),
            actual_units: signed_quantity.abs() as u32,
            actual_price_per_unit,
            total,
        })?;

        Ok(())
    }

    fn transfer_excess_funds_from_fleet_to_treasury(&mut self, fleet_id: &FleetId) -> Result<()> {
        if let Some(budget) = self.fleet_budgets.get_mut(&fleet_id) {
            let excess = budget.available_capital - budget.total_capital;
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
                        available_capital: Credits::new(0),
                        total_capital,
                    },
                );
                self.ledger_entries.push_back(ledger_entry);
            }
            TransferFundsTreasuryToFleet { fleet_id, credits } => {
                if let Some(budget) = self.fleet_budgets.get_mut(&fleet_id) {
                    self.treasury_fund -= credits;
                    budget.available_capital += credits;

                    self.ledger_entries.push_back(ledger_entry);
                } else {
                    return Err(anyhow!("Fleet {} doesn't exist", fleet_id));
                }
            }

            TicketCreated {
                fleet_id, allocated_credits, ..
            } => {
                if let Some(budget) = self.fleet_budgets.get_mut(&fleet_id) {
                    if budget.available_capital >= allocated_credits {
                        budget.available_capital -= allocated_credits;

                        self.ledger_entries.push_back(ledger_entry);
                    } else {
                        return Err(anyhow!(
                            "Insufficient funds. available_capital: {}; allocated_credits: {allocated_credits}",
                            budget.available_capital
                        ));
                    }
                } else {
                    return Err(anyhow!("Fleet {} doesn't exist", fleet_id));
                }
            }
            TicketCompleted { fleet_id, total, .. } => {
                if let Some(budget) = self.fleet_budgets.get_mut(&fleet_id) {
                    budget.available_capital += total;

                    self.ledger_entries.push_back(ledger_entry);

                    self.transfer_excess_funds_from_fleet_to_treasury(&fleet_id)?
                } else {
                    return Err(anyhow!("Fleet {} doesn't exist", fleet_id));
                }
            }
            TransferFundsFromFleetToTreasury { fleet_id, credits } => {
                if let Some(budget) = self.fleet_budgets.get_mut(&fleet_id) {
                    if budget.available_capital < credits {
                        return Err(anyhow!(
                            "Insufficient funds for transfering funds from fleet {fleet_id} to treasury. available_capital: {}; credits_to_transfer: {credits}",
                            budget.available_capital
                        ));
                    } else {
                        budget.available_capital -= credits;
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
    use crate::accounting::treasury_redesign::{LedgerEntry, Treasurer2};
    use anyhow::Result;
    use st_domain::budgeting::credits::Credits;
    use st_domain::{FleetId, TradeGoodSymbol, WaypointSymbol};

    #[test]
    //#[tokio::test] // for accessing runtime-infos with tokio-conso&le
    fn foo() -> Result<()> {
        //Start Fresh with 175k

        let mut treasurer = Treasurer2::new(175_000.into());
        let mut expected_ledger_entries = vec![LedgerEntry::TreasuryCreated { credits: 175_000.into() }];

        assert_eq!(treasurer.current_agent_credits(), Credits::new(175_000));
        assert_eq!(treasurer.ledger_entries, expected_ledger_entries);

        // Create fleet with 75k total budget

        treasurer.create_fleet(&FleetId(1), Credits::new(75_000))?;
        expected_ledger_entries.push(LedgerEntry::FleetCreated {
            fleet_id: FleetId(1),
            total_capital: 75_000.into(),
        });

        assert_eq!(treasurer.ledger_entries, expected_ledger_entries);
        assert_eq!(treasurer.current_agent_credits(), Credits::new(175_000));

        assert_eq!(
            treasurer
                .fleet_budgets
                .get(&FleetId(1))
                .unwrap()
                .available_capital,
            Credits::new(0)
        );

        treasurer.transfer_funds_to_fleet_to_top_up_available_capital(&FleetId(1))?;
        expected_ledger_entries.push(LedgerEntry::TransferFundsTreasuryToFleet {
            fleet_id: FleetId(1),
            credits: 75_000.into(),
        });

        assert_eq!(treasurer.ledger_entries, expected_ledger_entries);
        assert_eq!(treasurer.current_agent_credits(), Credits::new(175_000));
        assert_eq!(
            treasurer
                .fleet_budgets
                .get(&FleetId(1))
                .unwrap()
                .available_capital,
            Credits::new(75_000)
        );

        let purchase_ticket_ledger_entry = treasurer.create_purchase_trade_goods_ticket(
            &FleetId(1),
            TradeGoodSymbol::ADVANCED_CIRCUITRY,
            WaypointSymbol("FROM".to_string()),
            40,
            Credits(1_000.into()),
        )?;

        assert_eq!(
            treasurer
                .fleet_budgets
                .get(&FleetId(1))
                .unwrap()
                .available_capital,
            Credits::new(35_000)
        );

        expected_ledger_entries.push(LedgerEntry::TicketCreated {
            fleet_id: FleetId(1),
            ticket_details: purchase_ticket_ledger_entry.clone(),
            allocated_credits: 40_000.into(),
        });

        assert_eq!(treasurer.ledger_entries, expected_ledger_entries);
        assert_eq!(
            treasurer
                .fleet_budgets
                .get(&FleetId(1))
                .unwrap()
                .available_capital,
            Credits::new(35_000)
        );
        assert_eq!(treasurer.current_agent_credits(), Credits::new(135_000));

        treasurer.complete_ticket(&FleetId(1), &purchase_ticket_ledger_entry, 900.into())?;

        expected_ledger_entries.push(LedgerEntry::TicketCompleted {
            fleet_id: FleetId(1),
            finance_ticket: purchase_ticket_ledger_entry.clone(),
            actual_units: 40,
            actual_price_per_unit: 900.into(),
            total: 36_000.into(),
        });

        assert_eq!(treasurer.ledger_entries, expected_ledger_entries);
        assert_eq!(
            treasurer
                .fleet_budgets
                .get(&FleetId(1))
                .unwrap()
                .available_capital,
            Credits::new(71_000)
        );
        assert_eq!(treasurer.current_agent_credits(), Credits::new(171_000));

        Ok(())
    }
}
