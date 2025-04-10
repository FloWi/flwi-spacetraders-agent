use crate::{
    get_exploration_tasks_for_waypoint, ExplorationTask, JumpGate, MarketData, Shipyard, TicketId, TradeTicket, TravelAction, Waypoint, WaypointSymbol,
};
use async_trait::async_trait;
use mockall::automock;

#[automock]
#[async_trait]
pub trait BlackboardOps: Send + Sync {
    async fn compute_path(
        &self,
        from: WaypointSymbol,
        to: WaypointSymbol,
        engine_speed: u32,
        current_fuel: u32,
        fuel_capacity: u32,
    ) -> anyhow::Result<Vec<TravelAction>>;
    async fn insert_waypoint(&self, waypoint: &Waypoint) -> anyhow::Result<()>;
    async fn insert_market(&self, market_data: MarketData) -> anyhow::Result<()>;
    async fn insert_jump_gate(&self, jump_gate: JumpGate) -> anyhow::Result<()>;
    async fn insert_shipyard(&self, shipyard: Shipyard) -> anyhow::Result<()>;
    async fn get_closest_waypoint(&self, current_waypoint: &WaypointSymbol, candidates: &[WaypointSymbol]) -> anyhow::Result<Option<WaypointSymbol>>;
    async fn get_waypoint(&self, waypoint_symbol: &WaypointSymbol) -> anyhow::Result<Waypoint>;
    async fn get_ticket_by_id(&self, ticket_id: TicketId) -> anyhow::Result<TradeTicket>;

    // async fn report_purchase(&self, ticket_id: &TicketId, transaction_id: &TransactionTicketId, response: &PurchaseTradeGoodResponse) -> Result<()>;
    // async fn report_sale(&self, ticket_id: &TicketId, transaction_id: &TransactionTicketId, response: &SellTradeGoodResponse) -> Result<()>;
    // async fn report_delivery(&self, ticket_id: &TicketId, transaction_id: &TransactionTicketId, response: &SupplyConstructionSiteResponse) -> Result<()>;
    // async fn report_ship_purchase(&self, ticket_id: &TicketId, ticket: &PurchaseShipTicketDetails, response: PurchaseShipResponse) -> Result<()>;

    async fn get_exploration_tasks_waypoint(&self, waypoint_symbol: &WaypointSymbol) -> anyhow::Result<Vec<ExplorationTask>> {
        let waypoint = self.get_waypoint(waypoint_symbol).await?;
        Ok(get_exploration_tasks_for_waypoint(&waypoint))
    }
}
