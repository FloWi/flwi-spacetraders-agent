use anyhow::anyhow;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FinanceTicket {
    ticket_id: TicketId,
    fleet_id: FleetId,
    ship_symbol: ShipSymbol,
    details: FinanceTicketDetails,
    allocated_credits: Credits,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PurchaseTradeGoodsTicketDetails {
    pub waypoint_symbol: WaypointSymbol,
    pub trade_good: TradeGoodSymbol,
    pub expected_price_per_unit: Credits,
    pub quantity: u32,
    pub expected_total_purchase_price: Credits,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SellTradeGoodsTicketDetails {
    pub waypoint_symbol: WaypointSymbol,
    pub trade_good: TradeGoodSymbol,
    pub expected_price_per_unit: Credits,
    pub quantity: u32,
    pub expected_total_sell_price: Credits,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PurchaseShipTicketDetails {
    pub ship_type: ShipType,
    pub expected_purchase_price: Credits,
    pub shipyard_waypoint_symbol: WaypointSymbol,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RefuelShipTicketDetails {
    pub expected_price_per_unit: Credits,
    pub num_fuel_barrels: u32,
    pub expected_total_purchase_price: Credits,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FinanceTicketDetails {
    PurchaseTradeGoods(PurchaseTradeGoodsTicketDetails),
    SellTradeGoods(SellTradeGoodsTicketDetails),
    PurchaseShip(PurchaseShipTicketDetails),
    RefuelShip(RefuelShipTicketDetails),
}

impl FinanceTicketDetails {
    pub fn signum(&self) -> i64 {
        match self {
            FinanceTicketDetails::PurchaseTradeGoods(PurchaseTradeGoodsTicketDetails { .. }) => -1,
            FinanceTicketDetails::SellTradeGoods(SellTradeGoodsTicketDetails { .. }) => 1,
            FinanceTicketDetails::PurchaseShip(PurchaseShipTicketDetails { .. }) => -1,
            FinanceTicketDetails::RefuelShip(RefuelShipTicketDetails { .. }) => -1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LedgerEntry {
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
    SetNewTotalCapitalForFleet {
        fleet_id: FleetId,
        new_total_capital: Credits,
    },
    SetNewOperatingReserveForFleet {
        fleet_id: FleetId,
        new_operating_reserve: Credits,
    },
}

#[derive(PartialEq, Debug, Default, Clone, Serialize, Deserialize)]
pub struct FleetBudget {
    /// the cash we have at hand - "real money (single source of truth)"
    pub current_capital: Credits,
    /// funds reserved for tickets (not spent yet) - "virtual money"
    pub reserved_capital: Credits,
    /// this is the amount of money we need for operating. If we have more at one point, it will be transferred back to the treasury.
    /// "virtual money"
    pub total_capital: Credits,
    /// the amount of money we'd like to keep - "virtual money"
    pub operating_reserve: Credits,
}

use crate::budgeting::credits::Credits;
use crate::budgeting::treasury_redesign::LedgerEntry::*;
use crate::{FleetId, ShipSymbol, ShipType, TicketId, TradeGoodSymbol, WaypointSymbol};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
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

    pub fn get_fleet_budget(&self, fleet_id: &FleetId) -> Result<FleetBudget> {
        self.with_treasurer(|t| {
            t.fleet_budgets
                .get(fleet_id)
                .cloned()
                .ok_or(anyhow!("Fleet id not found"))
        })
    }

    pub fn current_agent_credits(&self) -> Result<Credits> {
        self.with_treasurer(|t| Ok(t.current_agent_credits()))
    }

    pub fn current_treasury_fund(&self) -> Result<Credits> {
        self.with_treasurer(|t| Ok(t.current_treasury_fund()))
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
        ship_symbol: ShipSymbol,
        quantity: u32,
        expected_price_per_unit: Credits,
    ) -> Result<FinanceTicket> {
        self.with_treasurer(|t| {
            t.create_purchase_trade_goods_ticket(fleet_id, trade_good_symbol, waypoint_symbol, ship_symbol, quantity, expected_price_per_unit)
        })
    }

    pub fn create_sell_trade_goods_ticket(
        &self,
        fleet_id: &FleetId,
        trade_good_symbol: TradeGoodSymbol,
        waypoint_symbol: WaypointSymbol,
        ship_symbol: ShipSymbol,
        quantity: u32,
        expected_price_per_unit: Credits,
    ) -> Result<FinanceTicket> {
        self.with_treasurer(|t| t.create_sell_trade_goods_ticket(fleet_id, trade_good_symbol, waypoint_symbol, ship_symbol, quantity, expected_price_per_unit))
    }

    pub fn create_ship_purchase_ticket(
        &self,
        fleet_id: &FleetId,
        ship_type: ShipType,
        expected_purchase_price: Credits,
        shipyard_waypoint_symbol: WaypointSymbol,
        ship_symbol: ShipSymbol,
    ) -> Result<FinanceTicket> {
        self.with_treasurer(|t| t.create_ship_purchase_ticket(fleet_id, ship_type, expected_purchase_price, shipyard_waypoint_symbol, ship_symbol))
    }

    pub fn get_ticket(&self, ticket_id: &TicketId) -> Result<FinanceTicket> {
        self.with_treasurer(|t| t.get_ticket(ticket_id))
    }

    pub fn complete_ticket(&self, fleet_id: &FleetId, finance_ticket: &FinanceTicket, actual_price_per_unit: Credits) -> Result<()> {
        self.with_treasurer(|t| t.complete_ticket(fleet_id, finance_ticket, actual_price_per_unit))
    }

    pub fn transfer_excess_funds_from_fleet_to_treasury(&self, fleet_id: &FleetId) -> Result<()> {
        self.with_treasurer(|t| t.transfer_excess_funds_from_fleet_to_treasury_if_necessary(fleet_id))
    }

    pub fn set_fleet_total_capital(&self, fleet_id: &FleetId, new_total_capital: Credits) -> Result<()> {
        self.with_treasurer(|t| t.set_fleet_total_capital(fleet_id, new_total_capital))
    }

    pub fn set_new_operating_reserve(&self, fleet_id: &FleetId, new_operating_reserve: Credits) -> Result<()> {
        self.with_treasurer(|t| t.set_new_operating_reserve(fleet_id, new_operating_reserve))
    }

    pub fn ledger_entries(&self) -> Result<VecDeque<LedgerEntry>> {
        self.with_treasurer(|t| Ok(t.ledger_entries.clone()))
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ImprovedTreasurer {
    treasury_fund: Credits,
    ledger_entries: VecDeque<LedgerEntry>,
    fleet_budgets: HashMap<FleetId, FleetBudget>,
    tickets: Vec<FinanceTicket>,
}

impl ImprovedTreasurer {
    pub fn new(starting_credits: Credits) -> Self {
        let mut treasurer = Self {
            treasury_fund: Default::default(),
            ledger_entries: Default::default(),
            fleet_budgets: Default::default(),
            tickets: vec![],
        };

        treasurer
            .process_ledger_entry(TreasuryCreated { credits: starting_credits })
            .unwrap();
        treasurer
    }

    pub fn from_ledger(ledger: Vec<LedgerEntry>) -> Result<Self> {
        // don't call Self::new(), because it creates a ledger_entry
        let mut treasurer: Self = Self {
            treasury_fund: Default::default(),
            ledger_entries: Default::default(),
            fleet_budgets: Default::default(),
            tickets: vec![],
        };

        for entry in ledger {
            treasurer.process_ledger_entry(entry)?
        }

        Ok(treasurer)
    }

    pub fn current_agent_credits(&self) -> Credits {
        self.treasury_fund
            + self
                .fleet_budgets
                .values()
                .map(|budget| budget.current_capital.0)
                .sum::<i64>()
    }

    pub fn current_treasury_fund(&self) -> Credits {
        self.treasury_fund
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
            let diff = fleet_budget.total_capital - fleet_budget.current_capital;
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
        ship_symbol: ShipSymbol,
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
                fleet_id: fleet_id.clone(),
                ship_symbol,
                details: FinanceTicketDetails::PurchaseTradeGoods(PurchaseTradeGoodsTicketDetails {
                    waypoint_symbol,
                    trade_good: trade_good_symbol,
                    expected_total_purchase_price: total,
                    quantity,
                    expected_price_per_unit,
                }),
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
        ship_symbol: ShipSymbol,
        quantity: u32,
        expected_price_per_unit: Credits,
    ) -> Result<FinanceTicket> {
        let ticket = FinanceTicket {
            ticket_id: Default::default(),
            fleet_id: fleet_id.clone(),
            ship_symbol,
            details: FinanceTicketDetails::SellTradeGoods(SellTradeGoodsTicketDetails {
                waypoint_symbol,
                trade_good: trade_good_symbol,
                expected_total_sell_price: expected_price_per_unit * quantity,
                quantity,
                expected_price_per_unit,
            }),
            allocated_credits: 0.into(),
        };

        self.process_ledger_entry(TicketCreated {
            fleet_id: fleet_id.clone(),
            ticket_details: ticket.clone(),
        })?;
        Ok(ticket)
    }

    pub fn create_ship_purchase_ticket(
        &mut self,
        fleet_id: &FleetId,
        ship_type: ShipType,
        expected_purchase_price: Credits,
        shipyard_waypoint_symbol: WaypointSymbol,
        ship_symbol: ShipSymbol,
    ) -> Result<FinanceTicket> {
        let ticket = FinanceTicket {
            ticket_id: Default::default(),
            fleet_id: fleet_id.clone(),
            ship_symbol,
            details: FinanceTicketDetails::PurchaseShip(PurchaseShipTicketDetails {
                ship_type,
                expected_purchase_price,
                shipyard_waypoint_symbol,
            }),
            allocated_credits: expected_purchase_price,
        };

        self.process_ledger_entry(TicketCreated {
            fleet_id: fleet_id.clone(),
            ticket_details: ticket.clone(),
        })?;

        Ok(ticket)
    }

    pub fn get_ticket(&self, ticket_id: &TicketId) -> Result<FinanceTicket> {
        self.tickets
            .iter()
            .find(|t| &t.ticket_id == ticket_id)
            .cloned()
            .ok_or(anyhow!("Ticket not found"))
    }

    pub fn complete_ticket(&mut self, fleet_id: &FleetId, finance_ticket: &FinanceTicket, actual_price_per_unit: Credits) -> Result<()> {
        let quantity: u32 = match finance_ticket.details {
            FinanceTicketDetails::PurchaseTradeGoods(PurchaseTradeGoodsTicketDetails { quantity, .. }) => quantity,
            FinanceTicketDetails::SellTradeGoods(SellTradeGoodsTicketDetails { quantity, .. }) => quantity,
            FinanceTicketDetails::PurchaseShip(PurchaseShipTicketDetails { .. }) => 1,
            FinanceTicketDetails::RefuelShip(RefuelShipTicketDetails { num_fuel_barrels, .. }) => num_fuel_barrels,
        };

        self.process_ledger_entry(TicketCompleted {
            fleet_id: fleet_id.clone(),
            finance_ticket: finance_ticket.clone(),
            actual_units: quantity,
            actual_price_per_unit,
            total: actual_price_per_unit * quantity * finance_ticket.details.signum(),
        })?;

        self.transfer_excess_funds_from_fleet_to_treasury_if_necessary(&fleet_id)?;

        Ok(())
    }

    fn transfer_excess_funds_from_fleet_to_treasury_if_necessary(&mut self, fleet_id: &FleetId) -> Result<()> {
        if let Some(budget) = self.fleet_budgets.get_mut(&fleet_id) {
            let excess = budget.current_capital - budget.total_capital;
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

    fn set_fleet_total_capital(&mut self, fleet_id: &FleetId, new_total_credits: Credits) -> Result<()> {
        if let Some(budget) = self.fleet_budgets.get_mut(&fleet_id) {
            self.process_ledger_entry(SetNewTotalCapitalForFleet {
                fleet_id: fleet_id.clone(),
                new_total_capital: new_total_credits,
            })?;
            self.transfer_excess_funds_from_fleet_to_treasury_if_necessary(&fleet_id)?;

            Ok(())
        } else {
            Err(anyhow!("Fleet {} doesn't exist", fleet_id))
        }
    }

    fn set_new_operating_reserve(&mut self, fleet_id: &FleetId, new_operating_reserve: Credits) -> Result<()> {
        if let Some(_) = self.fleet_budgets.get_mut(&fleet_id) {
            self.process_ledger_entry(SetNewOperatingReserveForFleet {
                fleet_id: fleet_id.clone(),
                new_operating_reserve,
            })?;

            Ok(())
        } else {
            Err(anyhow!("Fleet {} doesn't exist", fleet_id))
        }
    }

    /// This is a "stupid" function that just processes the ledger entry.
    /// e.g. no transfer of excess funds after selling trade goods with a profit to keep available capital below total capital.
    /// This will be done by subsequent calls with new ledger entries.
    /// So don't do recursive calls here - we want ot keep replayability    
    fn process_ledger_entry(&mut self, ledger_entry: LedgerEntry) -> Result<()> {
        let entry = ledger_entry.clone();
        match entry {
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
                    FleetBudget {
                        current_capital: 0.into(),
                        reserved_capital: 0.into(),
                        total_capital: total_capital,
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
                        self.tickets.push(ticket_details);

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
                    self.tickets
                        .retain(|t| t.ticket_id != finance_ticket.ticket_id);

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
            SetNewTotalCapitalForFleet { fleet_id, new_total_capital } => {
                if let Some(budget) = self.fleet_budgets.get_mut(&fleet_id) {
                    budget.total_capital = new_total_capital;
                    self.ledger_entries.push_back(ledger_entry);
                } else {
                    return Err(anyhow!("Fleet {} doesn't exist", fleet_id));
                }
            }
            SetNewOperatingReserveForFleet {
                fleet_id,
                new_operating_reserve,
            } => {
                if let Some(budget) = self.fleet_budgets.get_mut(&fleet_id) {
                    budget.operating_reserve = new_operating_reserve;
                    self.ledger_entries.push_back(ledger_entry);
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
    use crate::budgeting::credits::Credits;
    use crate::budgeting::treasury_redesign::LedgerEntry::TransferFundsFromFleetToTreasury;
    use crate::budgeting::treasury_redesign::{FleetBudget, ImprovedTreasurer, LedgerEntry, ThreadSafeTreasurer};
    use crate::{FleetId, ShipSymbol, ShipType, TradeGoodSymbol, WaypointSymbol};
    use anyhow::Result;
    use itertools::Itertools;

    #[test]
    fn test_fleet_budget_in_trade_cycle() -> Result<()> {
        //Start Fresh with 175k

        let treasurer = ThreadSafeTreasurer::new(175_000.into());
        let mut expected_ledger_entries = vec![LedgerEntry::TreasuryCreated { credits: 175_000.into() }];

        assert_eq!(treasurer.current_agent_credits()?, Credits::new(175_000));
        assert_eq!(
            serde_json::to_string_pretty(&treasurer.ledger_entries()?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );

        // Create fleet with 75k total budget

        let fleet_id = &FleetId(1);
        let ship_symbol = &ShipSymbol("FLWI-1".to_string());

        treasurer.create_fleet(fleet_id, Credits::new(75_000))?;
        expected_ledger_entries.push(LedgerEntry::FleetCreated {
            fleet_id: FleetId(1),
            total_capital: 75_000.into(),
        });

        assert_eq!(
            serde_json::to_string_pretty(&treasurer.ledger_entries()?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );
        assert_eq!(treasurer.current_agent_credits()?, Credits::new(175_000));

        assert_eq!(treasurer.get_fleet_budget(fleet_id)?.current_capital, Credits::new(0));

        // transfer 75k from treasurer to fleet budget

        treasurer.transfer_funds_to_fleet_to_top_up_available_capital(fleet_id)?;
        expected_ledger_entries.push(LedgerEntry::TransferFundsTreasuryToFleet {
            fleet_id: FleetId(1),
            credits: 75_000.into(),
        });

        assert_eq!(
            serde_json::to_string_pretty(&treasurer.ledger_entries()?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );
        assert_eq!(treasurer.current_agent_credits()?, Credits::new(175_000));
        assert_eq!(treasurer.get_fleet_budget(fleet_id)?.current_capital, Credits::new(75_000));

        // create purchase ticket (reduces available capital of fleet)

        let purchase_ticket = treasurer.create_purchase_trade_goods_ticket(
            fleet_id,
            TradeGoodSymbol::ADVANCED_CIRCUITRY,
            WaypointSymbol("FROM".to_string()),
            ship_symbol.clone(),
            40,
            Credits(1_000.into()),
        )?;

        assert_eq!(purchase_ticket.allocated_credits, 40_000.into());
        assert_eq!(treasurer.get_ticket(&purchase_ticket.ticket_id)?, purchase_ticket);
        expected_ledger_entries.push(LedgerEntry::TicketCreated {
            fleet_id: FleetId(1),
            ticket_details: purchase_ticket.clone(),
        });

        assert_eq!(
            treasurer.get_fleet_budget(fleet_id)?,
            FleetBudget {
                current_capital: 75_000.into(),
                reserved_capital: 40_000.into(),
                total_capital: 75_000.into(),
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
            fleet_id,
            TradeGoodSymbol::ADVANCED_CIRCUITRY,
            WaypointSymbol("TO".to_string()),
            ship_symbol.clone(),
            40,
            Credits(2_000.into()),
        )?;

        assert_eq!(treasurer.get_ticket(&sell_ticket.ticket_id)?, sell_ticket);

        assert_eq!(sell_ticket.allocated_credits, 0.into());

        expected_ledger_entries.push(LedgerEntry::TicketCreated {
            fleet_id: FleetId(1),
            ticket_details: sell_ticket.clone(),
        });

        assert_eq!(
            treasurer.get_fleet_budget(fleet_id)?,
            FleetBudget {
                current_capital: 75_000.into(),
                reserved_capital: 40_000.into(),
                total_capital: 75_000.into(),
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
        treasurer.complete_ticket(fleet_id, &purchase_ticket, purchase_price_per_unit)?;

        assert!(treasurer.get_ticket(&purchase_ticket.ticket_id).is_err());

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
            treasurer.get_fleet_budget(fleet_id)?,
            FleetBudget {
                current_capital: 39_000.into(), //75 - 36
                reserved_capital: 0.into(),     // we clear the reservation
                total_capital: 75_000.into(),
                ..Default::default()
            }
        );

        assert_eq!(
            serde_json::to_string_pretty(&treasurer.ledger_entries()?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );

        assert_eq!(treasurer.current_agent_credits()?, Credits::new(139_000));

        let sell_price_per_unit = 2_100.into(); // a little more than expected
        treasurer.complete_ticket(fleet_id, &sell_ticket, sell_price_per_unit)?;
        assert!(treasurer.get_ticket(&sell_ticket.ticket_id).is_err());

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
            treasurer.get_fleet_budget(fleet_id)?,
            FleetBudget {
                current_capital: 75_000.into(),
                reserved_capital: 0.into(),
                total_capital: 75_000.into(),
                ..Default::default()
            }
        );
        assert_eq!(treasurer.current_agent_credits()?, Credits::new(223_000));
        assert_eq!(
            serde_json::to_string_pretty(&treasurer.ledger_entries()?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );

        let current_treasurer = treasurer.get_instance()?;

        let actual_replayed_treasurer = ImprovedTreasurer::from_ledger(expected_ledger_entries)?;

        assert_eq!(
            serde_json::to_string_pretty(&actual_replayed_treasurer)?,
            serde_json::to_string_pretty(&current_treasurer)?
        );

        Ok(())
    }

    #[test]
    fn test_fleet_budget_for_ship_purchases() -> Result<()> {
        //Start Fresh with 175k

        let treasurer = ThreadSafeTreasurer::new(175_000.into());

        let fleet_id = &FleetId(1);
        let ship_symbol = &ShipSymbol("FLWI-1".to_string());

        treasurer.create_fleet(fleet_id, Credits::new(75_000))?;
        treasurer.transfer_funds_to_fleet_to_top_up_available_capital(&FleetId(1))?;

        // we tested the ledger entries up to this point in a different test, so we assume they're correct
        let mut expected_ledger_entries = treasurer.ledger_entries()?.into_iter().collect_vec();

        let ship_purchase_ticket = treasurer.create_ship_purchase_ticket(
            fleet_id,
            ShipType::SHIP_PROBE,
            25_000.into(),
            WaypointSymbol("FROM".to_string()),
            ship_symbol.clone(),
        )?;

        assert_eq!(ship_purchase_ticket.allocated_credits, 25_000.into());

        assert_eq!(
            treasurer.get_fleet_budget(fleet_id)?,
            FleetBudget {
                current_capital: 75_000.into(),
                reserved_capital: 25_000.into(),
                total_capital: 75_000.into(),
                ..Default::default()
            }
        );
        expected_ledger_entries.push(LedgerEntry::TicketCreated {
            fleet_id: FleetId(1),
            ticket_details: ship_purchase_ticket.clone(),
        });

        assert_eq!(treasurer.current_agent_credits()?, Credits::new(175_000));

        assert_eq!(
            serde_json::to_string_pretty(&treasurer.ledger_entries()?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );

        treasurer.complete_ticket(fleet_id, &ship_purchase_ticket, 22_500.into())?; // cheaper than expected
        expected_ledger_entries.push(LedgerEntry::TicketCompleted {
            fleet_id: FleetId(1),
            finance_ticket: ship_purchase_ticket.clone(),
            actual_units: 1,
            actual_price_per_unit: 22_500.into(),
            total: (-22_500).into(),
        });

        assert_eq!(treasurer.current_agent_credits()?, Credits::new(152_500));
        assert_eq!(
            treasurer.get_fleet_budget(fleet_id)?,
            FleetBudget {
                current_capital: 52_500.into(),
                reserved_capital: 0.into(),
                total_capital: 75_000.into(),
                ..Default::default()
            }
        );

        assert_eq!(
            serde_json::to_string_pretty(&treasurer.ledger_entries()?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );

        Ok(())
    }

    #[test]
    fn test_set_fleet_total_capital() -> Result<()> {
        let treasurer = ThreadSafeTreasurer::new(175_000.into());

        let fleet_id = &FleetId(1);

        treasurer.create_fleet(fleet_id, Credits::new(75_000))?;
        treasurer.transfer_funds_to_fleet_to_top_up_available_capital(&FleetId(1))?;

        assert_eq!(treasurer.current_agent_credits()?, Credits::new(175_000));
        assert_eq!(
            treasurer.get_fleet_budget(fleet_id)?,
            FleetBudget {
                current_capital: 75_000.into(),
                reserved_capital: 0.into(),
                total_capital: 75_000.into(),
                ..Default::default()
            }
        );

        // we tested the ledger entries up to this point in a different test, so we assume they're correct
        let mut expected_ledger_entries = treasurer.ledger_entries()?.into_iter().collect_vec();

        treasurer.set_fleet_total_capital(fleet_id, 150_000.into())?;

        assert_eq!(treasurer.current_agent_credits()?, Credits::new(175_000));
        assert_eq!(
            treasurer.get_fleet_budget(fleet_id)?,
            FleetBudget {
                current_capital: 75_000.into(),
                reserved_capital: 0.into(),
                total_capital: 150_000.into(),
                ..Default::default()
            }
        );

        expected_ledger_entries.push(LedgerEntry::SetNewTotalCapitalForFleet {
            fleet_id: fleet_id.clone(),
            new_total_capital: 150_000.into(),
        });

        assert_eq!(
            serde_json::to_string_pretty(&treasurer.ledger_entries()?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );
        //setting total capital below current_capital
        treasurer.set_fleet_total_capital(fleet_id, 50_000.into())?;

        // this will produce two entries in the ledger - one for the set-action and another one for the transfer of funds
        expected_ledger_entries.push(LedgerEntry::SetNewTotalCapitalForFleet {
            fleet_id: fleet_id.clone(),
            new_total_capital: 50_000.into(),
        });

        expected_ledger_entries.push(TransferFundsFromFleetToTreasury {
            fleet_id: fleet_id.clone(),
            credits: 25_000.into(),
        });

        assert_eq!(treasurer.current_agent_credits()?, Credits::new(175_000));
        assert_eq!(
            treasurer.get_fleet_budget(fleet_id)?,
            FleetBudget {
                current_capital: 50_000.into(),
                reserved_capital: 0.into(),
                total_capital: 50_000.into(),
                ..Default::default()
            }
        );

        assert_eq!(treasurer.current_agent_credits()?, Credits::new(175_000));
        assert_eq!(treasurer.current_treasury_fund()?, Credits::new(125_000));

        assert_eq!(
            serde_json::to_string_pretty(&treasurer.ledger_entries()?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );

        Ok(())
    }
}
