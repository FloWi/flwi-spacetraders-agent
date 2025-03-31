use chrono::{DateTime, Utc};
use itertools::Itertools;
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::Hash;
use strum_macros::Display;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Data<T> {
    pub data: T,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct AgentSymbol(pub String);

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct SystemSymbol(pub String);

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct WaypointSymbol(pub String);

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct ShipSymbol(pub String);

pub type GetJumpGateResponse = Data<JumpGate>;

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct JumpGate {
    pub symbol: WaypointSymbol,
    pub connections: Vec<WaypointSymbol>,
}

pub type GetShipyardResponse = Data<Shipyard>;

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Shipyard {
    pub symbol: WaypointSymbol,
    pub ship_types: Vec<ShipTypeEntry>,
    pub transactions: Option<Vec<ShipTransaction>>,
    pub ships: Option<Vec<ShipyardShip>>,
    pub modifications_fee: i32,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct ShipTransaction {}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct ShipyardShip {
    name: String,
    description: String,
    supply: SupplyLevel,
    activity: ActivityLevel,
    purchase_price: u32,
    frame: Frame,
    reactor: Reactor,
    engine: Engine,
    modules: Vec<Module>,
    mounts: Vec<Mount>,
    crew: ShipyardShipCrew,
}

pub type CreateChartResponse = Data<CreateChartBody>;

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct CreateChartBody {
    chart: Chart,
    pub waypoint: Waypoint,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Chart {
    waypoint_symbol: Option<WaypointSymbol>,
    submitted_by: Option<AgentSymbol>,
    submitted_on: Option<DateTime<Utc>>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[allow(non_camel_case_types)]
pub struct ShipTypeEntry {
    r#type: ShipType,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Display)]
#[allow(non_camel_case_types)]
pub enum ShipType {
    SHIP_PROBE,
    SHIP_MINING_DRONE,
    SHIP_SIPHON_DRONE,
    SHIP_INTERCEPTOR,
    SHIP_LIGHT_HAULER,
    SHIP_COMMAND_FRIGATE,
    SHIP_EXPLORER,
    SHIP_HEAVY_FREIGHTER,
    SHIP_LIGHT_SHUTTLE,
    SHIP_ORE_HOUND,
    SHIP_REFINING_FREIGHTER,
    SHIP_SURVEYOR,
    SHIP_BULK_FREIGHTER,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Display)]
#[allow(non_camel_case_types)]
pub enum ShipFrameSymbol {
    FRAME_PROBE,
    FRAME_DRONE,
    FRAME_INTERCEPTOR,
    FRAME_RACER,
    FRAME_FIGHTER,
    FRAME_FRIGATE,
    FRAME_SHUTTLE,
    FRAME_EXPLORER,
    FRAME_MINER,
    FRAME_LIGHT_FREIGHTER,
    FRAME_HEAVY_FREIGHTER,
    FRAME_TRANSPORT,
    FRAME_DESTROYER,
    FRAME_CRUISER,
    FRAME_CARRIER,
    FRAME_BULK_FREIGHTER,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Display)]
#[allow(non_camel_case_types)]
pub enum WaypointTraitSymbol {
    UNCHARTED,
    UNDER_CONSTRUCTION,
    MARKETPLACE,
    SHIPYARD,
    OUTPOST,
    SCATTERED_SETTLEMENTS,
    SPRAWLING_CITIES,
    MEGA_STRUCTURES,
    PIRATE_BASE,
    OVERCROWDED,
    HIGH_TECH,
    CORRUPT,
    BUREAUCRATIC,
    TRADING_HUB,
    INDUSTRIAL,
    BLACK_MARKET,
    RESEARCH_FACILITY,
    MILITARY_BASE,
    SURVEILLANCE_OUTPOST,
    EXPLORATION_OUTPOST,
    MINERAL_DEPOSITS,
    COMMON_METAL_DEPOSITS,
    PRECIOUS_METAL_DEPOSITS,
    RARE_METAL_DEPOSITS,
    METHANE_POOLS,
    ICE_CRYSTALS,
    EXPLOSIVE_GASES,
    STRONG_MAGNETOSPHERE,
    VIBRANT_AURORAS,
    SALT_FLATS,
    CANYONS,
    PERPETUAL_DAYLIGHT,
    PERPETUAL_OVERCAST,
    DRY_SEABEDS,
    MAGMA_SEAS,
    SUPERVOLCANOES,
    ASH_CLOUDS,
    VAST_RUINS,
    MUTATED_FLORA,
    TERRAFORMED,
    EXTREME_TEMPERATURES,
    EXTREME_PRESSURE,
    DIVERSE_LIFE,
    SCARCE_LIFE,
    FOSSILS,
    WEAK_GRAVITY,
    STRONG_GRAVITY,
    CRUSHING_GRAVITY,
    TOXIC_ATMOSPHERE,
    CORROSIVE_ATMOSPHERE,
    BREATHABLE_ATMOSPHERE,
    THIN_ATMOSPHERE,
    JOVIAN,
    ROCKY,
    VOLCANIC,
    FROZEN,
    SWAMP,
    BARREN,
    TEMPERATE,
    JUNGLE,
    OCEAN,
    RADIOACTIVE,
    MICRO_GRAVITY_ANOMALIES,
    DEBRIS_CLUSTER,
    DEEP_CRATERS,
    SHALLOW_CRATERS,
    UNSTABLE_COMPOSITION,
    HOLLOWED_INTERIOR,
    STRIPPED,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct WaypointTrait {
    pub symbol: WaypointTraitSymbol,
    pub name: String,
    pub description: String,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct WaypointModifierSymbol(pub String);

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
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

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct FactionSymbol(pub String);

#[derive(Deserialize, Serialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfoResponse {
    pub data: AgentInfoResponseData,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConstructionMaterial {
    pub trade_symbol: String,
    pub required: u32,
    pub fulfilled: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GetConstructionResponseData {
    pub symbol: String,
    pub materials: Vec<ConstructionMaterial>,
    pub is_complete: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GetConstructionResponse {
    pub data: GetConstructionResponseData,
}

#[derive(Deserialize, Serialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfoResponseData {
    pub account_id: Option<String>,
    pub symbol: String,
    pub headquarters: String,
    pub credits: i64,
    pub starting_faction: String,
    pub ship_count: u32,
}

#[derive(Deserialize, Serialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StStatusResponse {
    pub status: String,
    pub version: String,
    pub reset_date: String,
    pub description: String,
    pub stats: Stats,
    pub leaderboards: Leaderboards,
}

#[derive(Deserialize, Serialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub agents: i32,
    pub ships: i32,
    pub systems: i32,
    pub waypoints: i32,
}

#[derive(Deserialize, Serialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Leaderboards {
    pub most_credits: Vec<AgentCredits>,
    pub most_submitted_charts: Vec<AgentCharts>,
}

#[derive(Deserialize, Serialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentCredits {
    pub agent_symbol: String,
    pub credits: i64,
}

#[derive(Deserialize, Serialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentCharts {
    pub agent_symbol: String,
    pub chart_count: i32,
}

#[derive(Deserialize, Serialize, Debug, Copy, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub total: u32,
    pub page: u32,
    pub limit: u32,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Faction {
    pub symbol: String,
    pub name: String,
    pub description: String,
    pub headquarters: String,
    pub traits: Vec<FactionTrait>,
    pub is_recruiting: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct WaypointFaction {
    pub symbol: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct Orbital {
    pub symbol: WaypointSymbol,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Waypoint {
    pub symbol: WaypointSymbol,
    #[serde(rename = "type")]
    pub r#type: WaypointType,
    pub system_symbol: SystemSymbol,
    pub x: i64,
    pub y: i64,
    pub orbitals: Vec<Orbital>,
    pub orbits: Option<WaypointSymbol>,
    pub faction: Option<WaypointFaction>,
    pub traits: Vec<WaypointTrait>,
    pub modifiers: Vec<WaypointModifier>,
    pub chart: Option<Chart>,
    pub is_under_construction: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SystemPageWaypoint {
    pub symbol: WaypointSymbol,
    #[serde(rename = "type")]
    pub r#type: String,
    pub x: i64,
    pub y: i64,
    pub orbitals: Vec<Orbital>,
    pub orbits: Option<WaypointSymbol>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SystemsPageData {
    pub symbol: SystemSymbol,
    pub sector_symbol: String,
    #[serde(rename = "type")]
    pub r#type: String,
    pub x: i64,
    pub y: i64,
    pub waypoints: Vec<SystemPageWaypoint>,
    pub orbits: Option<WaypointSymbol>,
    pub factions: Vec<SystemFaction>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct SystemFaction {
    pub symbol: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
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

pub fn extract_system_symbol(waypoint_symbol: &WaypointSymbol) -> SystemSymbol {
    let parts: Vec<&str> = waypoint_symbol.0.split('-').collect();
    // Join the first two parts with '-'
    let first_two_parts = parts[..2].join("-");
    SystemSymbol(first_two_parts)
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GetMarketResponse {
    pub data: MarketData,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GetSystemResponse {
    pub data: SystemsPageData,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MarketData {
    pub symbol: WaypointSymbol,
    pub exports: Vec<TradeGood>,
    pub imports: Vec<TradeGood>,
    pub exchange: Vec<TradeGood>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transactions: Option<Vec<Transaction>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trade_goods: Option<Vec<MarketTradeGood>>,
}

impl MarketData {
    pub fn has_detailed_price_information(&self) -> bool {
        self.trade_goods.is_some()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TradeGood {
    pub symbol: TradeGoodSymbol,
    pub name: String,
    pub description: String,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    pub waypoint_symbol: WaypointSymbol,
    pub ship_symbol: ShipSymbol,
    pub trade_symbol: TradeGoodSymbol,
    #[serde(rename = "type")]
    pub transaction_type: TransactionType,
    pub units: i32,
    pub price_per_unit: i32,
    pub total_price: i32,
    pub timestamp: DateTime<Utc>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransactionType {
    Purchase,
    Sell,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MarketTradeGood {
    pub symbol: TradeGoodSymbol,
    #[serde(rename = "type")]
    pub trade_good_type: TradeGoodType,
    pub trade_volume: i32,
    pub supply: SupplyLevel,
    pub activity: Option<ActivityLevel>,
    pub purchase_price: i32,
    pub sell_price: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, Hash, Display, Ord, PartialOrd)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TradeGoodType {
    Export,
    Import,
    Exchange,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, Hash, Display, Ord, PartialOrd)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SupplyLevel {
    Abundant,
    High,
    Moderate,
    Limited,
    Scarce,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, Hash, Display, Ord, PartialOrd)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActivityLevel {
    Weak,
    Growing,
    Strong,
    Restricted,
}

pub trait LabelledCoordinate<T: Clone + PartialEq + Eq + Hash> {
    fn x(&self) -> i64;
    fn y(&self) -> i64;
    fn label(&self) -> &T;

    fn distance_to(&self, b: &Self) -> u32 {
        distance_to(self.x(), self.y(), b.x(), b.y())
    }

    // New method to create a serializable representation
    fn to_serializable(&self) -> SerializableCoordinate<T> {
        SerializableCoordinate {
            x: self.x(),
            y: self.y(),
            label: self.label().clone(),
        }
    }
}

pub fn distance_to(from_x: i64, from_y: i64, to_x: i64, to_y: i64) -> u32 {
    let dx = (to_x - from_x) as f64;
    let dy = (to_y - from_y) as f64;
    (dx * dx + dy * dy).sqrt().round() as u32
}

impl LabelledCoordinate<WaypointSymbol> for Waypoint {
    fn x(&self) -> i64 {
        self.x
    }

    fn y(&self) -> i64 {
        self.y
    }

    fn label(&self) -> &WaypointSymbol {
        &self.symbol
    }
}

// Serializable struct that represents any LabelledCoordinate
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SerializableCoordinate<T> {
    x: i64,
    y: i64,
    label: T,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationRequest {
    pub faction: FactionSymbol,
    pub symbol: String,
    pub email: String,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationResponse {
    pub agent: Agent,
    pub contract: Contract,
    pub faction: Faction,
    pub ships: Vec<Ship>,
    pub token: String,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub account_id: Option<String>,
    pub symbol: AgentSymbol,
    pub headquarters: String,
    pub credits: i64,
    pub starting_faction: String,
    pub ship_count: i32,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Contract {
    pub id: String,
    pub faction_symbol: String,
    #[serde(rename = "type")]
    pub contract_type: String,
    pub terms: ContractTerms,
    pub accepted: bool,
    pub fulfilled: bool,
    pub deadline_to_accept: DateTime<Utc>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct ContractTerms {
    pub deadline: DateTime<Utc>,
    pub payment: Payment,
    pub deliver: Vec<Delivery>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Payment {
    pub on_accepted: i64,
    pub on_fulfilled: i64,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Delivery {
    pub trade_symbol: String,
    pub destination_symbol: String,
    pub units_required: i32,
    pub units_fulfilled: i32,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct FactionTrait {
    pub symbol: String,
    pub name: String,
    pub description: String,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Ship {
    pub symbol: ShipSymbol,
    pub registration: Registration,
    pub nav: Nav,
    pub crew: Crew,
    pub frame: Frame,
    pub reactor: Reactor,
    pub engine: Engine,
    pub cooldown: Cooldown,
    pub modules: Vec<Module>,
    pub mounts: Vec<Mount>,
    pub cargo: Cargo,
    pub fuel: Fuel,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Registration {
    pub name: String,
    pub faction_symbol: String,
    pub role: ShipRegistrationRole,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ShipRegistrationRole {
    Fabricator,
    Harvester,
    Hauler,
    Interceptor,
    Excavator,
    Transport,
    Repair,
    Surveyor,
    Command,
    Carrier,
    Patrol,
    Satellite,
    Explorer,
    Refinery,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Nav {
    pub system_symbol: SystemSymbol,
    pub waypoint_symbol: WaypointSymbol,
    pub route: Route,
    pub status: NavStatus,
    pub flight_mode: FlightMode,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NavStatus {
    InTransit,
    InOrbit,
    Docked,
}

#[derive(Serialize, Deserialize, Eq, Hash, Clone, Debug, PartialEq, Display, Ord, PartialOrd)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FlightMode {
    Drift,
    Stealth,
    Cruise,
    Burn,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct NavAndFuelResponse {
    pub nav: Nav,
    pub fuel: Fuel,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct NavOnlyResponse {
    pub nav: Nav,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct DockShipResponse {
    pub data: NavOnlyResponse,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct PatchShipNavRequest {
    pub flight_mode: FlightMode,
}

pub type PatchShipNavResponse = Data<NavOnlyResponse>;

pub type SetFlightModeResponse = Data<NavAndFuelResponse>;

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct NavigateShipRequest {
    pub waypoint_symbol: WaypointSymbol,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct RefuelShipRequest {
    pub from_cargo: bool,
    pub amount: u32,
}

pub type RefuelShipResponse = Data<RefuelShipResponseBody>;

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct RefuelShipResponseBody {
    pub agent: Agent,
    pub fuel: Fuel,
    pub transaction: Transaction,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct NavigateShipResponse {
    pub data: NavAndFuelResponse,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct OrbitShipResponse {
    pub data: NavOnlyResponse,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    pub destination: NavRouteWaypoint,
    pub origin: NavRouteWaypoint,
    pub departure_time: DateTime<Utc>,
    pub arrival: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, Hash, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct NavRouteWaypoint {
    pub symbol: WaypointSymbol,
    #[serde(rename = "type")]
    pub waypoint_type: WaypointType,
    pub system_symbol: SystemSymbol,
    pub x: i32,
    pub y: i32,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Crew {
    pub current: i32,
    pub required: i32,
    pub capacity: i32,
    pub rotation: String,
    pub morale: i32,
    pub wages: i32,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct ShipyardShipCrew {
    pub required: i32,
    pub capacity: i32,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    pub symbol: ShipFrameSymbol,
    pub name: String,
    pub description: String,
    pub condition: OrderedFloat<f32>,
    pub integrity: OrderedFloat<f32>,
    pub module_slots: i32,
    pub mounting_points: i32,
    pub fuel_capacity: i32,
    pub requirements: Requirements,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Reactor {
    pub symbol: String,
    pub name: String,
    pub description: String,
    pub condition: OrderedFloat<f32>,
    pub integrity: OrderedFloat<f32>,
    pub power_output: i32,
    pub requirements: Requirements,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Engine {
    pub symbol: String,
    pub name: String,
    pub description: String,
    pub condition: OrderedFloat<f32>,
    pub integrity: OrderedFloat<f32>,
    pub speed: i32,
    pub requirements: Requirements,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Requirements {
    pub power: Option<i32>,
    pub crew: Option<i32>,
    pub slots: Option<i32>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Cooldown {
    pub ship_symbol: String,
    pub total_seconds: i32,
    pub remaining_seconds: i32,
    pub expiration: Option<DateTime<Utc>>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Module {
    pub symbol: String,
    pub capacity: Option<i32>,
    pub range: Option<i32>,
    pub name: String,
    pub description: String,
    pub requirements: Requirements,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Mount {
    pub symbol: String,
    pub name: String,
    pub description: Option<String>,
    pub strength: Option<i32>,
    pub deposits: Option<Vec<String>>,
    pub requirements: Requirements,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Cargo {
    pub capacity: i32,
    pub units: i32,
    pub inventory: Vec<Inventory>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub symbol: String,
    pub name: String,
    pub description: String,
    pub units: i32,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Fuel {
    pub current: i32,
    pub capacity: i32,
    pub consumed: FuelConsumed,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct FuelConsumed {
    pub amount: i32,
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupplyChainMap {
    pub export_to_import_map: HashMap<TradeGoodSymbol, Vec<TradeGoodSymbol>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetSupplyChainResponse {
    pub(crate) data: SupplyChainMap,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[allow(non_camel_case_types)]
pub enum WaypointType {
    PLANET,
    GAS_GIANT,
    MOON,
    ORBITAL_STATION,
    JUMP_GATE,
    ASTEROID_FIELD,
    ASTEROID,
    ENGINEERED_ASTEROID,
    ASTEROID_BASE,
    NEBULA,
    DEBRIS_FIELD,
    GRAVITY_WELL,
    ARTIFICIAL_GRAVITY_WELL,
    FUEL_STATION,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Display)]
#[allow(non_camel_case_types)]
pub enum TradeGoodSymbol {
    PRECIOUS_STONES,
    QUARTZ_SAND,
    SILICON_CRYSTALS,
    AMMONIA_ICE,
    LIQUID_HYDROGEN,
    LIQUID_NITROGEN,
    ICE_WATER,
    EXOTIC_MATTER,
    ADVANCED_CIRCUITRY,
    GRAVITON_EMITTERS,
    IRON,
    IRON_ORE,
    COPPER,
    COPPER_ORE,
    ALUMINUM,
    ALUMINUM_ORE,
    SILVER,
    SILVER_ORE,
    GOLD,
    GOLD_ORE,
    PLATINUM,
    PLATINUM_ORE,
    DIAMONDS,
    URANITE,
    URANITE_ORE,
    MERITIUM,
    MERITIUM_ORE,
    HYDROCARBON,
    ANTIMATTER,
    FAB_MATS,
    FERTILIZERS,
    FABRICS,
    FOOD,
    JEWELRY,
    MACHINERY,
    FIREARMS,
    ASSAULT_RIFLES,
    MILITARY_EQUIPMENT,
    EXPLOSIVES,
    LAB_INSTRUMENTS,
    AMMUNITION,
    ELECTRONICS,
    SHIP_PLATING,
    SHIP_PARTS,
    EQUIPMENT,
    FUEL,
    MEDICINE,
    DRUGS,
    CLOTHING,
    MICROPROCESSORS,
    PLASTICS,
    POLYNUCLEOTIDES,
    BIOCOMPOSITES,
    QUANTUM_STABILIZERS,
    NANOBOTS,
    AI_MAINFRAMES,
    QUANTUM_DRIVES,
    ROBOTIC_DRONES,
    CYBER_IMPLANTS,
    GENE_THERAPEUTICS,
    NEURAL_CHIPS,
    MOOD_REGULATORS,
    VIRAL_AGENTS,
    MICRO_FUSION_GENERATORS,
    SUPERGRAINS,
    LASER_RIFLES,
    HOLOGRAPHICS,
    SHIP_SALVAGE,
    RELIC_TECH,
    NOVEL_LIFEFORMS,
    BOTANICAL_SPECIMENS,
    CULTURAL_ARTIFACTS,
    FRAME_PROBE,
    FRAME_DRONE,
    FRAME_INTERCEPTOR,
    FRAME_RACER,
    FRAME_FIGHTER,
    FRAME_FRIGATE,
    FRAME_SHUTTLE,
    FRAME_EXPLORER,
    FRAME_MINER,
    FRAME_LIGHT_FREIGHTER,
    FRAME_HEAVY_FREIGHTER,
    FRAME_TRANSPORT,
    FRAME_DESTROYER,
    FRAME_CRUISER,
    FRAME_CARRIER,
    REACTOR_SOLAR_I,
    REACTOR_FUSION_I,
    REACTOR_FISSION_I,
    REACTOR_CHEMICAL_I,
    REACTOR_ANTIMATTER_I,
    ENGINE_IMPULSE_DRIVE_I,
    ENGINE_ION_DRIVE_I,
    ENGINE_ION_DRIVE_II,
    ENGINE_HYPER_DRIVE_I,
    MODULE_MINERAL_PROCESSOR_I,
    MODULE_GAS_PROCESSOR_I,
    MODULE_CARGO_HOLD_I,
    MODULE_CARGO_HOLD_II,
    MODULE_CARGO_HOLD_III,
    MODULE_CREW_QUARTERS_I,
    MODULE_ENVOY_QUARTERS_I,
    MODULE_PASSENGER_CABIN_I,
    MODULE_MICRO_REFINERY_I,
    MODULE_SCIENCE_LAB_I,
    MODULE_JUMP_DRIVE_I,
    MODULE_JUMP_DRIVE_II,
    MODULE_JUMP_DRIVE_III,
    MODULE_WARP_DRIVE_I,
    MODULE_WARP_DRIVE_II,
    MODULE_WARP_DRIVE_III,
    MODULE_SHIELD_GENERATOR_I,
    MODULE_SHIELD_GENERATOR_II,
    MODULE_ORE_REFINERY_I,
    MODULE_FUEL_REFINERY_I,
    MOUNT_GAS_SIPHON_I,
    MOUNT_GAS_SIPHON_II,
    MOUNT_GAS_SIPHON_III,
    MOUNT_SURVEYOR_I,
    MOUNT_SURVEYOR_II,
    MOUNT_SURVEYOR_III,
    MOUNT_SENSOR_ARRAY_I,
    MOUNT_SENSOR_ARRAY_II,
    MOUNT_SENSOR_ARRAY_III,
    MOUNT_MINING_LASER_I,
    MOUNT_MINING_LASER_II,
    MOUNT_MINING_LASER_III,
    MOUNT_LASER_CANNON_I,
    MOUNT_MISSILE_LAUNCHER_I,
    MOUNT_TURRET_I,
    SHIP_PROBE,
    SHIP_MINING_DRONE,
    SHIP_SIPHON_DRONE,
    SHIP_INTERCEPTOR,
    SHIP_LIGHT_HAULER,
    SHIP_COMMAND_FRIGATE,
    SHIP_EXPLORER,
    SHIP_HEAVY_FREIGHTER,
    SHIP_LIGHT_SHUTTLE,
    SHIP_ORE_HOUND,
    SHIP_REFINING_FREIGHTER,
    SHIP_SURVEYOR,
    SHIP_BULK_FREIGHTER,
}
