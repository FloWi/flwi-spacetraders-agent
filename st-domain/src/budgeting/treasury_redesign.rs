use crate::{serialize_as_sorted_map, ContractId};
use anyhow::anyhow;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
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
pub enum PurchaseCargoReason {
    Contract(ContractId),
    BoostSupplyChain,
    TradeProfitably,
    Construction,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Eq, Hash)]
pub struct PurchaseTradeGoodsTicketDetails {
    pub waypoint_symbol: WaypointSymbol,
    pub trade_good: TradeGoodSymbol,
    pub expected_price_per_unit: Credits,
    pub quantity: u32,
    pub expected_total_purchase_price: Credits,
    pub purchase_cargo_reason: Option<PurchaseCargoReason>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Eq, Hash)]
pub struct DeliverCargoContractTicketDetails {
    pub waypoint_symbol: WaypointSymbol,
    pub trade_good: TradeGoodSymbol,
    pub quantity: u32,
    pub contract_id: ContractId,
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
    DeliverContractCargo(DeliverCargoContractTicketDetails),
}

#[derive(Serialize, Deserialize, Debug, Clone, Display, PartialEq)]
pub enum FinanceTicketState {
    Open,
    Completed,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ActiveTrade {
    pub maybe_purchase: Option<(FinanceTicket, FinanceTicketState)>,
    pub delivery: FinanceTicket,
}

impl FinanceTicketDetails {
    pub fn signum(&self) -> i64 {
        match self {
            FinanceTicketDetails::PurchaseTradeGoods(PurchaseTradeGoodsTicketDetails { .. }) => -1,
            FinanceTicketDetails::SellTradeGoods(SellTradeGoodsTicketDetails { .. }) => 1,
            FinanceTicketDetails::PurchaseShip(PurchaseShipTicketDetails { .. }) => -1,
            FinanceTicketDetails::RefuelShip(RefuelShipTicketDetails { .. }) => -1,
            FinanceTicketDetails::SupplyConstructionSite(_) => 0,
            FinanceTicketDetails::DeliverContractCargo(_) => 0,
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
            FinanceTicketDetails::SupplyConstructionSite(d) => format!(
                "Delivering of {} units of {} for construction to {}",
                d.quantity, d.trade_good, d.waypoint_symbol
            ),
            FinanceTicketDetails::DeliverContractCargo(d) => format!(
                "Delivering of {} units of {} for contract {} to {}",
                d.quantity, d.trade_good, d.contract_id, d.waypoint_symbol
            ),
        }
    }

    pub fn get_units(&self) -> u32 {
        match self {
            FinanceTicketDetails::PurchaseTradeGoods(d) => d.quantity,
            FinanceTicketDetails::SellTradeGoods(d) => d.quantity,
            FinanceTicketDetails::PurchaseShip(_) => 1,
            FinanceTicketDetails::RefuelShip(d) => d.num_fuel_barrels,
            FinanceTicketDetails::SupplyConstructionSite(d) => d.quantity,
            FinanceTicketDetails::DeliverContractCargo(d) => d.quantity,
        }
    }

    pub fn get_price_per_unit(&self) -> Credits {
        match self {
            FinanceTicketDetails::PurchaseTradeGoods(d) => d.expected_price_per_unit,
            FinanceTicketDetails::SellTradeGoods(d) => d.expected_price_per_unit,
            FinanceTicketDetails::PurchaseShip(d) => d.expected_purchase_price,
            FinanceTicketDetails::RefuelShip(d) => d.expected_price_per_unit,
            FinanceTicketDetails::SupplyConstructionSite(_) => 0.into(),
            FinanceTicketDetails::DeliverContractCargo(_) => 0.into(),
        }
    }

