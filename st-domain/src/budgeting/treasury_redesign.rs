use crate::serialize_as_sorted_map;
use anyhow::anyhow;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Eq, Hash)]
pub struct FinanceTicket {
    pub ticket_id: TicketId,
    pub fleet_id: FleetId,
    pub ship_symbol: ShipSymbol,
    pub details: FinanceTicketDetails,
    pub allocated_credits: Credits,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Eq, Hash)]
pub struct PurchaseTradeGoodsTicketDetails {
    pub waypoint_symbol: WaypointSymbol,
    pub trade_good: TradeGoodSymbol,
    pub expected_price_per_unit: Credits,
    pub quantity: u32,
    pub expected_total_purchase_price: Credits,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Eq, Hash)]
pub struct SellTradeGoodsTicketDetails {
    pub waypoint_symbol: WaypointSymbol,
    pub trade_good: TradeGoodSymbol,
    pub expected_price_per_unit: Credits,
    pub quantity: u32,
    pub expected_total_sell_price: Credits,
    pub maybe_matching_purchase_ticket: Option<TicketId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Eq, Hash)]
pub struct DeliverConstructionMaterialsTicketDetails {
    pub waypoint_symbol: WaypointSymbol,
    pub trade_good: TradeGoodSymbol,
    pub quantity: u32,
    pub maybe_matching_purchase_ticket: Option<TicketId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Eq, Hash)]
pub struct PurchaseShipTicketDetails {
    pub ship_type: ShipType,
    pub assigned_fleet_id: FleetId,
    pub expected_purchase_price: Credits,
    pub waypoint_symbol: WaypointSymbol,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Eq, Hash)]
pub struct RefuelShipTicketDetails {
    pub expected_price_per_unit: Credits,
    pub num_fuel_barrels: u32,
    pub expected_total_purchase_price: Credits,
    pub waypoint_symbol: WaypointSymbol,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct ActiveTradeRoute {
    pub from: WaypointSymbol,
    pub to: WaypointSymbol,
    pub trade_good: TradeGoodSymbol,
    pub number_ongoing_trades: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinanceResult {
    FleetAlreadyHadSufficientFunds,
    TransferSuccessful { transfer_sum: Credits },
    TransferFailed { missing: Credits },
}

#[derive(Clone, Debug, Display, PartialEq, Serialize, Deserialize, Eq, Hash)]
pub enum FinanceTicketDetails {
    PurchaseTradeGoods(PurchaseTradeGoodsTicketDetails),
    SellTradeGoods(SellTradeGoodsTicketDetails),
    SupplyConstructionSite(DeliverConstructionMaterialsTicketDetails),
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
            FinanceTicketDetails::SupplyConstructionSite(_) => 0,
        }
    }

    pub fn get_description(&self) -> String {
        match self {
            FinanceTicketDetails::PurchaseTradeGoods(d) => format!(
                "Purchase of {} units of {} à {}/unit at {} for a total of {}",
                d.quantity, d.trade_good, d.expected_price_per_unit, d.waypoint_symbol, d.expected_total_purchase_price
            ),
            FinanceTicketDetails::SellTradeGoods(d) => format!(
                "Sell of {} units of {} à {}/unit at {} for a total of {}",
                d.quantity, d.trade_good, d.expected_price_per_unit, d.waypoint_symbol, d.expected_total_sell_price
            ),
            FinanceTicketDetails::PurchaseShip(d) => format!(
                "ShipPurchase of {} for {} at {} for fleet #{}",
                d.ship_type, d.expected_purchase_price, d.waypoint_symbol, d.assigned_fleet_id
            ),
            FinanceTicketDetails::RefuelShip(d) => format!(
                "Refueling of {} fuel-barrels à {} at {} for a total of {}",
                d.num_fuel_barrels, d.expected_price_per_unit, d.waypoint_symbol, d.expected_total_purchase_price
            ),
            FinanceTicketDetails::SupplyConstructionSite(d) => format!("Delivering of {} units of {} to {}", d.quantity, d.trade_good, d.waypoint_symbol),
        }
    }

    pub fn get_units(&self) -> u32 {
        match self {
            FinanceTicketDetails::PurchaseTradeGoods(d) => d.quantity,
            FinanceTicketDetails::SellTradeGoods(d) => d.quantity,
            FinanceTicketDetails::PurchaseShip(_) => 1,
            FinanceTicketDetails::RefuelShip(d) => d.num_fuel_barrels,
            FinanceTicketDetails::SupplyConstructionSite(d) => d.quantity,
        }
    }

