use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentSymbol(pub String);

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SystemSymbol(pub String);

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WaypointSymbol(pub String);

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FactionSymbol(pub String);

#[derive(Deserialize, Serialize, Debug)]
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

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GetConstructionResponseData {
    pub symbol: String,
    pub materials: Vec<ConstructionMaterial>,
    pub is_complete: bool,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GetConstructionResponse {
    pub data: GetConstructionResponseData,
}

#[derive(Deserialize, Serialize, Debug)]
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

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ListWaypointsInSystemResponse {
    pub data: Vec<ListWaypointsInSystemResponseData>,
    pub meta: Meta,
}

#[derive(Deserialize, Serialize, Debug, Copy, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub total: u32,
    pub page: u32,
    pub limit: u32,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Chart {
    pub waypoint_symbol: Option<String>,
    pub submitted_by: String,
    pub submitted_on: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Faction {
    pub symbol: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ListWaypointsInSystemResponseData {
    pub symbol: String,
    #[serde(rename = "type")]
    pub r#type: String,
    pub system_symbol: String,
    pub x: i64,
    pub y: i64,
    //pub orbitals: Vec<WaypointSymbol>,
    pub orbits: Option<WaypointSymbol>,
    pub faction: Faction,
    //pub traits: Vec<Struct>,
    //pub modifiers: Vec<Struct>,
    pub chart: Chart,
    pub is_under_construction: bool,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ListAgentsResponse {
    pub data: Vec<AgentInfoResponseData>,
    pub meta: Meta,
}

pub trait GetMeta {
    fn get_meta(&self) -> Meta;
}

impl GetMeta for ListWaypointsInSystemResponse {
    fn get_meta(&self) -> Meta {
        self.meta
    }
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
