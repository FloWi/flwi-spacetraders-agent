use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use itertools::Itertools;
use lazy_static::lazy_static;
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::hash::Hash;
use strum::{Display, EnumIter, EnumString, IntoEnumIterator};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Data<T> {
    pub data: T,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct AgentSymbol(pub String);

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct ContractId(pub String);

impl Display for ContractId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct SystemSymbol(pub String);

impl SystemSymbol {
    pub fn with_waypoint_suffix(&self, suffix: &str) -> WaypointSymbol {
        WaypointSymbol(format!("{}-{}", self.0, suffix))
    }
}

impl Display for SystemSymbol {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct WaypointSymbol(pub String);

impl Display for WaypointSymbol {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct ShipSymbol(pub String);

impl Display for ShipSymbol {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

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

impl Shipyard {
    pub fn has_detailed_price_information(&self) -> bool {
        self.ships.is_some()
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct ShipTransaction {
    pub waypoint_symbol: WaypointSymbol,
    pub ship_type: ShipType,
    pub price: u32,
    pub agent_symbol: AgentSymbol,
    pub timestamp: DateTime<Utc>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct ShipyardShip {
    pub name: String,
    pub r#type: ShipType,
    pub description: String,
    pub supply: SupplyLevel,
    pub activity: ActivityLevel,
    pub purchase_price: u32,
    pub frame: Frame,
    pub reactor: Reactor,
    pub engine: Engine,
    pub modules: Vec<Module>,
    pub mounts: Vec<Mount>,
    pub crew: ShipyardShipCrew,
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
    pub r#type: ShipType,
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

#[derive(Deserialize, Serialize, Debug, Display, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[allow(non_camel_case_types)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum WaypointModifierSymbol {
    STRIPPED,
    UNSTABLE,
    RADIATION_LEAK,
    CRITICAL_LIMIT,
    CIVIL_UNREST,
}

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

    pub fn symbol_ex_system_symbol(&self) -> String {
        self.0.rsplit_once('-').unwrap().1.to_string()
    }
}

#[derive(Deserialize, Serialize, Debug, EnumString, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[allow(non_camel_case_types)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum FactionSymbol {
    COSMIC,
    VOID,
    GALACTIC,
    QUANTUM,
    DOMINION,
    ASTRO,
    CORSAIRS,
    OBSIDIAN,
    AEGIS,
    UNITED,
    SOLITARY,
    COBALT,
    OMEGA,
    ECHO,
    LORDS,
    CULT,
    ANCIENTS,
    SHADOW,
    ETHEREAL,
}

#[derive(Deserialize, Serialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentResponse {
    pub data: Agent,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConstructionMaterial {
    pub trade_symbol: TradeGoodSymbol,
    pub required: u32,
    pub fulfilled: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Construction {
    pub symbol: WaypointSymbol,
    pub materials: Vec<ConstructionMaterial>,
    pub is_complete: bool,
}

impl Construction {
    pub(crate) fn all_construction_materials(&self) -> HashMap<TradeGoodSymbol, u32> {
        self.materials
            .iter()
            .map(|cm| (cm.trade_symbol.clone(), cm.required))
            .collect()
    }

    pub fn missing_construction_materials(&self) -> HashMap<TradeGoodSymbol, u32> {
        if self.is_complete {
            Default::default()
        } else {
            self.materials
                .iter()
                .filter_map(|cm| {
                    let missing = cm.required - cm.fulfilled;
                    (missing > 0).then(|| (cm.trade_symbol.clone(), missing))
                })
                .collect()
        }
    }

    pub fn get_material_mut(&mut self, trade_symbol: &TradeGoodSymbol) -> Option<&mut ConstructionMaterial> {
        self.materials
            .iter_mut()
            .find(|material| &material.trade_symbol == trade_symbol)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GetConstructionResponse {
    pub data: Construction,
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

impl Waypoint {
    pub fn has_reached_critical_limit(&self) -> bool {
        self.modifiers
            .iter()
            .any(|wp_modifier| wp_modifier.symbol == WaypointModifierSymbol::CRITICAL_LIMIT)
    }
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
    pub data: Vec<Agent>,
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
pub struct TransferCargoRequest {
    pub trade_symbol: TradeGoodSymbol,
    pub units: u32,
    pub ship_symbol: ShipSymbol,
}

pub type TransferCargoResponse = Data<TransferCargoResponseBody>;

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TransferCargoResponseBody {
    pub cargo: Cargo,
    pub target_cargo: Cargo,
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

/*
 case object SCARCE extends SupplyLevel("SCARCE", "SCARCE", 0, Hints())
 case object LIMITED extends SupplyLevel("LIMITED", "LIMITED", 1, Hints())
 case object MODERATE extends SupplyLevel("MODERATE", "MODERATE", 2, Hints())
 case object HIGH extends SupplyLevel("HIGH", "HIGH", 3, Hints())
 case object ABUNDANT extends SupplyLevel("ABUNDANT", "ABUNDANT", 4, Hints())


 case object WEAK extends ActivityLevel("WEAK", "WEAK", 0, Hints())
 case object GROWING extends ActivityLevel("GROWING", "GROWING", 1, Hints())
 case object STRONG extends ActivityLevel("STRONG", "STRONG", 2, Hints())
 case object RESTRICTED extends ActivityLevel("RESTRICTED", "RESTRICTED", 3, Hints())

*/

#[derive(Serialize, Deserialize, Clone, Debug, Display, EnumIter, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SupplyLevel {
    Scarce = 0,
    Limited = 1,
    Moderate = 2,
    High = 3,
    Abundant = 4,
}

#[derive(Serialize, Deserialize, Clone, Debug, Display, EnumIter, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActivityLevel {
    Strong = 4,
    Growing = 3,
    Weak = 2,
    Restricted = 1,
}

lazy_static! {
    pub static ref MAX_SUPPLY_LEVEL_SCORE: i32 = {
        SupplyLevel::iter()
            .map(|level| level as i32)
            .max()
            .unwrap_or(0)
    };
    pub static ref MAX_ACTIVITY_LEVEL_SCORE: i32 = {
        ActivityLevel::iter()
            .map(|level| level as i32)
            .max()
            .unwrap_or(0)
    };
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
    pub headquarters: WaypointSymbol,
    pub credits: i64,
    pub starting_faction: FactionSymbol,
    pub ship_count: i32,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Contract {
    pub id: ContractId,
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
    pub trade_symbol: TradeGoodSymbol,
    pub destination_symbol: WaypointSymbol,
    pub units_required: u32,
    pub units_fulfilled: u32,
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

impl Ship {
    pub fn is_in_orbit(&self) -> bool {
        self.nav.status == NavStatus::InOrbit || (self.nav.status == NavStatus::InTransit && self.nav.route.arrival <= Utc::now())
    }

    pub fn is_docked(&self) -> bool {
        self.nav.status == NavStatus::Docked
    }

    pub fn is_stationary(&self) -> bool {
        self.is_docked() || self.is_in_orbit()
    }

    pub fn has_trade_good_in_cargo(&self, trade_good: &TradeGoodSymbol, units: u32) -> bool {
        self.cargo
            .inventory
            .iter()
            .any(|inv| &inv.symbol == trade_good && inv.units >= units)
    }

    pub fn available_cargo_space(&self) -> u32 {
        (self.cargo.capacity - self.cargo.units) as u32
    }

    pub fn get_yield_size_for_siphoning(&self) -> u32 {
        self.mounts
            .iter()
            .filter_map(|m| {
                m.symbol
                    .is_gas_siphon()
                    .then_some(m.strength.unwrap_or_default() as u32)
            })
            .sum::<u32>()
    }

    pub fn is_mining_drone(&self) -> bool {
        match self.frame.symbol {
            ShipFrameSymbol::FRAME_DRONE => self.get_yield_size_for_mining() > 0,
            ShipFrameSymbol::FRAME_MINER => true,
            _ => false,
        }
    }

    pub fn is_hauler(&self) -> bool {
        match self.frame.symbol {
            ShipFrameSymbol::FRAME_BULK_FREIGHTER => true,
            ShipFrameSymbol::FRAME_LIGHT_FREIGHTER => true,
            ShipFrameSymbol::FRAME_HEAVY_FREIGHTER => true,
            _ => false,
        }
    }

    pub fn is_command_ship(&self) -> bool {
        self.frame.symbol == ShipFrameSymbol::FRAME_FRIGATE
    }

    pub fn is_siphoner(&self) -> bool {
        match self.frame.symbol {
            ShipFrameSymbol::FRAME_DRONE => self.get_yield_size_for_siphoning() > 0,
            _ => false,
        }
    }

    pub fn is_surveyor(&self) -> bool {
        match self.frame.symbol {
            ShipFrameSymbol::FRAME_DRONE => self.get_yield_size_for_surveying() > 0,
            _ => false,
        }
    }

    pub fn get_yield_size_for_mining(&self) -> u32 {
        self.mounts
            .iter()
            .filter_map(|m| {
                m.symbol
                    .is_mining_laser()
                    .then_some(m.strength.unwrap_or_default() as u32)
            })
            .sum::<u32>()
    }

    pub fn get_yield_size_for_surveying(&self) -> u32 {
        self.mounts
            .iter()
            .filter_map(|m| {
                m.symbol
                    .is_surveyor()
                    .then_some(m.strength.unwrap_or_default() as u32)
            })
            .sum::<u32>()
    }

    pub fn try_add_cargo(&mut self, units: u32, trade_good_symbol: &TradeGoodSymbol) -> Result<()> {
        if self.cargo.units + units as i32 > self.cargo.capacity {
            return Err(anyhow!("Not enough cargo space"));
        }
        if let Some(idx) = self
            .cargo
            .inventory
            .iter()
            .position(|inv| &inv.symbol == trade_good_symbol)
        {
            self.cargo.inventory.get_mut(idx).unwrap().units += units;
            self.cargo.units += units as i32;
        } else {
            let new_entry = Inventory {
                symbol: trade_good_symbol.clone(),
                units,
            };
            self.cargo.inventory.push(new_entry);
            self.cargo.units += units as i32;
        }
        Ok(())
    }

    pub fn try_remove_cargo(&mut self, units: u32, trade_good_symbol: &TradeGoodSymbol) -> Result<()> {
        if let Some((idx, inv)) = self
            .cargo
            .inventory
            .iter()
            .find_position(|inv| &inv.symbol == trade_good_symbol)
        {
            if inv.units < units {
                return Err(anyhow!(
                    "Cannot remove {} units of {} from cargo. Only {} units in inventory.",
                    units,
                    trade_good_symbol.to_string(),
                    inv.units
                ));
            }
            self.cargo.inventory.get_mut(idx).unwrap().units -= units;
            self.cargo.units -= units as i32;

            if self.cargo.inventory.get_mut(idx).unwrap().units == 0 {
                self.cargo.inventory.remove(idx);
            }
        } else {
            return Err(anyhow!("Cargo inventory entry for {} not found", trade_good_symbol.to_string()));
        }
        Ok(())
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Registration {
    pub name: String,
    pub faction_symbol: FactionSymbol,
    pub role: ShipRegistrationRole,
}

#[derive(Deserialize, Serialize, Debug, Clone, Display, PartialEq, Eq, Hash, Ord, PartialOrd)]
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
pub struct CargoOnlyResponse {
    pub cargo: Cargo,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct DockShipResponse {
    pub data: NavOnlyResponse,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct SiphonYield {
    pub symbol: TradeGoodSymbol,
    pub units: u32,
}
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Siphon {
    pub ship_symbol: ShipSymbol,
    #[serde(rename = "yield")]
    pub siphon_yield: SiphonYield,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct SiphonResourcesResponseBody {
    pub siphon: Siphon,
    pub cooldown: Cooldown,
    pub cargo: Cargo,
    //FIXME: add events
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct PatchShipNavRequest {
    pub flight_mode: FlightMode,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct JettisonCargoRequest {
    pub symbol: TradeGoodSymbol,
    pub units: u32,
}

pub type SiphonResourcesResponse = Data<SiphonResourcesResponseBody>;

pub type JettisonCargoResponse = Data<CargoOnlyResponse>;

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
pub struct SellTradeGoodRequest {
    pub symbol: TradeGoodSymbol,
    pub units: u32,
}

pub type SellTradeGoodResponse = Data<SellTradeGoodResponseBody>;
pub type PurchaseTradeGoodResponse = Data<PurchaseTradeGoodResponseBody>;
pub type SupplyConstructionSiteResponse = Data<SupplyConstructionSiteResponseBody>;
pub type PurchaseShipResponse = Data<PurchaseShipResponseBody>;

pub type ExtractResourcesResponse = Data<ExtractResourcesResponseBody>;

pub type CreateSurveyResponse = Data<CreateSurveyResponseBody>;

pub type NegotiateContractResponse = Data<NegotiateContractResponseBody>;

pub type AcceptContractResponse = Data<ContractWithAgentResponseBody>;

pub type FulfillContractResponse = Data<ContractWithAgentResponseBody>;

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct NegotiateContractResponseBody {
    pub contract: Contract,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct DeliverCargoToContractRequest {
    pub ship_symbol: ShipSymbol,
    pub trade_symbol: TradeGoodSymbol,
    pub units: u32,
}

pub type DeliverCargoToContractResponse = Data<DeliverCargoToContractResponseBody>;

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct ContractWithAgentResponseBody {
    pub contract: Contract,
    pub agent: Agent,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct DeliverCargoToContractResponseBody {
    pub contract: Contract,
    pub cargo: Cargo,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct SurveySignature(pub String);

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct SurveyDeposit {
    pub symbol: TradeGoodSymbol,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, EnumIter)]
#[allow(non_camel_case_types)]
pub enum SurveySize {
    SMALL,
    MODERATE,
    LARGE,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Survey {
    pub signature: SurveySignature,
    #[serde(rename = "symbol")]
    pub waypoint_symbol: WaypointSymbol,
    pub deposits: Vec<SurveyDeposit>,
    pub expiration: DateTime<Utc>,
    pub size: SurveySize,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct CreateSurveyResponseBody {
    pub cooldown: Cooldown,
    pub surveys: Vec<Survey>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Extraction {
    pub ship_symbol: ShipSymbol,
    #[serde(rename = "yield")]
    pub extraction_yield: ExtractionYield,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionYield {
    pub symbol: TradeGoodSymbol,
    pub units: u32,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct ExtractResourcesResponseBody {
    pub extraction: Extraction,
    pub cooldown: Cooldown,
    pub cargo: Cargo,
    pub modifiers: Option<Vec<WaypointModifier>>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct SellTradeGoodResponseBody {
    pub agent: Agent,
    pub cargo: Cargo,
    pub transaction: Transaction,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseTradeGoodRequest {
    pub symbol: TradeGoodSymbol,
    pub units: u32,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseTradeGoodResponseBody {
    pub agent: Agent,
    pub cargo: Cargo,
    pub transaction: Transaction,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct SupplyConstructionSiteRequest {
    pub ship_symbol: ShipSymbol,
    pub trade_symbol: TradeGoodSymbol,
    pub units: u32,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SupplyConstructionSiteResponseBody {
    pub cargo: Cargo,
    pub construction: Construction,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseShipRequest {
    pub ship_type: ShipType,
    pub waypoint_symbol: WaypointSymbol,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseShipResponseBody {
    pub ship: Ship,
    pub transaction: ShipPurchaseTransaction,
    pub agent: Agent,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ShipPurchaseTransaction {
    pub ship_symbol: ShipSymbol,
    pub ship_type: ShipType,
    pub waypoint_symbol: WaypointSymbol,
    pub agent_symbol: AgentSymbol,
    pub price: u64,
    pub timestamp: DateTime<Utc>,
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
    pub x: i64,
    pub y: i64,
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
    pub ship_symbol: ShipSymbol,
    pub total_seconds: i32,
    pub remaining_seconds: i32,
    pub expiration: Option<DateTime<Utc>>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[allow(non_camel_case_types)]
pub enum ModuleType {
    MODULE_MINERAL_PROCESSOR_I,
    MODULE_GAS_PROCESSOR_I,
    MODULE_CARGO_HOLD_I,
    MODULE_CARGO_HOLD_II,
    MODULE_CARGO_HOLD_III,
    MODULE_CREW_QUARTERS_I,
    MODULE_ENVOY_QUARTERS_I,
    MODULE_PASSENGER_CABIN_I,
    MODULE_MICRO_REFINERY_I,
    MODULE_ORE_REFINERY_I,
    MODULE_FUEL_REFINERY_I,
    MODULE_SCIENCE_LAB_I,
    MODULE_JUMP_DRIVE_I,
    MODULE_JUMP_DRIVE_II,
    MODULE_JUMP_DRIVE_III,
    MODULE_WARP_DRIVE_I,
    MODULE_WARP_DRIVE_II,
    MODULE_WARP_DRIVE_III,
    MODULE_SHIELD_GENERATOR_I,
    MODULE_SHIELD_GENERATOR_II,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Module {
    pub symbol: ModuleType,
    pub capacity: Option<i32>,
    pub range: Option<i32>,
    pub name: String,
    pub description: String,
    pub requirements: Requirements,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Mount {
    pub symbol: ShipMountSymbol,
    pub name: String,
    pub description: Option<String>,
    pub strength: Option<i32>,
    pub deposits: Option<Vec<TradeGoodSymbol>>,
    pub requirements: Requirements,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[allow(non_camel_case_types)]
pub enum ShipMountSymbol {
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
}

impl ShipMountSymbol {
    pub fn is_mining_laser(&self) -> bool {
        self == &ShipMountSymbol::MOUNT_MINING_LASER_I || self == &ShipMountSymbol::MOUNT_MINING_LASER_II || self == &ShipMountSymbol::MOUNT_MINING_LASER_III
    }

    pub fn is_gas_siphon(&self) -> bool {
        self == &ShipMountSymbol::MOUNT_GAS_SIPHON_I || self == &ShipMountSymbol::MOUNT_GAS_SIPHON_II || self == &ShipMountSymbol::MOUNT_GAS_SIPHON_III
    }

    pub fn is_surveyor(&self) -> bool {
        self == &ShipMountSymbol::MOUNT_SURVEYOR_I || self == &ShipMountSymbol::MOUNT_SURVEYOR_II || self == &ShipMountSymbol::MOUNT_SURVEYOR_III
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct Cargo {
    pub capacity: i32,
    pub units: i32,
    pub inventory: Vec<Inventory>,
}

impl Cargo {
    pub fn available_cargo_space(&self) -> u32 {
        
        (self.capacity - self.units) as u32
    }

    pub fn with_item_added_mut(&mut self, new_item: TradeGoodSymbol, units: u32) -> Result<(), NotEnoughSpaceError> {
        let available_space = self.available_cargo_space();

        if available_space < units {
            return Err(NotEnoughSpaceError {
                required: units,
                available: available_space,
            });
        }

        // Find the index of the inventory item with the matching symbol
        let maybe_item_index = self
            .inventory
            .iter()
            .position(|item| item.symbol == new_item);

        // If the item exists in inventory
        if let Some(index) = maybe_item_index {
            let item = &mut self.inventory[index];
            item.units += units;
        } else {
            self.inventory.push(Inventory::new(new_item, units));
        }

        self.units += units as i32;

        Ok(())
    }

    pub fn with_item_added(&self, new_item: TradeGoodSymbol, units: u32) -> Result<Cargo, NotEnoughSpaceError> {
        let mut cloned = self.clone();

        match cloned.with_item_added_mut(new_item, units) {
            Ok(_) => Ok(cloned),
            Err(e) => Err(e),
        }
    }

    pub fn with_units_removed(&self, trade_good_symbol: TradeGoodSymbol, units: u32) -> Result<Cargo, NotEnoughItemsInCargoError> {
        let mut cargo = self.clone();

        cargo.with_units_removed_mut(trade_good_symbol, units)?;
        Ok(cargo)
    }
    pub fn with_units_removed_mut(&mut self, trade_good_symbol: TradeGoodSymbol, units: u32) -> Result<(), NotEnoughItemsInCargoError> {
        // Find the index of the inventory item with the matching symbol
        let maybe_item_index = self
            .inventory
            .iter()
            .position(|item| item.symbol == trade_good_symbol);

        // If the item exists in inventory
        if let Some(index) = maybe_item_index {
            let item = &mut self.inventory[index];

            // Check if we have enough units to remove
            if item.units < units {
                return Err(NotEnoughItemsInCargoError {
                    required: units,
                    current: item.units,
                });
            }

            // Update the item units
            item.units -= units;

            // If units became 0, remove the item from inventory
            if item.units == 0 {
                self.inventory.remove(index);
            }

            // Update the total cargo units
            self.units -= units as i32;

            Ok(())
        } else {
            // Item not found in inventory
            Err(NotEnoughItemsInCargoError { required: units, current: 0 })
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub symbol: TradeGoodSymbol,
    pub units: u32,
}

impl Inventory {
    pub fn new(symbol: TradeGoodSymbol, units: u32) -> Self {
        Self { symbol, units }
    }
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
    pub data: SupplyChainMap,
}

#[derive(Deserialize, Serialize, Debug, Display, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
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

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Display, EnumIter)]
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

#[derive(Debug)]
pub struct NotEnoughItemsInCargoError {
    pub required: u32,
    pub current: u32,
}

#[derive(Debug)]
pub struct NotEnoughSpaceError {
    pub required: u32,
    pub available: u32,
}