    pub fn get_waypoint(&self) -> WaypointSymbol {
        match self {
            FinanceTicketDetails::PurchaseTradeGoods(d) => d.waypoint_symbol.clone(),
            FinanceTicketDetails::SellTradeGoods(d) => d.waypoint_symbol.clone(),
            FinanceTicketDetails::PurchaseShip(d) => d.waypoint_symbol.clone(),
            FinanceTicketDetails::RefuelShip(d) => d.waypoint_symbol.clone(),
            FinanceTicketDetails::SupplyConstructionSite(d) => d.waypoint_symbol.clone(),
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
        trade_good_symbol: TradeGoodSymbol,
        units: u32,
        price_per_unit: Credits,
        total: Credits,
    },
    TransferredFundsFromFleetToTreasury {
        fleet_id: FleetId,
        credits: Credits,
    },
    TransferredFundsFromTreasuryToFleet {
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
    ArchivedFleetBudget {
        fleet_id: FleetId,
        budget: FleetBudget,
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
    pub budget: Credits,
    /// the amount of money we'd like to keep - "virtual money"
    pub operating_reserve: Credits,
}

impl FleetBudget {
    pub fn available_capital(&self) -> Credits {
        self.current_capital - self.reserved_capital - self.operating_reserve
    }
}

use crate::budgeting::credits::Credits;
use crate::budgeting::treasury_redesign::LedgerEntry::*;
use crate::{FleetId, ShipSymbol, ShipType, TicketId, TradeGoodSymbol, WaypointSymbol};
use itertools::Itertools;
use std::sync::{Arc, Mutex};
use strum::Display;

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

    pub fn get_fleet_tickets(&self) -> Result<HashMap<FleetId, Vec<FinanceTicket>>> {
        self.with_treasurer(|t| t.get_fleet_tickets())
    }
    pub fn get_fleet_budgets(&self) -> Result<HashMap<FleetId, FleetBudget>> {
        self.with_treasurer(|t| t.get_fleet_budgets())
    }
    pub fn get_active_tickets(&self) -> Result<HashMap<TicketId, FinanceTicket>> {
        self.with_treasurer(|t| t.get_active_tickets())
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
        maybe_matching_purchase_ticket: Option<TicketId>,
    ) -> Result<FinanceTicket> {
        self.with_treasurer(|t| {
            t.create_sell_trade_goods_ticket(
                fleet_id,
                trade_good_symbol,
                waypoint_symbol,
                ship_symbol,
                quantity,
                expected_price_per_unit,
                maybe_matching_purchase_ticket,
            )
        })
    }

    pub fn create_delivery_construction_material_ticket(
        &self,
        fleet_id: &FleetId,
        trade_good_symbol: TradeGoodSymbol,
        waypoint_symbol: WaypointSymbol,
        ship_symbol: ShipSymbol,
        quantity: u32,
        maybe_matching_purchase_ticket: Option<TicketId>,
    ) -> Result<FinanceTicket> {
        self.with_treasurer(|t| {
            t.create_delivery_construction_material_ticket(
                fleet_id,
                trade_good_symbol,
                waypoint_symbol,
                ship_symbol,
                quantity,
                maybe_matching_purchase_ticket,
            )
        })
    }

    pub fn report_expense(
        &self,
        fleet_id: &FleetId,
        current_destination: Option<WaypointSymbol>,
        current_tickets: Vec<FinanceTicket>,
        trade_good_symbol: TradeGoodSymbol,
        units: u32,
        price_per_unit: Credits,
    ) -> Result<()> {
        self.with_treasurer(|t| t.report_expense(fleet_id, current_destination, current_tickets, trade_good_symbol, units, price_per_unit))
    }

    pub fn get_active_trade_routes(&self) -> Result<Vec<ActiveTradeRoute>> {
        self.with_treasurer(|t| t.get_active_trade_routes())
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

    pub fn try_finance_purchase_for_fleet(&self, fleet_id: &FleetId, required_credits: Credits) -> Result<FinanceResult> {
        self.with_treasurer(|t| t.try_finance_purchase_for_fleet(fleet_id, required_credits))
    }

    pub fn set_fleet_budget(&self, fleet_id: &FleetId, new_total_capital: Credits) -> Result<()> {
        self.with_treasurer(|t| t.set_fleet_total_capital(fleet_id, new_total_capital))
    }

    pub fn set_new_operating_reserve(&self, fleet_id: &FleetId, new_operating_reserve: Credits) -> Result<()> {
        self.with_treasurer(|t| t.set_new_operating_reserve(fleet_id, new_operating_reserve))
    }

    pub fn ledger_entries(&self) -> Result<VecDeque<LedgerEntry>> {
        self.with_treasurer(|t| Ok(t.ledger_entries.clone()))
    }

    pub fn transfer_all_funds_to_treasury(&self) -> Result<()> {
        self.with_treasurer(|t| t.transfer_all_funds_to_treasury())
    }
    pub fn remove_all_fleets(&self) -> Result<()> {
        self.with_treasurer(|t| t.remove_all_fleets())
    }

    pub fn remove_fleet(&self, fleet_id: &FleetId) -> Result<()> {
        self.with_treasurer(|t| t.remove_fleet(fleet_id))
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ImprovedTreasurer {
    treasury_fund: Credits,
    ledger_entries: VecDeque<LedgerEntry>,

    #[serde(serialize_with = "serialize_as_sorted_map")]
    fleet_budgets: HashMap<FleetId, FleetBudget>,

    #[serde(serialize_with = "serialize_as_sorted_map")]
    active_tickets: HashMap<TicketId, FinanceTicket>,

    #[serde(serialize_with = "serialize_as_sorted_map")]
    completed_tickets: HashMap<TicketId, FinanceTicket>,
}

impl ImprovedTreasurer {
    pub fn new(starting_credits: Credits) -> Self {
        let mut treasurer = Self {
            treasury_fund: Default::default(),
            ledger_entries: Default::default(),
            fleet_budgets: Default::default(),
            active_tickets: Default::default(),
            completed_tickets: Default::default(),
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
            active_tickets: Default::default(),
            completed_tickets: Default::default(),
        };

        for entry in ledger {
            treasurer.process_ledger_entry(entry)?
        }

        Ok(treasurer)
    }

    pub fn get_fleet_tickets(&self) -> Result<HashMap<FleetId, Vec<FinanceTicket>>> {
        Ok(self
            .active_tickets
            .values()
            .cloned()
            .into_group_map_by(|t| t.fleet_id.clone()))
    }

    pub fn get_fleet_budgets(&self) -> Result<HashMap<FleetId, FleetBudget>> {
        Ok(self.fleet_budgets.clone())
    }

    pub fn get_active_tickets(&self) -> Result<HashMap<TicketId, FinanceTicket>> {
        Ok(self.active_tickets.clone())
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

    fn reimburse_expense(&mut self, fleet_id: &FleetId, credits: Credits) -> Result<()> {
        if let Some(fleet_budget) = self.fleet_budgets.get(fleet_id) {
            if credits > self.treasury_fund {
                anyhow::bail!("Insufficient funds for reimbursing fleet #{} of {}", fleet_id, credits);
            }
            self.process_ledger_entry(TransferredFundsFromTreasuryToFleet {
                fleet_id: fleet_id.clone(),
                credits,
            })
        } else {
            Err(anyhow!("Fleet not found {}", fleet_id))
        }
    }

    fn transfer_funds_to_fleet_to_top_up_available_capital(&mut self, fleet_id: &FleetId) -> Result<()> {
        if let Some(fleet_budget) = self.fleet_budgets.get(fleet_id) {
            let diff = fleet_budget.budget - fleet_budget.current_capital;
            if diff.is_positive() {
                let transfer_sum = self.treasury_fund.min(diff);
                self.process_ledger_entry(TransferredFundsFromTreasuryToFleet {
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

    fn get_active_trade_routes(&self) -> Result<Vec<ActiveTradeRoute>> {
        let mut active_routes = HashMap::new();

        // we have two types of delivery tickets - SellTradeGoods and SupplyConstructionSite
        for (waypoint_symbol, trade_good, maybe_matching_purchase_ticket) in self
            .active_tickets
            .values()
            .filter_map(|t| match &t.details {
                FinanceTicketDetails::SupplyConstructionSite(d) => {
                    Some((d.waypoint_symbol.clone(), d.trade_good.clone(), d.maybe_matching_purchase_ticket.clone()))
                }
                FinanceTicketDetails::SellTradeGoods(d) => Some((d.waypoint_symbol.clone(), d.trade_good.clone(), d.maybe_matching_purchase_ticket.clone())),
                FinanceTicketDetails::PurchaseTradeGoods(_) => None,
                FinanceTicketDetails::PurchaseShip(_) => None,
                FinanceTicketDetails::RefuelShip(_) => None,
            })
        {
            if let Some(purchase_ticket_id) = maybe_matching_purchase_ticket {
                let maybe_purchase_ticket = self
                    .active_tickets
                    .get(&purchase_ticket_id)
                    .or_else(|| self.completed_tickets.get(&purchase_ticket_id));

                if let Some(purchase_ticket) = maybe_purchase_ticket {
                    let from_wp = purchase_ticket.details.get_waypoint();
                    let to_wp = waypoint_symbol.clone();
                    active_routes
                        .entry((from_wp, to_wp, trade_good.clone()))
                        .and_modify(|counter| *counter += 1)
                        .or_insert(1);
                }
            }
        }

        Ok(active_routes
            .into_iter()
            .map(|((from, to, trade_good), number_ongoing_trades)| ActiveTradeRoute {
                from,
                to,
                trade_good,
                number_ongoing_trades,
            })
            .collect_vec())
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
            let affordable_units: u32 = (fleet_budget.available_capital().0 / expected_price_per_unit.0) as u32;
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
        maybe_matching_purchase_ticket: Option<TicketId>,
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
                maybe_matching_purchase_ticket,
            }),
            allocated_credits: 0.into(),
        };

        self.process_ledger_entry(TicketCreated {
            fleet_id: fleet_id.clone(),
            ticket_details: ticket.clone(),
        })?;
        Ok(ticket)
    }

    pub fn create_delivery_construction_material_ticket(
        &mut self,
        fleet_id: &FleetId,
        trade_good_symbol: TradeGoodSymbol,
        waypoint_symbol: WaypointSymbol,
        ship_symbol: ShipSymbol,
        quantity: u32,
        maybe_matching_purchase_ticket: Option<TicketId>,
    ) -> Result<FinanceTicket> {
        let ticket = FinanceTicket {
            ticket_id: Default::default(),
            fleet_id: fleet_id.clone(),
            ship_symbol,
            details: FinanceTicketDetails::SupplyConstructionSite(DeliverConstructionMaterialsTicketDetails {
                waypoint_symbol,
                trade_good: trade_good_symbol,
                quantity,
                maybe_matching_purchase_ticket,
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
                assigned_fleet_id: fleet_id.clone(),
                expected_purchase_price,
                waypoint_symbol: shipyard_waypoint_symbol,
            }),
            allocated_credits: expected_purchase_price,
        };

        self.process_ledger_entry(TicketCreated {
            fleet_id: fleet_id.clone(),
            ticket_details: ticket.clone(),
        })?;

        Ok(ticket)
    }

    pub(crate) fn report_expense(
        &mut self,
        fleet_id: &FleetId,
        current_destination: Option<WaypointSymbol>,
        current_tickets: Vec<FinanceTicket>,
        trade_good_symbol: TradeGoodSymbol,
        units: u32,
        price_per_unit: Credits,
    ) -> Result<()> {
        let maybe_ticket = current_destination.clone().and_then(|destination| {
            current_tickets
                .iter()
                .find(|t| t.details.get_waypoint() == destination)
        });

        let total = price_per_unit * units;

        if self.treasury_fund >= total {
            self.reimburse_expense(fleet_id, total)?;
        }

        self.process_ledger_entry(ExpenseLogged {
            fleet_id: fleet_id.clone(),
            maybe_ticket_id: maybe_ticket.map(|t| t.ticket_id.clone()),
            trade_good_symbol,
            units,
            price_per_unit,
            total,
        })?;

        Ok(())
    }

    pub fn get_ticket(&self, ticket_id: &TicketId) -> Result<FinanceTicket> {
        self.active_tickets
            .get(ticket_id)
            .cloned()
            .ok_or(anyhow!("Ticket not found"))
    }

    pub fn complete_ticket(&mut self, fleet_id: &FleetId, finance_ticket: &FinanceTicket, actual_price_per_unit: Credits) -> Result<()> {
        let quantity: u32 = match finance_ticket.details {
            FinanceTicketDetails::PurchaseTradeGoods(PurchaseTradeGoodsTicketDetails { quantity, .. }) => quantity,
            FinanceTicketDetails::SellTradeGoods(SellTradeGoodsTicketDetails { quantity, .. }) => quantity,
            FinanceTicketDetails::PurchaseShip(PurchaseShipTicketDetails { .. }) => 1,
            FinanceTicketDetails::RefuelShip(RefuelShipTicketDetails { num_fuel_barrels, .. }) => num_fuel_barrels,
            FinanceTicketDetails::SupplyConstructionSite(DeliverConstructionMaterialsTicketDetails { quantity, .. }) => quantity,
        };

        self.process_ledger_entry(TicketCompleted {
            fleet_id: fleet_id.clone(),
            finance_ticket: finance_ticket.clone(),
            actual_units: quantity,
            actual_price_per_unit,
            total: actual_price_per_unit * quantity * finance_ticket.details.signum(),
        })?;

        self.transfer_excess_funds_from_fleet_to_treasury_if_necessary(fleet_id)?;

        Ok(())
    }

    fn transfer_excess_funds_from_fleet_to_treasury_if_necessary(&mut self, fleet_id: &FleetId) -> Result<()> {
        if let Some(budget) = self.fleet_budgets.get_mut(fleet_id) {
            let excess = budget.current_capital - budget.budget;
            if excess.is_positive() {
                self.process_ledger_entry(TransferredFundsFromFleetToTreasury {
                    fleet_id: fleet_id.clone(),
                    credits: excess,
                })?;
            }
            Ok(())
        } else {
            Err(anyhow!("Fleet {} doesn't exist", fleet_id))
        }
    }

    fn try_finance_purchase_for_fleet(&mut self, fleet_id: &FleetId, required_credits: Credits) -> Result<FinanceResult> {
        if let Some(_) = self.fleet_budgets.get_mut(fleet_id) {
            if required_credits.is_negative() || required_credits.is_zero() {
                // no need to transfer - fleet has enough budget
                Ok(FinanceResult::FleetAlreadyHadSufficientFunds)
            } else {
                let diff_from_treasury = self.current_treasury_fund() - required_credits;
                if diff_from_treasury.is_positive() {
                    self.process_ledger_entry(TransferredFundsFromTreasuryToFleet {
                        fleet_id: fleet_id.clone(),
                        credits: required_credits,
                    })?;
                    Ok(FinanceResult::TransferSuccessful {
                        transfer_sum: required_credits,
                    })
                } else {
                    Ok(FinanceResult::TransferFailed {
                        missing: diff_from_treasury.abs(),
                    })
                }
            }
        } else {
            Err(anyhow!("Fleet {} doesn't exist", fleet_id))
        }
    }

    fn set_fleet_total_capital(&mut self, fleet_id: &FleetId, new_total_credits: Credits) -> Result<()> {
        if let Some(budget) = self.fleet_budgets.get_mut(fleet_id) {
            self.process_ledger_entry(SetNewTotalCapitalForFleet {
                fleet_id: fleet_id.clone(),
                new_total_capital: new_total_credits,
            })?;
            self.transfer_excess_funds_from_fleet_to_treasury_if_necessary(fleet_id)?;

            Ok(())
        } else {
            Err(anyhow!("Fleet {} doesn't exist", fleet_id))
        }
    }

    fn set_new_operating_reserve(&mut self, fleet_id: &FleetId, new_operating_reserve: Credits) -> Result<()> {
        if self.fleet_budgets.get_mut(fleet_id).is_some() {
            self.process_ledger_entry(SetNewOperatingReserveForFleet {
                fleet_id: fleet_id.clone(),
                new_operating_reserve,
            })?;

            Ok(())
        } else {
            Err(anyhow!("Fleet {} doesn't exist", fleet_id))
        }
    }

    pub fn transfer_all_funds_to_treasury(&mut self) -> Result<()> {
        for fleet_id in self.fleet_budgets.keys().cloned().sorted_by_key(|id| id.0) {
            self.transfer_all_funds_from_fleet_to_treasury(&fleet_id)?
        }
        Ok(())
    }

    fn transfer_all_funds_from_fleet_to_treasury(&mut self, fleet_id: &FleetId) -> Result<()> {
        if let Some(budget) = self.fleet_budgets.get(fleet_id) {
            if budget.current_capital.is_positive() {
                self.process_ledger_entry(TransferredFundsFromFleetToTreasury {
                    fleet_id: fleet_id.clone(),
                    credits: budget.current_capital,
                })?;
            }
            Ok(())
        } else {
            Err(anyhow!("Fleet {} doesn't exist", fleet_id))
        }
    }

    pub fn remove_all_fleets(&mut self) -> Result<()> {
        // make sure we don't void any cash
        self.transfer_all_funds_to_treasury()?;

        for (fleet_id, budget) in self
            .fleet_budgets
            .clone()
            .iter()
            .sorted_by_key(|(id, _)| id.0)
        {
            self.remove_fleet(fleet_id)?
        }

        Ok(())
    }

    pub(crate) fn remove_fleet(&mut self, fleet_id: &FleetId) -> Result<()> {
        self.transfer_all_funds_from_fleet_to_treasury(fleet_id)?;

        if let Some(budget) = self.fleet_budgets.get(fleet_id) {
            self.process_ledger_entry(ArchivedFleetBudget {
                fleet_id: fleet_id.clone(),
                budget: budget.clone(),
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
                        budget: total_capital,
                        ..Default::default()
                    },
                );
                self.ledger_entries.push_back(ledger_entry);
            }
            TransferredFundsFromTreasuryToFleet { fleet_id, credits } => {
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
                        self.active_tickets
                            .insert(ticket_details.ticket_id, ticket_details);

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
                    self.active_tickets.remove(&finance_ticket.ticket_id);

                    self.completed_tickets
                        .insert(finance_ticket.ticket_id, finance_ticket);

                    self.ledger_entries.push_back(ledger_entry);
                } else {
                    return Err(anyhow!("Fleet {} doesn't exist", fleet_id));
                }
            }
            TransferredFundsFromFleetToTreasury { fleet_id, credits } => {
                if let Some(budget) = self.fleet_budgets.get_mut(&fleet_id) {
                    if budget.current_capital < credits {
                        return Err(anyhow!(
                            "Insufficient funds for transferring funds from fleet {fleet_id} to treasury. available_capital: {}; credits_to_transfer: {credits}",
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
                    budget.budget = new_total_capital;
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
            ArchivedFleetBudget { fleet_id, .. } => {
                if self.fleet_budgets.remove(&fleet_id).is_some() {
                    self.ledger_entries.push_back(ledger_entry);
                } else {
                    return Err(anyhow!("Fleet {} doesn't exist", fleet_id));
                }
            }
            ExpenseLogged { fleet_id, total, .. } => {
                if let Some(budget) = self.fleet_budgets.get_mut(&fleet_id) {
                    budget.current_capital -= total;

                    self.ledger_entries.push_back(ledger_entry);
                } else {
                    return Err(anyhow!("Fleet {} doesn't exist", fleet_id));
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::budgeting::credits::Credits;
    use crate::budgeting::treasury_redesign::LedgerEntry::{ArchivedFleetBudget, TransferredFundsFromFleetToTreasury};
    use crate::budgeting::treasury_redesign::{ActiveTradeRoute, FinanceResult, FleetBudget, ImprovedTreasurer, LedgerEntry, ThreadSafeTreasurer};
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
        expected_ledger_entries.push(LedgerEntry::TransferredFundsFromTreasuryToFleet {
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
                budget: 75_000.into(),
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
            Some(purchase_ticket.ticket_id),
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
                budget: 75_000.into(),
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
                budget: 75_000.into(),
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

        expected_ledger_entries.push(TransferredFundsFromFleetToTreasury {
            fleet_id: FleetId(1),
            credits: 48_000.into(),
        });

        assert_eq!(
            treasurer.get_fleet_budget(fleet_id)?,
            FleetBudget {
                current_capital: 75_000.into(),
                reserved_capital: 0.into(),
                budget: 75_000.into(),
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
                budget: 75_000.into(),
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
                budget: 75_000.into(),
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
                budget: 75_000.into(),
                ..Default::default()
            }
        );

        // we tested the ledger entries up to this point in a different test, so we assume they're correct
        let mut expected_ledger_entries = treasurer.ledger_entries()?.into_iter().collect_vec();

        treasurer.set_fleet_budget(fleet_id, 150_000.into())?;

        assert_eq!(treasurer.current_agent_credits()?, Credits::new(175_000));
        assert_eq!(
            treasurer.get_fleet_budget(fleet_id)?,
            FleetBudget {
                current_capital: 75_000.into(),
                reserved_capital: 0.into(),
                budget: 150_000.into(),
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
        treasurer.set_fleet_budget(fleet_id, 50_000.into())?;

        // this will produce two entries in the ledger - one for the set-action and another one for the transfer of funds
        expected_ledger_entries.push(LedgerEntry::SetNewTotalCapitalForFleet {
            fleet_id: fleet_id.clone(),
            new_total_capital: 50_000.into(),
        });

        expected_ledger_entries.push(TransferredFundsFromFleetToTreasury {
            fleet_id: fleet_id.clone(),
            credits: 25_000.into(),
        });

        assert_eq!(treasurer.current_agent_credits()?, Credits::new(175_000));
        assert_eq!(
            treasurer.get_fleet_budget(fleet_id)?,
            FleetBudget {
                current_capital: 50_000.into(),
                reserved_capital: 0.into(),
                budget: 50_000.into(),
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

    #[test]
    fn test_getting_active_trade_routes() -> Result<()> {
        let treasurer = ThreadSafeTreasurer::new(175_000.into());

        let fleet_id = &FleetId(1);
        let ship_symbol = &ShipSymbol("FLWI-1".to_string());

        treasurer.create_fleet(fleet_id, Credits::new(75_000))?;

        treasurer.transfer_funds_to_fleet_to_top_up_available_capital(fleet_id)?;

        let trade_good = TradeGoodSymbol::ADVANCED_CIRCUITRY;
        let from_wps = WaypointSymbol("FROM".to_string());
        let to_wps = WaypointSymbol("TO".to_string());
        let completed_purchase_ticket_1 =
            treasurer.create_purchase_trade_goods_ticket(fleet_id, trade_good.clone(), from_wps.clone(), ship_symbol.clone(), 40, Credits(1_000.into()))?;

        treasurer.complete_ticket(fleet_id, &completed_purchase_ticket_1, 1_000.into())?;

        let purchase_ticket_2 =
            treasurer.create_purchase_trade_goods_ticket(fleet_id, trade_good.clone(), from_wps.clone(), ship_symbol.clone(), 40, Credits(1_000.into()))?;

        let _sell_ticket_1 = treasurer.create_sell_trade_goods_ticket(
            fleet_id,
            trade_good.clone(),
            to_wps.clone(),
            ship_symbol.clone(),
            40,
            Credits(2_000.into()),
            Some(completed_purchase_ticket_1.ticket_id),
        )?;

        let _sell_ticket_2 = treasurer.create_sell_trade_goods_ticket(
            fleet_id,
            trade_good.clone(),
            to_wps.clone(),
            ship_symbol.clone(),
            40,
            Credits(2_000.into()),
            Some(purchase_ticket_2.ticket_id),
        )?;

        let _unrelated_sell_ticket = treasurer.create_sell_trade_goods_ticket(
            fleet_id,
            trade_good.clone(),
            to_wps.clone(),
            ship_symbol.clone(),
            20,
            Credits(1_234.into()),
            None,
        )?;

        assert_eq!(
            treasurer.get_active_trade_routes()?,
            vec![ActiveTradeRoute {
                from: from_wps,
                to: to_wps,
                trade_good: trade_good.clone(),
                number_ongoing_trades: 2,
            }]
        );

        Ok(())
    }

    #[test]
    fn test_financing_of_purchase() -> Result<()> {
        let treasurer = ThreadSafeTreasurer::new(175_000.into());

        let fleet_id = &FleetId(1);
        let ship_symbol = &ShipSymbol("FLWI-1".to_string());

        treasurer.create_fleet(fleet_id, Credits::new(10_000))?;
        treasurer.transfer_funds_to_fleet_to_top_up_available_capital(fleet_id)?;

        // we tested the ledger entries up to this point in a different test, so we assume they're correct
        let mut expected_ledger_entries = treasurer.ledger_entries()?.into_iter().collect_vec();

        let no_op_finance_result = treasurer.try_finance_purchase_for_fleet(fleet_id, 5_000.into())?;

        assert_eq!(no_op_finance_result, FinanceResult::FleetAlreadyHadSufficientFunds);

        let successful_finance_result = treasurer.try_finance_purchase_for_fleet(fleet_id, 25_000.into())?;

        expected_ledger_entries.push(LedgerEntry::TransferredFundsFromTreasuryToFleet {
            fleet_id: fleet_id.clone(),
            credits: 15_000.into(),
        });

        assert_eq!(
            treasurer.get_fleet_budget(fleet_id)?,
            FleetBudget {
                current_capital: 25_000.into(),
                reserved_capital: 0.into(),
                budget: 10_000.into(),
                ..Default::default()
            }
        );
        assert_eq!(successful_finance_result, FinanceResult::TransferSuccessful { transfer_sum: 15_000.into() });
        assert_eq!(treasurer.current_treasury_fund()?, 150_000.into());
        assert_eq!(
            serde_json::to_string_pretty(&treasurer.ledger_entries()?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );

        let too_expensive_result = treasurer.try_finance_purchase_for_fleet(fleet_id, 200_000.into())?;

        assert_eq!(too_expensive_result, FinanceResult::TransferFailed { missing: 175_000.into() });

        //no ledger entry
        assert_eq!(
            serde_json::to_string_pretty(&treasurer.ledger_entries()?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );

        // unchanged
        assert_eq!(treasurer.current_treasury_fund()?, 150_000.into());

        // unchanged budget
        assert_eq!(
            treasurer.get_fleet_budget(fleet_id)?,
            FleetBudget {
                current_capital: 25_000.into(),
                reserved_capital: 0.into(),
                budget: 10_000.into(),
                ..Default::default()
            }
        );

        Ok(())
    }

    #[test]
    fn test_removing_fleets() -> Result<()> {
        let treasurer = ThreadSafeTreasurer::new(175_000.into());

        treasurer.create_fleet(&FleetId(1), Credits::new(75_000))?;
        treasurer.create_fleet(&FleetId(2), Credits::new(50_000))?;
        treasurer.transfer_funds_to_fleet_to_top_up_available_capital(&FleetId(1))?;
        treasurer.transfer_funds_to_fleet_to_top_up_available_capital(&FleetId(2))?;

        // we tested the ledger entries up to this point in a different test, so we assume they're correct
        let mut expected_ledger_entries = treasurer.ledger_entries()?.into_iter().collect_vec();
        assert_eq!(treasurer.current_treasury_fund()?, 50_000.into());

        treasurer.transfer_all_funds_to_treasury()?;
        expected_ledger_entries.push(TransferredFundsFromFleetToTreasury {
            fleet_id: FleetId(1),
            credits: 75_000.into(),
        });

        expected_ledger_entries.push(TransferredFundsFromFleetToTreasury {
            fleet_id: FleetId(2),
            credits: 50_000.into(),
        });

        assert_eq!(treasurer.current_treasury_fund()?, 175_000.into());
        assert_eq!(
            serde_json::to_string_pretty(&treasurer.ledger_entries()?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );

        Ok(())
    }

    #[test]
    fn test_archiving_fleet() -> Result<()> {
        let treasurer = ThreadSafeTreasurer::new(175_000.into());

        treasurer.create_fleet(&FleetId(1), Credits::new(75_000))?;

        // we tested the ledger entries up to this point in a different test, so we assume they're correct
        let mut expected_ledger_entries = treasurer.ledger_entries()?.into_iter().collect_vec();

        treasurer.remove_all_fleets()?;
        expected_ledger_entries.push(ArchivedFleetBudget {
            fleet_id: FleetId(1),
            budget: FleetBudget {
                current_capital: Default::default(),
                reserved_capital: Default::default(),
                budget: 75_000.into(),
                operating_reserve: Default::default(),
            },
        });

        assert_eq!(
            serde_json::to_string_pretty(&treasurer.ledger_entries()?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );

        Ok(())
    }

    #[test]
    fn test_archiving_fleet_with_capital() -> Result<()> {
        let treasurer = ThreadSafeTreasurer::new(175_000.into());

        treasurer.create_fleet(&FleetId(1), Credits::new(75_000))?;
        treasurer.transfer_funds_to_fleet_to_top_up_available_capital(&FleetId(1))?;

        // we tested the ledger entries up to this point in a different test, so we assume they're correct
        let mut expected_ledger_entries = treasurer.ledger_entries()?.into_iter().collect_vec();

        assert_eq!(treasurer.current_treasury_fund()?, 100_000.into());

        treasurer.remove_all_fleets()?;

        expected_ledger_entries.push(TransferredFundsFromFleetToTreasury {
            fleet_id: FleetId(1),
            credits: 75_000.into(),
        });

        expected_ledger_entries.push(ArchivedFleetBudget {
            fleet_id: FleetId(1),
            budget: FleetBudget {
                current_capital: Default::default(),
                reserved_capital: Default::default(),
                budget: 75_000.into(),
                operating_reserve: Default::default(),
            },
        });

        assert_eq!(treasurer.current_treasury_fund()?, 175_000.into());

        assert_eq!(
            serde_json::to_string_pretty(&treasurer.ledger_entries()?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );

        Ok(())
    }
}