    pub fn get_waypoint(&self) -> WaypointSymbol {
        match self {
            FinanceTicketDetails::PurchaseTradeGoods(d) => d.waypoint_symbol.clone(),
            FinanceTicketDetails::SellTradeGoods(d) => d.waypoint_symbol.clone(),
            FinanceTicketDetails::PurchaseShip(d) => d.waypoint_symbol.clone(),
            FinanceTicketDetails::RefuelShip(d) => d.waypoint_symbol.clone(),
            FinanceTicketDetails::SupplyConstructionSite(d) => d.waypoint_symbol.clone(),
            FinanceTicketDetails::DeliverContractCargo(d) => d.waypoint_symbol.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Income {
    ContractAccepted { contract_id: ContractId, accepted_reward: Credits },
    ContractFulfilled { contract_id: ContractId, fulfilled_reward: Credits },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TreasurerArchiveEntry {
    from_ledger_id: u64,
    to_ledger_id: u64,
    entry: ImprovedTreasurer,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LedgerArchiveEntry {
    id: u64,
    entry: LedgerEntry,
    created_at: DateTime<Utc>,
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
    IncomeLogged {
        fleet_id: FleetId,
        income: Income,
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
    TreasuryReset {
        credits: Credits,
    },
    BrokenTicketDeleted {
        fleet_id: FleetId,
        finance_ticket: FinanceTicket,
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
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use std::sync::Arc;
use strum::Display;
use tokio::sync::Mutex;

#[async_trait]
pub trait LedgerArchiver {
    async fn process_entry(&mut self, entry: LedgerEntry) -> Result<()>;
}

pub type LedgerArchiveTaskSender = tokio::sync::mpsc::UnboundedSender<LedgerArchiveTask>;

#[derive(Debug)]
pub struct LedgerArchiveTask {
    pub entry: LedgerEntry,
    pub response_sender: tokio::sync::mpsc::Sender<Result<()>>,
}

#[derive(Clone, Debug)]
pub struct ThreadSafeTreasurer {
    inner: Arc<Mutex<ImprovedTreasurer>>,
    task_sender: LedgerArchiveTaskSender,
}

impl ThreadSafeTreasurer {
    pub fn from_replayed_ledger_log(ledger_entries: Vec<LedgerEntry>, ledger_archiving_task_sender: LedgerArchiveTaskSender) -> Self {
        match ImprovedTreasurer::from_ledger(ledger_entries) {
            Ok(instance) => Self {
                inner: Arc::new(Mutex::new(instance)),
                task_sender: ledger_archiving_task_sender,
            },
            Err(err) => {
                panic!("Creating a new treasurer instance from replayed log should have worked. Error is: {err:?}")
            }
        }
    }

    pub async fn new(starting_credits: Credits, ledger_archiving_task_sender: LedgerArchiveTaskSender) -> Self
where {
        let new_instance = ImprovedTreasurer::new();
        let instance = Self {
            inner: Arc::new(Mutex::new(new_instance)),
            task_sender: ledger_archiving_task_sender,
        };

        match instance
            .with_treasurer(|t| t.process_ledger_entry(TreasuryCreated { credits: starting_credits }))
            .await
        {
            Ok(_) => instance,
            Err(err) => {
                panic!("Creating a new treasurer instance should have worked. Error is: {err:?}")
            }
        }
    }

    // compatible api for ShipAction
    pub async fn get_maybe_active_tickets_for_ship(&self, ship_symbol: &ShipSymbol) -> Result<Option<Vec<FinanceTicket>>> {
        let tickets = self.get_active_tickets_for_ship(ship_symbol).await?;

        if tickets.is_empty() {
            Ok(None)
        } else {
            Ok(Some(tickets))
        }
    }

    pub async fn get_active_tickets_for_ship(&self, ship_symbol: &ShipSymbol) -> Result<Vec<FinanceTicket>> {
        let active_tickets = self.get_active_tickets().await?;
        Ok(active_tickets
            .values()
            .filter(|t| &t.ship_symbol == ship_symbol)
            .cloned()
            .collect_vec())
    }

    // Helper method to execute operations on the treasurer
    async fn with_treasurer<F, R>(&self, operation: F) -> Result<R>
    where
        F: FnOnce(&mut ImprovedTreasurer) -> Result<R>,
    {
        let mut treasurer = self.inner.lock().await;

        let starting_idx = treasurer.ledger_entries.len();
        let result = operation(&mut treasurer);

        match result {
            Err(error) => Err(error),
            Ok(res) => {
                let ending_idx = treasurer.ledger_entries.len();

                // Process all new entries that were added
                if ending_idx > starting_idx {
                    let new_entries: Vec<LedgerEntry> = treasurer
                        .ledger_entries
                        .range(starting_idx..ending_idx)
                        .cloned()
                        .collect();

                    // Send all new entries for archiving
                    for entry in new_entries {
                        let (archiving_response_sender, mut archiving_response_receiver) = tokio::sync::mpsc::channel(1);

                        // println!("with_treasurer: sending task to task_sender");
                        // Send task to external processor
                        self.task_sender
                            .send(LedgerArchiveTask {
                                entry,
                                response_sender: archiving_response_sender,
                            })
                            .map_err(|_| anyhow!("Ledger processor disconnected"))?;
                        // println!("with_treasurer: sent task to task_sender, waiting for archiving");

                        // Wait for processing to complete
                        let _ = archiving_response_receiver
                            .recv()
                            .await
                            .ok_or_else(|| anyhow!("Failed to receive response from processor"))?;

                        // println!("with_treasurer: Got ACK back on sync channel");
                    }
                }

                Ok(res)
            }
        }
    }

    pub async fn remove_tickets_with_0_units(&self) -> Result<()> {
        self.with_treasurer(|t| t.remove_tickets_with_0_units())
            .await?;

        Ok(())
    }

    pub async fn reset_treasurer_due_to_agent_credit_diff(&self, starting_credits: Credits) -> Result<()> {
        self.remove_all_fleets().await?;

        self.with_treasurer(|t| t.process_ledger_entry(TreasuryReset { credits: starting_credits }))
            .await?;

        Ok(())
    }

    pub async fn get_instance(&self) -> Result<ImprovedTreasurer> {
        self.with_treasurer(|t| Ok(t.clone())).await
    }

    pub async fn get_fleet_budget(&self, fleet_id: &FleetId) -> Result<FleetBudget> {
        self.with_treasurer(|t| {
            t.fleet_budgets
                .get(fleet_id)
                .cloned()
                .ok_or(anyhow!("Fleet id not found"))
        })
        .await
    }

    pub async fn get_fleet_tickets(&self) -> Result<HashMap<FleetId, Vec<FinanceTicket>>> {
        self.with_treasurer(|t| t.get_fleet_tickets()).await
    }
    pub async fn get_fleet_budgets(&self) -> Result<HashMap<FleetId, FleetBudget>> {
        self.with_treasurer(|t| t.get_fleet_budgets()).await
    }
    pub async fn get_active_tickets(&self) -> Result<HashMap<TicketId, FinanceTicket>> {
        self.with_treasurer(|t| t.get_active_tickets()).await
    }

    pub async fn get_current_agent_credits(&self) -> Result<Credits> {
        self.with_treasurer(|t| Ok(t.current_agent_credits())).await
    }

    pub async fn get_current_treasury_fund(&self) -> Result<Credits> {
        self.with_treasurer(|t| Ok(t.current_treasury_fund())).await
    }

    pub async fn create_fleet(&self, fleet_id: &FleetId, total_capital: Credits) -> Result<()> {
        self.with_treasurer(|t| t.create_fleet(fleet_id, total_capital))
            .await
    }

    pub async fn transfer_funds_to_fleet_to_top_up_available_capital(&self, fleet_id: &FleetId) -> Result<()> {
        self.with_treasurer(|t| t.transfer_funds_to_fleet_to_top_up_available_capital(fleet_id))
            .await
    }

    pub async fn create_purchase_trade_goods_ticket(
        &self,
        fleet_id: &FleetId,
        trade_good_symbol: TradeGoodSymbol,
        waypoint_symbol: WaypointSymbol,
        ship_symbol: ShipSymbol,
        quantity: u32,
        expected_price_per_unit: Credits,
        maybe_purchase_cargo_reason: Option<PurchaseCargoReason>,
    ) -> Result<FinanceTicket> {
        self.with_treasurer(|t| {
            t.create_purchase_trade_goods_ticket(
                fleet_id,
                trade_good_symbol,
                waypoint_symbol,
                ship_symbol,
                quantity,
                expected_price_per_unit,
                maybe_purchase_cargo_reason,
            )
        })
        .await
    }

    pub async fn create_multiple_tickets(
        &self,
        ship_symbol: &ShipSymbol,
        fleet_id: &FleetId,
        tickets: Vec<FinanceTicketDetails>,
    ) -> Result<Vec<FinanceTicket>> {
        self.with_treasurer(|t| t.create_multiple_tickets(ship_symbol, fleet_id, tickets))
            .await
    }

    pub async fn create_sell_trade_goods_ticket(
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
        .await
    }

    pub async fn create_delivery_construction_material_ticket(
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
        .await
    }

    pub async fn report_income(&self, fleet_id: &FleetId, income: Income) -> Result<()> {
        self.with_treasurer(|t| t.report_income(fleet_id, income))
            .await
    }

    pub async fn report_expense(
        &self,
        fleet_id: &FleetId,
        current_destination: Option<WaypointSymbol>,
        current_tickets: Vec<FinanceTicket>,
        trade_good_symbol: TradeGoodSymbol,
        units: u32,
        price_per_unit: Credits,
    ) -> Result<()> {
        self.with_treasurer(|t| t.report_expense(fleet_id, current_destination, current_tickets, trade_good_symbol, units, price_per_unit))
            .await
    }

    pub async fn get_active_trade_routes(&self) -> Result<Vec<ActiveTradeRoute>> {
        self.with_treasurer(|t| t.get_active_trade_routes()).await
    }

    pub async fn create_ship_purchase_ticket_financed_from_global_treasury(
        &self,
        fleet_id: &FleetId,
        ship_type: ShipType,
        expected_purchase_price: Credits,
        shipyard_waypoint_symbol: WaypointSymbol,
        ship_symbol: ShipSymbol,
    ) -> Result<FinanceTicket> {
        self.with_treasurer(|t| t.create_ship_purchase_ticket(fleet_id, ship_type, expected_purchase_price, shipyard_waypoint_symbol, ship_symbol))
            .await
    }

    pub async fn get_ticket(&self, ticket_id: &TicketId) -> Result<FinanceTicket> {
        self.with_treasurer(|t| t.get_ticket(ticket_id)).await
    }

    pub async fn complete_ticket(&self, fleet_id: &FleetId, finance_ticket: &FinanceTicket, actual_price_per_unit: Credits) -> Result<()> {
        self.with_treasurer(|t| t.complete_ticket(fleet_id, finance_ticket, actual_price_per_unit))
            .await
    }

    pub async fn transfer_excess_funds_from_fleet_to_treasury(&self, fleet_id: &FleetId) -> Result<()> {
        self.with_treasurer(|t| t.transfer_excess_funds_from_fleet_to_treasury_if_necessary(fleet_id))
            .await
    }

    pub async fn set_fleet_budget(&self, fleet_id: &FleetId, new_total_capital: Credits) -> Result<()> {
        self.with_treasurer(|t| t.set_fleet_total_capital(fleet_id, new_total_capital))
            .await
    }

    pub async fn set_new_operating_reserve(&self, fleet_id: &FleetId, new_operating_reserve: Credits) -> Result<()> {
        self.with_treasurer(|t| t.set_new_operating_reserve(fleet_id, new_operating_reserve))
            .await
    }

    pub async fn get_ledger_entries(&self) -> Result<VecDeque<LedgerEntry>> {
        self.with_treasurer(|t| Ok(t.ledger_entries.clone())).await
    }

    pub async fn transfer_all_funds_to_treasury(&self) -> Result<()> {
        self.with_treasurer(|t| t.transfer_all_funds_to_treasury())
            .await
    }

    pub async fn remove_all_fleets(&self) -> Result<()> {
        self.with_treasurer(|t| t.remove_all_fleets()).await
    }

    pub async fn remove_fleet(&self, fleet_id: &FleetId) -> Result<()> {
        self.with_treasurer(|t| t.remove_fleet(fleet_id)).await
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ImprovedTreasurer {
    treasury_fund: Credits,

    // we keep it around for testing and debugging - can be removed at one point
    #[serde(skip)]
    ledger_entries: VecDeque<LedgerEntry>,

    #[serde(serialize_with = "serialize_as_sorted_map")]
    fleet_budgets: HashMap<FleetId, FleetBudget>,

    #[serde(serialize_with = "serialize_as_sorted_map")]
    active_tickets: HashMap<TicketId, FinanceTicket>,

    #[serde(serialize_with = "serialize_as_sorted_map")]
    completed_tickets: HashMap<TicketId, FinanceTicket>,
}

impl Default for ImprovedTreasurer {
    fn default() -> Self {
        Self::new()
    }
}

impl ImprovedTreasurer {
    pub fn new() -> Self {
        Self {
            treasury_fund: Default::default(),
            ledger_entries: Default::default(),
            fleet_budgets: Default::default(),
            active_tickets: Default::default(),
            completed_tickets: Default::default(),
        }
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

    pub fn compute_active_trades(&self) -> HashMap<ShipSymbol, Vec<ActiveTrade>> {
        let mut active_trades: HashMap<ShipSymbol, Vec<ActiveTrade>> = HashMap::new();
        // println!(
        //     "inside compute_active_trades()\nactive_tickets: {}",
        //     serde_json::to_string(&self.active_tickets).unwrap()
        // );

        let all_referenced_matching_purchase_tickets = self
            .active_tickets
            .values()
            .filter_map(|active_ticket| match &active_ticket.details {
                FinanceTicketDetails::SellTradeGoods(d) => d.maybe_matching_purchase_ticket,
                FinanceTicketDetails::SupplyConstructionSite(d) => d.maybe_matching_purchase_ticket,
                FinanceTicketDetails::PurchaseTradeGoods(_) => None,
                FinanceTicketDetails::PurchaseShip(_) => None,
                FinanceTicketDetails::RefuelShip(_) => None,
                FinanceTicketDetails::DeliverContractCargo(_) => None,
            })
            .collect::<HashSet<_>>();

        for active_ticket in self.active_tickets.values() {
            if all_referenced_matching_purchase_tickets.contains(&active_ticket.ticket_id) {
                // if this ticket is referenced from another ticket, we don't include it here as an orphaned ticket
                continue;
            }
            let maybe_matching_purchase_ticket_id = match &active_ticket.details {
                FinanceTicketDetails::SellTradeGoods(d) => d.maybe_matching_purchase_ticket,
                FinanceTicketDetails::SupplyConstructionSite(d) => d.maybe_matching_purchase_ticket,
                FinanceTicketDetails::PurchaseTradeGoods(_) => None,
                FinanceTicketDetails::PurchaseShip(_) => None,
                FinanceTicketDetails::RefuelShip(_) => None,
                FinanceTicketDetails::DeliverContractCargo(_) => None,
            };

            let maybe_matching_purchase_ticket = maybe_matching_purchase_ticket_id.and_then(|ticket_id| self.get_ticket_with_state(&ticket_id));

            let active_trades_of_ship = active_trades
                .entry(active_ticket.ship_symbol.clone())
                .or_default();

            active_trades_of_ship.push(ActiveTrade {
                maybe_purchase: maybe_matching_purchase_ticket,
                delivery: active_ticket.clone(),
            });
        }

        active_trades
    }

    fn create_fleet(&mut self, fleet_id: &FleetId, total_capital: Credits) -> Result<()> {
        self.process_ledger_entry(FleetCreated {
            fleet_id: fleet_id.clone(),
            total_capital,
        })?;

        Ok(())
    }

    fn reimburse_expense(&mut self, fleet_id: &FleetId, credits: Credits) -> Result<()> {
        if let Some(_fleet_budget) = self.fleet_budgets.get(fleet_id) {
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

    fn get_ticket_with_state(&self, ticket_id: &TicketId) -> Option<(FinanceTicket, FinanceTicketState)> {
        self.active_tickets
            .get(ticket_id)
            .map(|t| (t.clone(), FinanceTicketState::Open))
            .or_else(|| {
                self.completed_tickets
                    .get(ticket_id)
                    .map(|t| (t.clone(), FinanceTicketState::Open))
            })
    }

    fn get_active_trade_routes(&self) -> Result<Vec<ActiveTradeRoute>> {
        let mut active_routes = HashMap::new();

        // we have two types of delivery tickets - SellTradeGoods and SupplyConstructionSite
        for (waypoint_symbol, trade_good, maybe_matching_purchase_ticket) in self
            .active_tickets
            .values()
            .filter_map(|t| match &t.details {
                FinanceTicketDetails::SupplyConstructionSite(d) => Some((d.waypoint_symbol.clone(), d.trade_good.clone(), d.maybe_matching_purchase_ticket)),
                FinanceTicketDetails::SellTradeGoods(d) => Some((d.waypoint_symbol.clone(), d.trade_good.clone(), d.maybe_matching_purchase_ticket)),
                FinanceTicketDetails::PurchaseTradeGoods(_) => None,
                FinanceTicketDetails::PurchaseShip(_) => None,
                FinanceTicketDetails::RefuelShip(_) => None,
                FinanceTicketDetails::DeliverContractCargo(d) => Some((d.waypoint_symbol.clone(), d.trade_good.clone(), None)),
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

    pub(crate) fn remove_tickets_with_0_units(&mut self) -> Result<()> {
        let mut broken_tickets = Vec::new();
        for ticket in self.active_tickets.values() {
            match &ticket.details {
                FinanceTicketDetails::PurchaseTradeGoods(d) => {
                    if d.quantity == 0 {
                        broken_tickets.push(ticket.clone());
                    }
                }
                FinanceTicketDetails::SellTradeGoods(d) => {
                    if d.quantity == 0 {
                        broken_tickets.push(ticket.clone());
                    }
                }
                FinanceTicketDetails::SupplyConstructionSite(d) => {
                    if d.quantity == 0 {
                        broken_tickets.push(ticket.clone());
                    }
                }
                FinanceTicketDetails::PurchaseShip(_) => {}
                FinanceTicketDetails::RefuelShip(_) => {}
                FinanceTicketDetails::DeliverContractCargo(d) => {
                    if d.quantity == 0 {
                        broken_tickets.push(ticket.clone());
                    }
                }
            }
        }

        for ticket in broken_tickets {
            self.process_ledger_entry(LedgerEntry::BrokenTicketDeleted {
                fleet_id: ticket.fleet_id.clone(),
                finance_ticket: ticket.clone(),
            })?
        }

        Ok(())
    }

    pub fn create_multiple_tickets(
        &mut self,
        ship_symbol: &ShipSymbol,
        fleet_id: &FleetId,
        ticket_details: Vec<FinanceTicketDetails>,
    ) -> Result<Vec<FinanceTicket>> {
        if let Some(fleet_budget) = self.fleet_budgets.get(fleet_id) {
            let required_budget: Credits = ticket_details
                .iter()
                .map(|details| (details.get_price_per_unit() * details.get_units()).0)
                .sum::<i64>()
                .into();

            let available_capital = fleet_budget.available_capital();
            if available_capital < required_budget {
                Err(anyhow!(
                    "Insufficient budget for fleet #{}. Required: {}c, available: {}c",
                    fleet_id,
                    required_budget,
                    available_capital
                ))?
            } else {
                let mut tickets = Vec::new();
                for details in ticket_details {
                    let total = details.get_price_per_unit() * details.get_units();
                    let ticket = FinanceTicket {
                        ticket_id: Default::default(),
                        fleet_id: fleet_id.clone(),
                        ship_symbol: ship_symbol.clone(),
                        details,
                        allocated_credits: total,
                    };
                    self.process_ledger_entry(TicketCreated {
                        fleet_id: fleet_id.clone(),
                        ticket_details: ticket.clone(),
                    })?;
                    tickets.push(ticket);
                }
                Ok(tickets)
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
        maybe_purchase_cargo_reason: Option<PurchaseCargoReason>,
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
                    purchase_cargo_reason: maybe_purchase_cargo_reason,
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
        match self.try_finance_purchase_for_fleet(fleet_id, expected_purchase_price)? {
            FinanceResult::FleetAlreadyHadSufficientFunds => {}
            FinanceResult::TransferSuccessful { .. } => {}
            FinanceResult::TransferFailed { .. } => {
                anyhow::bail!("Transfer failed")
            }
        }

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

    pub fn report_income(&mut self, fleet_id: &FleetId, income: Income) -> Result<()> {
        self.process_ledger_entry(IncomeLogged {
            fleet_id: fleet_id.clone(),
            income,
        })?;

        Ok(())
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
            maybe_ticket_id: maybe_ticket.map(|t| t.ticket_id),
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
        let quantity: u32 = finance_ticket.details.get_units();

        let total = actual_price_per_unit * quantity * finance_ticket.details.signum();

        self.process_ledger_entry(TicketCompleted {
            fleet_id: fleet_id.clone(),
            finance_ticket: finance_ticket.clone(),
            actual_units: quantity,
            actual_price_per_unit,
            total,
        })?;

        let is_expense = finance_ticket.details.signum() < 0;

        if is_expense {
            let has_spent_more_than_allocated = finance_ticket.allocated_credits < total.abs();
            if has_spent_more_than_allocated {
                let diff = total.abs() - finance_ticket.allocated_credits;
                self.reimburse_expense(fleet_id, diff)?;
            } else {
                self.transfer_excess_funds_from_fleet_to_treasury_if_necessary(fleet_id)?;
            }
        } else {
            self.transfer_excess_funds_from_fleet_to_treasury_if_necessary(fleet_id)?;
        }

        Ok(())
    }

    fn transfer_excess_funds_from_fleet_to_treasury_if_necessary(&mut self, fleet_id: &FleetId) -> Result<()> {
        if let Some(budget) = self.fleet_budgets.get_mut(fleet_id) {
            let excess = budget.current_capital - budget.budget - budget.reserved_capital;
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
        if self.fleet_budgets.get_mut(fleet_id).is_some() {
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
        if let Some(_fleet_budget) = self.fleet_budgets.get_mut(fleet_id) {
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

        for (fleet_id, _budget) in self
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
    pub fn process_ledger_entry(&mut self, ledger_entry: LedgerEntry) -> Result<()> {
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
                    // we don't need to check the budget if we don't allocate any credits
                    if allocated_credits == 0.into() || budget.current_capital >= allocated_credits {
                        budget.reserved_capital += allocated_credits;
                        self.active_tickets
                            .insert(ticket_details.ticket_id, ticket_details);

                        self.ledger_entries.push_back(ledger_entry);
                    } else {
                        return Err(anyhow!(
                            "Insufficient funds for creating ticket {} for fleet #{}. available_capital: {}; allocated_credits: {}",
                            ticket_details.ticket_id,
                            fleet_id,
                            budget.current_capital,
                            allocated_credits
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
            TreasuryReset { credits } => {
                self.active_tickets.clear();
                self.completed_tickets.clear();
                self.treasury_fund = credits;
                self.ledger_entries.push_back(ledger_entry);
            }
            LedgerEntry::BrokenTicketDeleted { fleet_id, finance_ticket } => {
                if let Some(budget) = self.fleet_budgets.get_mut(&fleet_id) {
                    if finance_ticket.allocated_credits.is_positive() {
                        // clear the reservation
                        budget.reserved_capital -= finance_ticket.allocated_credits;
                    }

                    budget.current_capital += finance_ticket.allocated_credits;
                    self.active_tickets.remove(&finance_ticket.ticket_id);

                    self.ledger_entries.push_back(ledger_entry);
                } else {
                    return Err(anyhow!("Fleet {} doesn't exist", fleet_id));
                }
            }
            LedgerEntry::IncomeLogged { fleet_id, income } => {
                if let Some(budget) = self.fleet_budgets.get_mut(&fleet_id) {
                    match income {
                        Income::ContractAccepted { accepted_reward, .. } => {
                            budget.current_capital += accepted_reward;
                        }
                        Income::ContractFulfilled { fulfilled_reward, .. } => {
                            budget.current_capital += fulfilled_reward;
                        }
                    }
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
    use crate::budgeting::test_sync_ledger::create_test_ledger_setup;
    use crate::budgeting::treasury_redesign::LedgerEntry::{ArchivedFleetBudget, TransferredFundsFromFleetToTreasury, TransferredFundsFromTreasuryToFleet};
    use crate::budgeting::treasury_redesign::{
        ActiveTrade, ActiveTradeRoute, FleetBudget, ImprovedTreasurer, LedgerArchiveEntry, LedgerEntry, PurchaseCargoReason, ThreadSafeTreasurer,
        TreasurerArchiveEntry,
    };
    use crate::{FleetId, ShipSymbol, ShipType, TradeGoodSymbol, WaypointSymbol};
    use anyhow::Result;
    use itertools::Itertools;
    use std::collections::HashMap;

    use tokio::test;

    #[test]
    async fn test_computing_active_trades_from_ledger_entries_should_produce_no_duplicate_entries() -> Result<()> {
        let json_str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/active_trades_from_ledger_entries_creates_duplicates.json"
        ));

        let ledger_entries = serde_json::from_str::<Vec<LedgerEntry>>(json_str)?;
        let treasurer = ImprovedTreasurer::from_ledger(ledger_entries)?;
        let active_trades: HashMap<ShipSymbol, Vec<ActiveTrade>> = treasurer.compute_active_trades();

        let trades_in_question = active_trades
            .get(&ShipSymbol("FLWI_2_TEST-1".to_string()))
            .unwrap();

        let ticket_ids = trades_in_question
            .iter()
            .flat_map(|trade| {
                let this_ticket_id = vec![trade.delivery.ticket_id];
                let maybe_purchase_ticket_id = trade
                    .maybe_purchase
                    .clone()
                    .map(|p| vec![p.0.ticket_id])
                    .unwrap_or_default();
                this_ticket_id
                    .into_iter()
                    .chain(maybe_purchase_ticket_id.into_iter())
            })
            .collect_vec();

        let duplicates = ticket_ids.iter().duplicates().cloned().collect_vec();

        println!("{}", serde_json::to_string(&trades_in_question)?);

        assert_eq!(duplicates, vec![]);

        Ok(())
    }

    #[test]
    async fn test_fleet_budget_in_trade_cycle() -> Result<()> {
        //Start Fresh with 175k

        let (test_archiver, task_sender) = create_test_ledger_setup().await;

        let treasurer = ThreadSafeTreasurer::new(175_000.into(), task_sender.clone()).await;
        let mut expected_ledger_entries = vec![LedgerEntry::TreasuryCreated { credits: 175_000.into() }];

        assert_eq!(treasurer.get_current_agent_credits().await?, Credits::new(175_000));
        assert_eq!(
            serde_json::to_string_pretty(&treasurer.get_ledger_entries().await?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );

        // Create fleet with 75k total budget

        let fleet_id = &FleetId(1);
        let ship_symbol = &ShipSymbol("FLWI-1".to_string());

        treasurer
            .create_fleet(fleet_id, Credits::new(75_000))
            .await?;
        expected_ledger_entries.push(LedgerEntry::FleetCreated {
            fleet_id: FleetId(1),
            total_capital: 75_000.into(),
        });

        assert_eq!(
            serde_json::to_string_pretty(&treasurer.get_ledger_entries().await?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );
        assert_eq!(treasurer.get_current_agent_credits().await?, Credits::new(175_000));

        assert_eq!(treasurer.get_fleet_budget(fleet_id).await?.current_capital, Credits::new(0));

        // transfer 75k from treasurer to fleet budget

        treasurer
            .transfer_funds_to_fleet_to_top_up_available_capital(fleet_id)
            .await?;

        expected_ledger_entries.push(LedgerEntry::TransferredFundsFromTreasuryToFleet {
            fleet_id: FleetId(1),
            credits: 75_000.into(),
        });

        assert_eq!(
            serde_json::to_string_pretty(&treasurer.get_ledger_entries().await?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );
        assert_eq!(treasurer.get_current_agent_credits().await?, Credits::new(175_000));
        assert_eq!(treasurer.get_fleet_budget(fleet_id).await?.current_capital, Credits::new(75_000));

        // create purchase ticket (reduces available capital of fleet)

        let purchase_ticket = treasurer
            .create_purchase_trade_goods_ticket(
                fleet_id,
                TradeGoodSymbol::ADVANCED_CIRCUITRY,
                WaypointSymbol("FROM".to_string()),
                ship_symbol.clone(),
                40,
                Credits(1_000.into()),
                Some(PurchaseCargoReason::TradeProfitably),
            )
            .await?;

        assert_eq!(purchase_ticket.allocated_credits, 40_000.into());
        assert_eq!(treasurer.get_ticket(&purchase_ticket.ticket_id).await?, purchase_ticket);
        expected_ledger_entries.push(LedgerEntry::TicketCreated {
            fleet_id: FleetId(1),
            ticket_details: purchase_ticket.clone(),
        });

        assert_eq!(
            treasurer.get_fleet_budget(fleet_id).await?,
            FleetBudget {
                current_capital: 75_000.into(),
                reserved_capital: 40_000.into(),
                budget: 75_000.into(),
                ..Default::default()
            }
        );
        assert_eq!(
            serde_json::to_string_pretty(&treasurer.get_ledger_entries().await?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );
        assert_eq!(treasurer.get_current_agent_credits().await?, Credits::new(175_000));

        // create sell ticket

        let sell_ticket = treasurer
            .create_sell_trade_goods_ticket(
                fleet_id,
                TradeGoodSymbol::ADVANCED_CIRCUITRY,
                WaypointSymbol("TO".to_string()),
                ship_symbol.clone(),
                40,
                Credits(2_000.into()),
                Some(purchase_ticket.ticket_id),
            )
            .await?;

        assert_eq!(treasurer.get_ticket(&sell_ticket.ticket_id).await?, sell_ticket);

        assert_eq!(sell_ticket.allocated_credits, 0.into());

        expected_ledger_entries.push(LedgerEntry::TicketCreated {
            fleet_id: FleetId(1),
            ticket_details: sell_ticket.clone(),
        });

        assert_eq!(
            treasurer.get_fleet_budget(fleet_id).await?,
            FleetBudget {
                current_capital: 75_000.into(),
                reserved_capital: 40_000.into(),
                budget: 75_000.into(),
                ..Default::default()
            }
        );

        assert_eq!(
            serde_json::to_string_pretty(&treasurer.get_ledger_entries().await?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );
        assert_eq!(treasurer.get_current_agent_credits().await?, Credits::new(175_000));

        // perform purchase (we spent less than expected)
        let purchase_price_per_unit = 900.into(); // a little less than expected
        treasurer
            .complete_ticket(fleet_id, &purchase_ticket, purchase_price_per_unit)
            .await?;

        assert!(treasurer
            .get_ticket(&purchase_ticket.ticket_id)
            .await
            .is_err());

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
            treasurer.get_fleet_budget(fleet_id).await?,
            FleetBudget {
                current_capital: 39_000.into(), //75 - 36
                reserved_capital: 0.into(),     // we clear the reservation
                budget: 75_000.into(),
                ..Default::default()
            }
        );

        assert_eq!(
            serde_json::to_string_pretty(&treasurer.get_ledger_entries().await?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );

        assert_eq!(treasurer.get_current_agent_credits().await?, Credits::new(139_000));

        let sell_price_per_unit = 2_100.into(); // a little more than expected
        treasurer
            .complete_ticket(fleet_id, &sell_ticket, sell_price_per_unit)
            .await?;
        assert!(treasurer.get_ticket(&sell_ticket.ticket_id).await.is_err());

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
            treasurer.get_fleet_budget(fleet_id).await?,
            FleetBudget {
                current_capital: 75_000.into(),
                reserved_capital: 0.into(),
                budget: 75_000.into(),
                ..Default::default()
            }
        );
        assert_eq!(treasurer.get_current_agent_credits().await?, Credits::new(223_000));
        assert_eq!(
            serde_json::to_string_pretty(&treasurer.get_ledger_entries().await?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );

        let current_treasurer = treasurer.get_instance().await?;

        let actual_replayed_treasurer = ImprovedTreasurer::from_ledger(expected_ledger_entries)?;

        assert_eq!(
            serde_json::to_string_pretty(&actual_replayed_treasurer)?,
            serde_json::to_string_pretty(&current_treasurer)?
        );

        Ok(())
    }

    #[test]
    async fn test_fleet_budget_for_ship_purchases() -> Result<()> {
        //Start Fresh with 175k
        let (test_archiver, task_sender) = create_test_ledger_setup().await;

        let treasurer = ThreadSafeTreasurer::new(175_000.into(), task_sender.clone()).await;

        let fleet_id = &FleetId(1);
        let ship_symbol = &ShipSymbol("FLWI-1".to_string());

        treasurer
            .create_fleet(fleet_id, Credits::new(75_000))
            .await?;

        treasurer
            .transfer_funds_to_fleet_to_top_up_available_capital(fleet_id)
            .await?;

        // we tested the ledger entries up to this point in a different test, so we assume they're correct
        let mut expected_ledger_entries = treasurer
            .get_ledger_entries()
            .await?
            .into_iter()
            .collect_vec();

        let ship_purchase_ticket = treasurer
            .create_ship_purchase_ticket_financed_from_global_treasury(
                fleet_id,
                ShipType::SHIP_PROBE,
                25_000.into(),
                WaypointSymbol("FROM".to_string()),
                ship_symbol.clone(),
            )
            .await?;

        assert_eq!(ship_purchase_ticket.allocated_credits, 25_000.into());

        assert_eq!(
            treasurer.get_fleet_budget(fleet_id).await?,
            FleetBudget {
                current_capital: 100_000.into(),
                reserved_capital: 25_000.into(),
                budget: 75_000.into(),
                ..Default::default()
            }
        );

        expected_ledger_entries.push(LedgerEntry::TransferredFundsFromTreasuryToFleet {
            fleet_id: FleetId(1),
            credits: 25_000.into(),
        });

        expected_ledger_entries.push(LedgerEntry::TicketCreated {
            fleet_id: FleetId(1),
            ticket_details: ship_purchase_ticket.clone(),
        });

        assert_eq!(treasurer.get_current_agent_credits().await?, Credits::new(175_000));

        assert_eq!(
            serde_json::to_string_pretty(&treasurer.get_ledger_entries().await?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );

        treasurer
            .complete_ticket(fleet_id, &ship_purchase_ticket, 22_500.into())
            .await?; // cheaper than expected

        expected_ledger_entries.push(LedgerEntry::TicketCompleted {
            fleet_id: FleetId(1),
            finance_ticket: ship_purchase_ticket.clone(),
            actual_units: 1,
            actual_price_per_unit: 22_500.into(),
            total: (-22_500).into(),
        });

        expected_ledger_entries.push(LedgerEntry::TransferredFundsFromFleetToTreasury {
            fleet_id: FleetId(1),
            credits: 2_500.into(), // the amount we reserved too much
        });

        assert_eq!(treasurer.get_current_agent_credits().await?, Credits::new(152_500));
        assert_eq!(
            treasurer.get_fleet_budget(fleet_id).await?,
            FleetBudget {
                current_capital: 75_000.into(),
                reserved_capital: 0.into(),
                budget: 75_000.into(),
                ..Default::default()
            }
        );

        assert_eq!(
            serde_json::to_string_pretty(&treasurer.get_ledger_entries().await?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );

        assert_eq!(
            serde_json::to_string_pretty(&test_archiver.get_entries())?,
            serde_json::to_string_pretty(&treasurer.get_ledger_entries().await?)?
        );

        Ok(())
    }

    #[test]
    async fn test_set_fleet_total_capital() -> Result<()> {
        let (test_archiver, task_sender) = create_test_ledger_setup().await;

        let treasurer = ThreadSafeTreasurer::new(175_000.into(), task_sender.clone()).await;

        let fleet_id = &FleetId(1);

        treasurer
            .create_fleet(fleet_id, Credits::new(75_000))
            .await?;
        treasurer
            .transfer_funds_to_fleet_to_top_up_available_capital(&FleetId(1))
            .await?;

        assert_eq!(treasurer.get_current_agent_credits().await?, Credits::new(175_000));
        assert_eq!(
            treasurer.get_fleet_budget(fleet_id).await?,
            FleetBudget {
                current_capital: 75_000.into(),
                reserved_capital: 0.into(),
                budget: 75_000.into(),
                ..Default::default()
            }
        );

        // we tested the ledger entries up to this point in a different test, so we assume they're correct
        let mut expected_ledger_entries = treasurer
            .get_ledger_entries()
            .await?
            .into_iter()
            .collect_vec();

        treasurer.set_fleet_budget(fleet_id, 150_000.into()).await?;

        assert_eq!(treasurer.get_current_agent_credits().await?, Credits::new(175_000));
        assert_eq!(
            treasurer.get_fleet_budget(fleet_id).await?,
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
            serde_json::to_string_pretty(&treasurer.get_ledger_entries().await?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );
        //setting total capital below current_capital
        treasurer.set_fleet_budget(fleet_id, 50_000.into()).await?;

        // this will produce two entries in the ledger - one for the set-action and another one for the transfer of funds
        expected_ledger_entries.push(LedgerEntry::SetNewTotalCapitalForFleet {
            fleet_id: fleet_id.clone(),
            new_total_capital: 50_000.into(),
        });

        expected_ledger_entries.push(TransferredFundsFromFleetToTreasury {
            fleet_id: fleet_id.clone(),
            credits: 25_000.into(),
        });

        assert_eq!(treasurer.get_current_agent_credits().await?, Credits::new(175_000));
        assert_eq!(
            treasurer.get_fleet_budget(fleet_id).await?,
            FleetBudget {
                current_capital: 50_000.into(),
                reserved_capital: 0.into(),
                budget: 50_000.into(),
                ..Default::default()
            }
        );

        assert_eq!(treasurer.get_current_agent_credits().await?, Credits::new(175_000));
        assert_eq!(treasurer.get_current_treasury_fund().await?, Credits::new(125_000));

        assert_eq!(
            serde_json::to_string_pretty(&treasurer.get_ledger_entries().await?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );

        assert_eq!(
            serde_json::to_string_pretty(&test_archiver.get_entries())?,
            serde_json::to_string_pretty(&treasurer.get_ledger_entries().await?)?
        );

        Ok(())
    }

    #[test]
    async fn test_getting_active_trade_routes() -> Result<()> {
        let (test_archiver, task_sender) = create_test_ledger_setup().await;

        let treasurer = ThreadSafeTreasurer::new(175_000.into(), task_sender.clone()).await;

        let fleet_id = &FleetId(1);
        let ship_symbol = &ShipSymbol("FLWI-1".to_string());

        treasurer
            .create_fleet(fleet_id, Credits::new(75_000))
            .await?;

        treasurer
            .transfer_funds_to_fleet_to_top_up_available_capital(fleet_id)
            .await?;

        let trade_good = TradeGoodSymbol::ADVANCED_CIRCUITRY;
        let from_wps = WaypointSymbol("FROM".to_string());
        let to_wps = WaypointSymbol("TO".to_string());
        let completed_purchase_ticket_1 = treasurer
            .create_purchase_trade_goods_ticket(
                fleet_id,
                trade_good.clone(),
                from_wps.clone(),
                ship_symbol.clone(),
                40,
                Credits(1_000.into()),
                Some(PurchaseCargoReason::TradeProfitably),
            )
            .await?;

        treasurer
            .complete_ticket(fleet_id, &completed_purchase_ticket_1, 1_000.into())
            .await?;

        let purchase_ticket_2 = treasurer
            .create_purchase_trade_goods_ticket(
                fleet_id,
                trade_good.clone(),
                from_wps.clone(),
                ship_symbol.clone(),
                40,
                Credits(1_000.into()),
                Some(PurchaseCargoReason::TradeProfitably),
            )
            .await?;

        let _sell_ticket_1 = treasurer
            .create_sell_trade_goods_ticket(
                fleet_id,
                trade_good.clone(),
                to_wps.clone(),
                ship_symbol.clone(),
                40,
                Credits(2_000.into()),
                Some(completed_purchase_ticket_1.ticket_id),
            )
            .await?;

        let _sell_ticket_2 = treasurer
            .create_sell_trade_goods_ticket(
                fleet_id,
                trade_good.clone(),
                to_wps.clone(),
                ship_symbol.clone(),
                40,
                Credits(2_000.into()),
                Some(purchase_ticket_2.ticket_id),
            )
            .await?;

        let _unrelated_sell_ticket = treasurer
            .create_sell_trade_goods_ticket(
                fleet_id,
                trade_good.clone(),
                to_wps.clone(),
                ship_symbol.clone(),
                20,
                Credits(1_234.into()),
                None,
            )
            .await?;

        assert_eq!(
            treasurer.get_active_trade_routes().await?,
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
    async fn test_financing_of_ship_purchase() -> Result<()> {
        let (test_archiver, task_sender) = create_test_ledger_setup().await;

        let treasurer = ThreadSafeTreasurer::new(175_000.into(), task_sender.clone()).await;

        let fleet_id = &FleetId(1);
        let ship_symbol = &ShipSymbol("FLWI-1".to_string());

        treasurer
            .create_fleet(fleet_id, Credits::new(10_000))
            .await?;

        treasurer
            .transfer_funds_to_fleet_to_top_up_available_capital(fleet_id)
            .await?;

        // we tested the ledger entries up to this point in a different test, so we assume they're correct
        let mut expected_ledger_entries = treasurer
            .get_ledger_entries()
            .await?
            .into_iter()
            .collect_vec();

        let ship_purchase_ticket = treasurer
            .create_ship_purchase_ticket_financed_from_global_treasury(
                fleet_id,
                ShipType::SHIP_PROBE,
                25_000.into(),
                WaypointSymbol("FROM".to_string()),
                ship_symbol.clone(),
            )
            .await?;

        expected_ledger_entries.push(LedgerEntry::TransferredFundsFromTreasuryToFleet {
            fleet_id: fleet_id.clone(),
            credits: 25_000.into(),
        });

        Ok(())
    }

    #[test]
    async fn test_removing_fleets() -> Result<()> {
        let (test_archiver, task_sender) = create_test_ledger_setup().await;

        let treasurer = ThreadSafeTreasurer::new(175_000.into(), task_sender.clone()).await;

        treasurer
            .create_fleet(&FleetId(1), Credits::new(75_000))
            .await?;
        treasurer
            .create_fleet(&FleetId(2), Credits::new(50_000))
            .await?;
        treasurer
            .transfer_funds_to_fleet_to_top_up_available_capital(&FleetId(1))
            .await?;
        treasurer
            .transfer_funds_to_fleet_to_top_up_available_capital(&FleetId(2))
            .await?;

        // we tested the ledger entries up to this point in a different test, so we assume they're correct
        let mut expected_ledger_entries = treasurer
            .get_ledger_entries()
            .await?
            .into_iter()
            .collect_vec();
        assert_eq!(treasurer.get_current_treasury_fund().await?, 50_000.into());

        treasurer.transfer_all_funds_to_treasury().await?;

        expected_ledger_entries.push(TransferredFundsFromFleetToTreasury {
            fleet_id: FleetId(1),
            credits: 75_000.into(),
        });

        expected_ledger_entries.push(TransferredFundsFromFleetToTreasury {
            fleet_id: FleetId(2),
            credits: 50_000.into(),
        });

        assert_eq!(treasurer.get_current_treasury_fund().await?, 175_000.into());
        assert_eq!(
            serde_json::to_string_pretty(&treasurer.get_ledger_entries().await?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );

        assert_eq!(
            serde_json::to_string_pretty(&test_archiver.get_entries())?,
            serde_json::to_string_pretty(&treasurer.get_ledger_entries().await?)?
        );

        Ok(())
    }

    #[test]
    async fn test_archiving_fleet() -> Result<()> {
        println!("test: call create_test_ledger_setup()");
        let (test_archiver, task_sender) = create_test_ledger_setup().await;
        println!("test: called create_test_ledger_setup()");

        println!("test: Creating ThreadSafeTreasurer");
        let treasurer = ThreadSafeTreasurer::new(175_000.into(), task_sender.clone()).await;
        println!("test: Created ThreadSafeTreasurer");

        treasurer
            .create_fleet(&FleetId(1), Credits::new(75_000))
            .await?;

        // we tested the ledger entries up to this point in a different test, so we assume they're correct
        let mut expected_ledger_entries = treasurer
            .get_ledger_entries()
            .await?
            .into_iter()
            .collect_vec();

        treasurer.remove_all_fleets().await?;
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
            serde_json::to_string_pretty(&treasurer.get_ledger_entries().await?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );

        assert_eq!(
            serde_json::to_string_pretty(&test_archiver.get_entries())?,
            serde_json::to_string_pretty(&treasurer.get_ledger_entries().await?)?
        );

        Ok(())
    }

    #[test]
    async fn test_archiving_fleet_with_capital() -> Result<()> {
        let (test_archiver, task_sender) = create_test_ledger_setup().await;

        let treasurer = ThreadSafeTreasurer::new(175_000.into(), task_sender.clone()).await;

        treasurer
            .create_fleet(&FleetId(1), Credits::new(75_000))
            .await?;
        treasurer
            .transfer_funds_to_fleet_to_top_up_available_capital(&FleetId(1))
            .await?;

        // we tested the ledger entries up to this point in a different test, so we assume they're correct
        let mut expected_ledger_entries = treasurer
            .get_ledger_entries()
            .await?
            .into_iter()
            .collect_vec();

        assert_eq!(treasurer.get_current_treasury_fund().await?, 100_000.into());

        treasurer.remove_all_fleets().await?;

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

        assert_eq!(treasurer.get_current_treasury_fund().await?, 175_000.into());

        assert_eq!(
            serde_json::to_string_pretty(&treasurer.get_ledger_entries().await?)?,
            serde_json::to_string_pretty(&expected_ledger_entries)?
        );

        assert_eq!(
            serde_json::to_string_pretty(&test_archiver.get_entries())?,
            serde_json::to_string_pretty(&treasurer.get_ledger_entries().await?)?
        );

        Ok(())
    }

    #[test]
    async fn test_completing_ticket_while_ship_purchase_is_on_the_way() -> Result<()> {
        let (test_archiver, task_sender) = create_test_ledger_setup().await;

        let treasurer = ThreadSafeTreasurer::new(175_000.into(), task_sender.clone()).await;

        treasurer
            .create_fleet(&FleetId(1), Credits::new(75_000))
            .await?;

        treasurer
            .transfer_funds_to_fleet_to_top_up_available_capital(&FleetId(1))
            .await?;

        let fleet_id = &FleetId(1);
        let ship_symbol = &ShipSymbol("FLWI-1".to_string());

        let ship_purchase_ticket = treasurer
            .create_ship_purchase_ticket_financed_from_global_treasury(
                fleet_id,
                ShipType::SHIP_PROBE,
                25_000.into(),
                WaypointSymbol("FROM".to_string()),
                ship_symbol.clone(),
            )
            .await?;

        let purchase_ticket = treasurer
            .create_purchase_trade_goods_ticket(
                fleet_id,
                TradeGoodSymbol::ADVANCED_CIRCUITRY,
                WaypointSymbol("FROM".to_string()),
                ship_symbol.clone(),
                40,
                Credits(1_000.into()),
                Some(PurchaseCargoReason::TradeProfitably),
            )
            .await?;

        assert_eq!(treasurer.get_current_agent_credits().await?, Credits::new(175_000));

        // create sell ticket

        let sell_ticket = treasurer
            .create_sell_trade_goods_ticket(
                fleet_id,
                TradeGoodSymbol::ADVANCED_CIRCUITRY,
                WaypointSymbol("TO".to_string()),
                ship_symbol.clone(),
                40,
                Credits(2_000.into()),
                Some(purchase_ticket.ticket_id),
            )
            .await?;

        // we tested the ledger entries up to this point in a different test, so we assume they're correct
        let expected_ledger_entries = treasurer
            .get_ledger_entries()
            .await?
            .into_iter()
            .collect_vec();

        let budget_after_all_tickets = treasurer.get_fleet_budget(fleet_id).await?;

        assert_eq!(budget_after_all_tickets.current_capital, 100_000.into());
        assert_eq!(budget_after_all_tickets.reserved_capital, 65_000.into()); // 40k for goods, 25k for ship

        // complete purchase as expected
        treasurer
            .complete_ticket(fleet_id, &purchase_ticket, 1_000.into())
            .await?;

        let budget_after_trade_purchase = treasurer.get_fleet_budget(fleet_id).await?;

        assert_eq!(budget_after_trade_purchase.current_capital, 60_000.into()); // -40k for the purchase
        assert_eq!(budget_after_trade_purchase.reserved_capital, 25_000.into()); // only 25k for ship

        // complete sell as expected
        treasurer
            .complete_ticket(fleet_id, &sell_ticket, 2_000.into())
            .await?;

        let budget_after_trade_sell = treasurer.get_fleet_budget(fleet_id).await?;

        assert_eq!(budget_after_trade_sell.current_capital, 100_000.into()); // +80k for the sell, but we're giving 40k away.
        assert_eq!(budget_after_trade_sell.reserved_capital, 25_000.into()); // only 25k for ship

        Ok(())
    }

    #[test]
    async fn test_purchasing_ship_for_more_money_than_allocated() -> Result<()> {
        //treasurer should reimburse the overpaid credits immediately
        let (test_archiver, task_sender) = create_test_ledger_setup().await;

        let treasurer = ThreadSafeTreasurer::new(175_000.into(), task_sender.clone()).await;

        treasurer.create_fleet(&FleetId(1), Credits::new(0)).await?;

        let market_observation_fleet_id = &FleetId(1);
        let ship_symbol = &ShipSymbol("FLWI-1".to_string());

        let ship_purchase_ticket = treasurer
            .create_ship_purchase_ticket_financed_from_global_treasury(
                market_observation_fleet_id,
                ShipType::SHIP_PROBE,
                25_000.into(),
                WaypointSymbol("FROM".to_string()),
                ship_symbol.clone(),
            )
            .await?;

        assert_eq!(
            treasurer
                .get_fleet_budget(market_observation_fleet_id)
                .await?
                .current_capital,
            Credits::new(25_000)
        );

        treasurer
            .complete_ticket(market_observation_fleet_id, &ship_purchase_ticket, 26_000.into())
            .await?;

        assert_eq!(
            treasurer
                .get_fleet_budget(market_observation_fleet_id)
                .await?
                .current_capital,
            Credits::new(0)
        );

        assert!(treasurer
            .get_ledger_entries()
            .await?
            .contains(&TransferredFundsFromTreasuryToFleet {
                fleet_id: market_observation_fleet_id.clone(),
                credits: 1_000.into()
            }));

        Ok(())
    }

    #[test]
    async fn test_purchasing_ship_for_less_money_than_allocated() -> Result<()> {
        //treasurer should reimburse the overpaid credits immediately
        let (test_archiver, task_sender) = create_test_ledger_setup().await;

        let treasurer = ThreadSafeTreasurer::new(175_000.into(), task_sender.clone()).await;

        treasurer.create_fleet(&FleetId(1), Credits::new(0)).await?;

        let market_observation_fleet_id = &FleetId(1);
        let ship_symbol = &ShipSymbol("FLWI-1".to_string());

        let ship_purchase_ticket = treasurer
            .create_ship_purchase_ticket_financed_from_global_treasury(
                market_observation_fleet_id,
                ShipType::SHIP_PROBE,
                25_000.into(),
                WaypointSymbol("FROM".to_string()),
                ship_symbol.clone(),
            )
            .await?;

        assert_eq!(
            treasurer
                .get_fleet_budget(market_observation_fleet_id)
                .await?
                .current_capital,
            Credits::new(25_000)
        );

        treasurer
            .complete_ticket(market_observation_fleet_id, &ship_purchase_ticket, 24_000.into())
            .await?;

        assert_eq!(
            treasurer
                .get_fleet_budget(market_observation_fleet_id)
                .await?
                .current_capital,
            Credits::new(0)
        );

        assert!(treasurer
            .get_ledger_entries()
            .await?
            .contains(&TransferredFundsFromFleetToTreasury {
                fleet_id: market_observation_fleet_id.clone(),
                credits: 1_000.into()
            }));

        Ok(())
    }

    #[test]
    async fn test_from_ledger_entries() -> Result<()> {
        let ledger_entries_str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/treasurer_test_ledger_data.json"));
        let ledger_entries = serde_json::from_str::<Vec<LedgerEntry>>(ledger_entries_str)?;

        let mut treasurer = ImprovedTreasurer::new();

        let construction_fleet_id = FleetId(2);
        for entry in ledger_entries {
            let before = treasurer.fleet_budgets.get(&construction_fleet_id).cloned();
            treasurer.process_ledger_entry(entry.clone())?;
            let after = treasurer.fleet_budgets.get(&construction_fleet_id).cloned();

            if before.is_some() || after.is_some() {
                println!("\n========================================================================================");
                println!("\nledger_entry: {}", serde_json::to_string(&entry)?);
                println!("\nbefore fleet budget: {}", serde_json::to_string(&before)?);
                println!(
                    "before_available_capital: {}",
                    before
                        .map(|b| b.available_capital().to_string())
                        .unwrap_or("---".to_string())
                );

                println!("\nafter fleet budget: {}", serde_json::to_string(&after)?);
                println!(
                    "after_available_capital: {}",
                    after
                        .map(|b| b.available_capital().to_string())
                        .unwrap_or("---".to_string())
                );
            }
        }

        Ok(())
    }

    #[test]
    async fn test_2_from_ledger_entries() -> Result<()> {
        let ledger_entries_str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/treasurer_test_ledger_data_failed_completion_of_ship_purchase_after_restart_of_treasurer.json"
        ));
        let ledger_entries = serde_json::from_str::<Vec<LedgerEntry>>(ledger_entries_str)?;

        let mut treasurer = ImprovedTreasurer::new();

        let construction_fleet_id = FleetId(2);
        for entry in ledger_entries {
            let before = treasurer.fleet_budgets.get(&construction_fleet_id).cloned();
            treasurer.process_ledger_entry(entry.clone())?;
            let after = treasurer.fleet_budgets.get(&construction_fleet_id).cloned();

            if before.is_some() || after.is_some() {
                println!("\n========================================================================================");
                println!("\nledger_entry: {}", serde_json::to_string(&entry)?);
                println!("\nbefore fleet budget: {}", serde_json::to_string(&before)?);
                println!(
                    "before_available_capital: {}",
                    before
                        .map(|b| b.available_capital().to_string())
                        .unwrap_or("---".to_string())
                );

                println!("\nafter fleet budget: {}", serde_json::to_string(&after)?);
                println!(
                    "after_available_capital: {}",
                    after
                        .map(|b| b.available_capital().to_string())
                        .unwrap_or("---".to_string())
                );
            }
        }

        Ok(())
    }

    #[test]
    async fn test_3_from_ledger_entries() -> Result<()> {
        let ledger_entries_str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/latest_ledger_entries_for_testing.json"));

        let ledger_entries = serde_json::from_str::<Vec<LedgerEntry>>(ledger_entries_str)?;

        let mut treasurer = ImprovedTreasurer::new();

        let market_observation_fleet = FleetId(1);
        for entry in ledger_entries {
            let before = treasurer
                .fleet_budgets
                .get(&market_observation_fleet)
                .cloned();
            treasurer.process_ledger_entry(entry.clone())?;
            let after = treasurer
                .fleet_budgets
                .get(&market_observation_fleet)
                .cloned();

            if before.is_some() || after.is_some() {
                println!("\n========================================================================================");
                println!("\nledger_entry: {}", serde_json::to_string(&entry)?);
                println!("\nbefore fleet budget: {}", serde_json::to_string(&before)?);
                println!(
                    "before_available_capital: {}",
                    before
                        .map(|b| b.available_capital().to_string())
                        .unwrap_or("---".to_string())
                );

                println!("\nafter fleet budget: {}", serde_json::to_string(&after)?);
                println!(
                    "after_available_capital: {}",
                    after
                        .map(|b| b.available_capital().to_string())
                        .unwrap_or("---".to_string())
                );
            }
        }

        Ok(())
    }

    #[test]
    async fn test_archiving_treasurers_from_series_of_ledger_entries_should_yield_same_result() -> Result<()> {
        let ledger_archive_entries_str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/ledger_entry_export/export.json"));

        let ledger_archive_entries = serde_json::from_str::<Vec<LedgerArchiveEntry>>(ledger_archive_entries_str)?;
        let ledger_entries = ledger_archive_entries
            .iter()
            .map(|a| a.entry.clone())
            .collect_vec();
        let expected_num_chunks = ledger_entries.len().div_ceil(100);
        let actual_chunks = load_from_ledger_archive_entries(None, ledger_archive_entries, 100)?;

        assert_eq!(expected_num_chunks, actual_chunks.len());

        let actual_final_treasurer_from_chunks = actual_chunks.last().unwrap().clone().entry;

        let treasurer_from_whole_ledger = ImprovedTreasurer::from_ledger(ledger_entries)?;

        assert_eq!(actual_final_treasurer_from_chunks, treasurer_from_whole_ledger);

        Ok(())
    }

    fn load_from_ledger_archive_entries(
        latest_treasurer: Option<TreasurerArchiveEntry>,
        ledger_archive_entries: Vec<LedgerArchiveEntry>,
        chunk_size: usize,
    ) -> Result<Vec<TreasurerArchiveEntry>> {
        let from_id = latest_treasurer
            .clone()
            .map(|t| t.to_ledger_id + 1)
            .unwrap_or_default();

        let mut treasurer_archive_entries: Vec<TreasurerArchiveEntry> = latest_treasurer.iter().cloned().collect_vec();

        for chunk in &ledger_archive_entries
            .into_iter()
            .skip_while(|archive_entry| archive_entry.id < from_id)
            .chunks(chunk_size)
        {
            if let Some(current) = treasurer_archive_entries.last() {
                let mut first = None;
                let mut last = None;
                let mut new_treasurer = current.entry.clone();
                for ledger_entry in chunk {
                    if first.is_none() {
                        first = Some(ledger_entry.clone());
                    }
                    new_treasurer.process_ledger_entry(ledger_entry.entry.clone())?;
                    last = Some(ledger_entry);
                }

                treasurer_archive_entries.push(TreasurerArchiveEntry {
                    from_ledger_id: first.unwrap().id,
                    to_ledger_id: last.unwrap().id,
                    entry: new_treasurer,
                })
            } else {
                // no treasurer yet - we start a new one
                let serialized_chunk = chunk.collect_vec();
                let first = serialized_chunk.first().cloned().unwrap();
                let last = serialized_chunk.last().cloned().unwrap();
                let ledger_entries_of_chunk = serialized_chunk.into_iter().map(|x| x.entry).collect_vec();
                let new_treasurer = ImprovedTreasurer::from_ledger(ledger_entries_of_chunk)?;

                treasurer_archive_entries.push(TreasurerArchiveEntry {
                    from_ledger_id: first.id,
                    to_ledger_id: last.id,
                    entry: new_treasurer,
                })
            }
        }

        Ok(treasurer_archive_entries)
    }
}
