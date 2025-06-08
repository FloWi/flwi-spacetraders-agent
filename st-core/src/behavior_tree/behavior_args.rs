use anyhow::Result;
use st_domain::blackboard_ops::BlackboardOps;

use crate::contract_manager;
use crate::materialized_supply_chain_manager::MaterializedSupplyChainManager;
use crate::transfer_cargo_manager::TransferCargoManager;
use st_domain::budgeting::treasury_redesign::{FinanceTicket, FinanceTicketDetails, ThreadSafeTreasurer};
use st_domain::{
    Contract, FleetId, MarketEntry, PurchaseShipResponse, PurchaseTradeGoodResponse, SellTradeGoodResponse, ShipSymbol, SupplyConstructionSiteResponse,
    SystemSymbol,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct BehaviorArgs {
    pub blackboard: Arc<dyn BlackboardOps>,
    pub treasurer: ThreadSafeTreasurer,
    pub transfer_cargo_manager: Arc<TransferCargoManager>,
    pub materialized_supply_chain_manager: MaterializedSupplyChainManager,
}

impl BehaviorArgs {
    pub(crate) async fn mark_purchase_as_completed(&self, ticket: FinanceTicket, response: &PurchaseTradeGoodResponse) -> Result<()> {
        self.treasurer
            .complete_ticket(&ticket.fleet_id, &ticket, response.data.transaction.price_per_unit.into())
            .await?;

        Ok(())
    }

    pub(crate) async fn mark_sell_as_completed(&self, ticket: FinanceTicket, response: &SellTradeGoodResponse) -> Result<()> {
        self.treasurer
            .complete_ticket(&ticket.fleet_id, &ticket, response.data.transaction.price_per_unit.into())
            .await?;

        Ok(())
    }

    pub(crate) async fn mark_ship_purchase_as_completed(&self, ticket: FinanceTicket, response: &PurchaseShipResponse) -> Result<()> {
        self.treasurer
            .complete_ticket(&ticket.fleet_id, &ticket, (response.data.transaction.price as i64).into())
            .await?;

        Ok(())
    }

    pub(crate) async fn mark_construction_delivery_as_completed(&self, ticket: FinanceTicket, response: &SupplyConstructionSiteResponse) -> Result<()> {
        self.treasurer
            .complete_ticket(&ticket.fleet_id, &ticket, 0.into())
            .await?;

        Ok(())
    }

    // async fn check_contract_affordability(&self, contract: &Contract, cargo_capacity: u32, fleet_id: &FleetId) -> anyhow::Result<bool>

    pub(crate) async fn check_contract_affordability(
        &self,
        system_symbol: &SystemSymbol,
        contract: &Contract,
        cargo_capacity: u32,
        fleet_id: &FleetId,
    ) -> Result<bool> {
        let latest_market_entries: Vec<MarketEntry> = self
            .blackboard
            .get_latest_market_entries(system_symbol)
            .await?;

        let result = contract_manager::calculate_necessary_purchase_tickets_for_contract(cargo_capacity, contract, &latest_market_entries)?;

        let required_capital = result.required_capital();

        let budget = self.treasurer.get_fleet_budget(fleet_id).await?;

        if budget.available_capital() > required_capital {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub(crate) async fn create_contract_tickets(
        &self,
        ship_symbol: &ShipSymbol,
        system_symbol: &SystemSymbol,
        contract: &Contract,
        cargo_capacity: u32,
        fleet_id: &FleetId,
    ) -> Result<bool> {
        let latest_market_entries: Vec<MarketEntry> = self
            .blackboard
            .get_latest_market_entries(system_symbol)
            .await?;

        let result = contract_manager::calculate_necessary_purchase_tickets_for_contract(cargo_capacity, contract, &latest_market_entries)?;

        let required_capital = result.required_capital();

        let mut all_details: Vec<FinanceTicketDetails> = Vec::new();

        for purchase_ticket_details in result.purchase_tickets {
            all_details.push(FinanceTicketDetails::PurchaseTradeGoods(purchase_ticket_details))
        }

        for delivery_ticket_details in result.delivery_tickets {
            all_details.push(FinanceTicketDetails::DeliverContractCargo(delivery_ticket_details))
        }

        self.treasurer
            .create_multiple_tickets(ship_symbol, fleet_id, all_details)
            .await?;

        let budget = self.treasurer.get_fleet_budget(fleet_id).await?;

        if budget.available_capital() > required_capital {
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

// Implement Deref for BehaviorArgs to allow transparent access to BlackboardOps methods
impl std::ops::Deref for BehaviorArgs {
    type Target = dyn BlackboardOps;

    fn deref(&self) -> &Self::Target {
        &*self.blackboard
    }
}
