use chrono::{DateTime, Utc};
use itertools::Itertools;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentSymbol(pub String);

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SystemSymbol(pub String);

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WaypointSymbol(pub String);

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WaypointTraitSymbol(pub String);

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WaypointTrait {
    pub symbol: WaypointTraitSymbol,
    pub name: String,
    pub description: String,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WaypointModifierSymbol(pub String);

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WaypointModifier {
    pub symbol: WaypointModifierSymbol,
    pub name: String,
    pub description: String,
}

impl WaypointSymbol {
    pub fn system_symbol(&self) -> SystemSymbol {
        SystemSymbol(self.0.splitn(3, "-").take(2).join("-"))
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FactionSymbol(pub String);

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfoResponse {
    pub data: AgentInfoResponseData,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ConstructionMaterial {
    pub trade_symbol: String,
    pub required: u32,
    pub fulfilled: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GetConstructionResponseData {
    pub symbol: String,
    pub materials: Vec<ConstructionMaterial>,
    pub is_complete: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GetConstructionResponse {
    pub data: GetConstructionResponseData,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfoResponseData {
    pub account_id: Option<String>,
    pub symbol: String,
    pub headquarters: String,
    pub credits: i64,
    pub starting_faction: String,
    pub ship_count: u32,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StStatusResponse {
    pub status: String,
    pub version: String,
    pub reset_date: String,
    pub description: String,
    pub stats: Stats,
    pub leaderboards: Leaderboards,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub agents: i32,
    pub ships: i32,
    pub systems: i32,
    pub waypoints: i32,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Leaderboards {
    pub most_credits: Vec<AgentCredits>,
    pub most_submitted_charts: Vec<AgentCharts>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AgentCredits {
    pub agent_symbol: String,
    pub credits: i64,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AgentCharts {
    pub agent_symbol: String,
    pub chart_count: i32,
}

#[derive(Deserialize, Serialize, Debug, Copy, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub total: u32,
    pub page: u32,
    pub limit: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Chart {
    pub waypoint_symbol: Option<String>,
    pub submitted_by: String,
    pub submitted_on: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Faction {
    pub symbol: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Orbital {
    pub symbol: WaypointSymbol,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WaypointInSystemResponseData {
    pub symbol: WaypointSymbol,
    #[serde(rename = "type")]
    pub r#type: String,
    pub system_symbol: SystemSymbol,
    pub x: i64,
    pub y: i64,
    pub orbitals: Vec<Orbital>,
    pub orbits: Option<WaypointSymbol>,
    pub faction: Faction,
    pub traits: Vec<WaypointTrait>,
    pub modifiers: Vec<WaypointModifier>,
    pub chart: Chart,
    pub is_under_construction: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ListAgentsResponse {
    pub data: Vec<AgentInfoResponseData>,
    pub meta: Meta,
}

pub trait GetMeta {
    fn get_meta(&self) -> Meta;
}

impl GetMeta for ListAgentsResponse {
    fn get_meta(&self) -> Meta {
        self.meta
    }
}

pub(crate) fn extract_system_symbol(waypoint_symbol: &WaypointSymbol) -> SystemSymbol {
    let parts: Vec<&str> = waypoint_symbol.0.split('-').collect();
    // Join the first two parts with '-'
    let first_two_parts = parts[..2].join("-");
    SystemSymbol(first_two_parts)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GetMarketResponse {
    pub data: MarketData,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MarketData {
    pub symbol: String,
    pub exports: Vec<TradeGood>,
    pub imports: Vec<TradeGood>,
    pub exchange: Vec<TradeGood>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transactions: Option<Vec<Transaction>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trade_goods: Option<Vec<MarketTradeGood>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TradeGood {
    pub symbol: String,
    pub name: String,
    pub description: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    pub waypoint_symbol: String,
    pub ship_symbol: String,
    pub trade_symbol: String,
    #[serde(rename = "type")]
    pub transaction_type: TransactionType,
    pub units: i32,
    pub price_per_unit: i32,
    pub total_price: i32,
    pub timestamp: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransactionType {
    Purchase,
    Sell,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MarketTradeGood {
    pub symbol: String,
    #[serde(rename = "type")]
    pub trade_good_type: TradeGoodType,
    pub trade_volume: i32,
    pub supply: SupplyLevel,
    pub activity: Option<ActivityLevel>,
    pub purchase_price: i32,
    pub sell_price: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TradeGoodType {
    Export,
    Import,
    Exchange,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SupplyLevel {
    Scarce,
    Limited,
    Moderate,
    High,
    Abundant,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActivityLevel {
    Weak,
    Growing,
    Strong,
    Restricted,
}
