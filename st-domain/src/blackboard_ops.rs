use crate::{
    get_exploration_tasks_for_waypoint, Construction, CreateSurveyResponse, CreateSurveyResponseBody, ExplorationTask, Extraction, JumpGate, MarketData,
    MiningOpsConfig, Shipyard, Survey, TravelAction, Waypoint, WaypointSymbol,
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
    async fn get_available_agent_credits(&self) -> anyhow::Result<i64>;

    // async fn report_purchase(&self, ticket_id: &TicketId, transaction_id: &TransactionTicketId, response: &PurchaseTradeGoodResponse) -> Result<()>;
    // async fn report_sale(&self, ticket_id: &TicketId, transaction_id: &TransactionTicketId, response: &SellTradeGoodResponse) -> Result<()>;
    // async fn report_delivery(&self, ticket_id: &TicketId, transaction_id: &TransactionTicketId, response: &SupplyConstructionSiteResponse) -> Result<()>;
    // async fn report_ship_purchase(&self, ticket_id: &TicketId, ticket: &PurchaseShipTicketDetails, response: PurchaseShipResponse) -> Result<()>;

    async fn get_exploration_tasks_waypoint(&self, waypoint_symbol: &WaypointSymbol) -> anyhow::Result<Vec<ExplorationTask>> {
        let waypoint = self.get_waypoint(waypoint_symbol).await?;
        Ok(get_exploration_tasks_for_waypoint(&waypoint))
    }

    async fn update_construction_site(&self, construction: &Construction) -> anyhow::Result<()>;

    async fn get_best_survey_for_current_demand(&self, mining_config: &MiningOpsConfig) -> anyhow::Result<Option<Survey>>;
    async fn mark_survey_as_exhausted(&self, survey: &Survey) -> anyhow::Result<()>;
    async fn save_survey_response(&self, create_survey_response: CreateSurveyResponse) -> anyhow::Result<()>;
    async fn log_survey_usage(&self, survey: Survey, extraction: Extraction) -> anyhow::Result<()>;
    async fn is_survey_necessary(&self, maybe_mining_waypoint: Option<WaypointSymbol>) -> anyhow::Result<bool>;
}
