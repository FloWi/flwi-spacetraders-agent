use crate::budgeting::budgeting::{PurchaseShipTransactionGoal, PurchaseTradeGoodsTransactionGoal, SellTradeGoodsTransactionGoal, TransactionTicket};
use crate::{
    Agent, Construction, EvaluatedTradingOpportunity, FlightMode, JumpGate, MarketData, MaterializedSupplyChain, PurchaseShipResponse,
    PurchaseTradeGoodResponse, RefuelShipResponse, SellTradeGoodResponse, Ship, ShipSymbol, ShipType, Shipyard, ShipyardShip, SupplyConstructionSiteResponse,
    SystemSymbol, TradeGoodSymbol, WaypointSymbol,
};
use chrono::{DateTime, Utc};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use strum::Display;
use uuid::Uuid;

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum FleetUpdateMessage {
    FleetTaskCompleted {
        fleet_task_completion: FleetTaskCompletion,
        fleet_id: FleetId,
    },
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum ShipTaskMessage {
    ObservedMarketplace(WaypointSymbol),
    ObservedShipyard(WaypointSymbol),
    ObservedJumpGate(WaypointSymbol),
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, Eq, PartialEq)]
pub enum ShipRole {
    MarketObserver,
    ShipPurchaser,
    MiningSurveyor,
    Miner,
    MiningHauler,
    Siphoner,
    SiphoningHauler,
    Trader,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FleetDecisionFacts {
    pub marketplaces_of_interest: Vec<WaypointSymbol>,
    pub marketplaces_with_up_to_date_infos: Vec<WaypointSymbol>,
    pub shipyards_of_interest: Vec<WaypointSymbol>,
    pub shipyards_with_up_to_date_infos: Vec<WaypointSymbol>,
    pub construction_site: Option<Construction>,
    pub ships: Vec<Ship>,
    pub materialized_supply_chain: Option<MaterializedSupplyChain>,
    pub agent_info: Agent,
}

impl FleetDecisionFacts {
    pub fn missing_construction_materials(&self) -> HashMap<TradeGoodSymbol, u32> {
        if let Some(construction_site) = self.construction_site.clone() {
            if construction_site.is_complete {
                Default::default()
            } else {
                construction_site
                    .materials
                    .iter()
                    .filter_map(|cm| {
                        let missing = cm.required - cm.fulfilled;
                        (missing >= 0).then(|| (cm.trade_symbol.clone(), missing))
                    })
                    .collect()
            }
        } else {
            Default::default()
        }
    }

    pub fn all_construction_materials(&self) -> HashMap<TradeGoodSymbol, u32> {
        if let Some(construction_site) = self.construction_site.clone() {
            construction_site.materials.iter().map(|cm| (cm.trade_symbol.clone(), cm.required)).collect()
        } else {
            Default::default()
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum RefuelingType {
    RefuelDirectly,
    StoreFuelBarrelsInCargo,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, Hash, Copy)]
pub struct TicketId(pub Uuid);

impl Default for TicketId {
    fn default() -> Self {
        Self::new()
    }
}

impl TicketId {
    pub fn new() -> TicketId {
        Self(Uuid::new_v4())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum TradeTicket {
    TradeCargo {
        ticket_id: TicketId,
        purchase_completion_status: Vec<(PurchaseGoodTicketDetails, bool)>,
        sale_completion_status: Vec<(SellGoodTicketDetails, bool)>,
        evaluation_result: Vec<EvaluatedTradingOpportunity>,
    },
    DeliverConstructionMaterials {
        ticket_id: TicketId,
        purchase_completion_status: Vec<(PurchaseGoodTicketDetails, bool)>,
        delivery_status: Vec<(DeliverConstructionMaterialTicketDetails, bool)>,
    },
    PurchaseShipTicket {
        ticket_id: TicketId,
        details: PurchaseShipTicketDetails,
    },
    // RefuelShip {
    //     details: PurchaseGoodTicketDetails,
    //     refueling_type: RefuelingType,
    // },
}

impl TradeTicket {
    pub fn total_costs(&self) -> u64 {
        todo!()
    }
}

impl TradeTicket {
    pub fn ticket_id(&self) -> TicketId {
        match self {
            TradeTicket::TradeCargo { ticket_id, .. } => ticket_id.clone(),
            TradeTicket::DeliverConstructionMaterials { ticket_id, .. } => ticket_id.clone(),
            TradeTicket::PurchaseShipTicket { ticket_id, .. } => ticket_id.clone(),
        }
    }
    pub fn is_complete(&self) -> bool {
        match self {
            TradeTicket::TradeCargo {
                ticket_id,
                purchase_completion_status,
                sale_completion_status,
                evaluation_result: _evaluation_result,
            } => purchase_completion_status.iter().all(|(_, is_complete)| *is_complete) && sale_completion_status.iter().all(|(_, is_complete)| *is_complete),
            TradeTicket::DeliverConstructionMaterials {
                ticket_id,
                purchase_completion_status,
                delivery_status,
            } => purchase_completion_status.iter().all(|(_, is_complete)| *is_complete) && delivery_status.iter().all(|(_, is_complete)| *is_complete),
            TradeTicket::PurchaseShipTicket { ticket_id, details } => details.is_complete,
        }
    }
}

impl TradeTicket {
    pub fn mark_transaction_as_complete(&mut self, transaction_ticket_id: &TransactionTicketId) {
        match self {
            TradeTicket::TradeCargo {
                purchase_completion_status,
                sale_completion_status,
                ..
            } => {
                // Try to find and mark a purchase as complete
                for (purchase_details, completed) in purchase_completion_status.iter_mut() {
                    if &purchase_details.id == transaction_ticket_id {
                        *completed = true;
                        return;
                    }
                }

                // If not found in purchases, try to find in sales
                for (sale_details, completed) in sale_completion_status.iter_mut() {
                    if &sale_details.id == transaction_ticket_id {
                        *completed = true;
                        return;
                    }
                }
            }
            TradeTicket::DeliverConstructionMaterials {
                purchase_completion_status,
                delivery_status,
                ..
            } => {
                // Try to find and mark a purchase as complete
                for (purchase_details, completed) in purchase_completion_status.iter_mut() {
                    if &purchase_details.id == transaction_ticket_id {
                        *completed = true;
                        return;
                    }
                }

                // Try to find and mark a delivery as complete
                for (delivery_details, completed) in delivery_status.iter_mut() {
                    if &delivery_details.id == transaction_ticket_id {
                        *completed = true;
                        return;
                    }
                }
            }
            TradeTicket::PurchaseShipTicket { details, .. } => {
                // Assuming PurchaseShipTicketDetails has a transaction_id field
                if &details.id == transaction_ticket_id {
                    // You might need to add a field to track completion status
                    // details.completed = true;
                    details.is_complete = true
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct TransactionTicketId(pub Uuid);

impl TransactionTicketId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PurchaseGoodTicketDetails {
    pub id: TransactionTicketId,
    pub ship_symbol: ShipSymbol,
    pub waypoint_symbol: WaypointSymbol,
    pub trade_good: TradeGoodSymbol,
    pub quantity: u32,
    pub price_per_unit: u64,
    pub allocated_credits: u64,
    pub purchase_reason: PurchaseReason,
}

impl PurchaseGoodTicketDetails {
    pub fn from_trading_opportunity(opp: &EvaluatedTradingOpportunity) -> PurchaseGoodTicketDetails {
        let purchase_mtg = &opp.trading_opportunity.purchase_market_trade_good_entry;
        PurchaseGoodTicketDetails {
            id: TransactionTicketId(Uuid::new_v4()),
            ship_symbol: opp.ship_symbol.clone(),
            waypoint_symbol: opp.trading_opportunity.purchase_waypoint_symbol.clone(),
            trade_good: purchase_mtg.symbol.clone(),
            quantity: opp.units,
            price_per_unit: purchase_mtg.purchase_price as u64,
            allocated_credits: purchase_mtg.purchase_price as u64 * opp.units as u64,
            purchase_reason: PurchaseReason::Trading,
        }
    }
}

impl SellGoodTicketDetails {
    pub fn from_trading_opportunity(opp: &EvaluatedTradingOpportunity) -> SellGoodTicketDetails {
        let sell_mtg = &opp.trading_opportunity.sell_market_trade_good_entry;

        SellGoodTicketDetails {
            id: TransactionTicketId(Uuid::new_v4()),
            ship_symbol: opp.ship_symbol.clone(),
            waypoint_symbol: opp.trading_opportunity.sell_waypoint_symbol.clone(),
            trade_good: sell_mtg.symbol.clone(),
            quantity: opp.units,
            price_per_unit: sell_mtg.sell_price as u64,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SellGoodTicketDetails {
    pub id: TransactionTicketId,
    pub ship_symbol: ShipSymbol,
    pub waypoint_symbol: WaypointSymbol,
    pub trade_good: TradeGoodSymbol,
    pub quantity: u32,
    pub price_per_unit: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DeliverConstructionMaterialTicketDetails {
    pub id: TransactionTicketId,
    pub ship_symbol: ShipSymbol,
    pub construction_site_waypoint_symbol: WaypointSymbol,
    pub trade_good: TradeGoodSymbol,
    pub quantity: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PurchaseShipTicketDetails {
    pub id: TransactionTicketId,
    pub ship_symbol: ShipSymbol,
    pub waypoint_symbol: WaypointSymbol,
    pub ship_type: ShipType,
    pub price: u64,
    pub allocated_credits: u64,
    pub assigned_fleet_id: FleetId,
    pub is_complete: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum PurchaseReason {
    Trading,
    ConstructionSiteSupply,
    ContractFulfilment,
    ShipUpgrade,
}

#[derive(Serialize, Deserialize, Clone, Debug, Display)]
pub enum ShipTask {
    ObserveWaypointDetails { waypoint_symbol: WaypointSymbol },

    ObserveAllWaypointsOnce { waypoint_symbols: Vec<WaypointSymbol> },

    MineMaterialsAtWaypoint { mining_waypoint: WaypointSymbol },

    SurveyAsteroid { waypoint_symbol: WaypointSymbol },

    Trade { ticket_id: TicketId },

    PrepositionShipForTrade { first_purchase_location: WaypointSymbol },
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SystemSpawningFleetConfig {
    pub system_symbol: SystemSymbol,
    pub marketplace_waypoints_of_interest: Vec<WaypointSymbol>,
    pub shipyard_waypoints_of_interest: Vec<WaypointSymbol>,
    pub desired_fleet_config: Vec<ShipType>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct MarketObservationFleetConfig {
    pub system_symbol: SystemSymbol,
    pub marketplace_waypoints_of_interest: Vec<WaypointSymbol>,
    pub shipyard_waypoints_of_interest: Vec<WaypointSymbol>,
    pub desired_fleet_config: Vec<ShipType>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct TradingFleetConfig {
    pub system_symbol: SystemSymbol,
    pub materialized_supply_chain: Option<MaterializedSupplyChain>,
    pub desired_fleet_config: Vec<ShipType>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct ConstructJumpGateFleetConfig {
    pub system_symbol: SystemSymbol,
    pub jump_gate_waypoint: WaypointSymbol,
    pub materialized_supply_chain: Option<MaterializedSupplyChain>,
    pub desired_fleet_config: Vec<ShipType>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct MiningFleetConfig {
    pub system_symbol: SystemSymbol,
    pub mining_waypoint: WaypointSymbol,
    pub materials: Vec<TradeGoodSymbol>,
    pub delivery_locations: HashMap<TradeGoodSymbol, WaypointSymbol>,
    pub materialized_supply_chain: Option<MaterializedSupplyChain>,
    pub desired_fleet_config: Vec<ShipType>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SiphoningFleetConfig {
    pub system_symbol: SystemSymbol,
    pub siphoning_waypoint: WaypointSymbol,
    pub materials: Vec<TradeGoodSymbol>,
    pub delivery_locations: HashMap<TradeGoodSymbol, WaypointSymbol>,
    pub materialized_supply_chain: Option<MaterializedSupplyChain>,
    pub desired_fleet_config: Vec<ShipType>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, Display)]
pub enum FleetConfig {
    SystemSpawningCfg(SystemSpawningFleetConfig),
    MarketObservationCfg(MarketObservationFleetConfig),
    TradingCfg(TradingFleetConfig),
    ConstructJumpGateCfg(ConstructJumpGateFleetConfig),
    MiningCfg(MiningFleetConfig),
    SiphoningCfg(SiphoningFleetConfig),
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct FleetId(pub i32);

impl Display for FleetId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct FleetTaskCompletion {
    pub task: FleetTask,
    pub completed_at: DateTime<Utc>,
}

#[derive(Deserialize, Serialize, Debug, Display, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum FleetTask {
    InitialExploration { system_symbol: SystemSymbol },
    ObserveAllWaypointsOfSystemWithStationaryProbes { system_symbol: SystemSymbol },
    ConstructJumpGate { system_symbol: SystemSymbol },
    TradeProfitably { system_symbol: SystemSymbol },
    MineOres { system_symbol: SystemSymbol },
    SiphonGases { system_symbol: SystemSymbol },
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Fleet {
    pub id: FleetId,
    pub cfg: FleetConfig,
}

/// Deep copy of fleet admiral state for serde-compatibility
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct FleetsOverview {
    pub completed_fleet_tasks: Vec<FleetTaskCompletion>,
    pub fleets: HashMap<FleetId, Fleet>,
    pub all_ships: HashMap<ShipSymbol, Ship>,
    pub fleet_task_assignments: HashMap<FleetId, Vec<FleetTask>>,
    pub ship_fleet_assignment: HashMap<ShipSymbol, FleetId>,
    pub ship_tasks: HashMap<ShipSymbol, ShipTask>,
    pub open_trade_tickets: HashMap<ShipSymbol, TransactionTicket>,
    pub stationary_probe_locations: Vec<StationaryProbeLocation>,
}

#[derive(Deserialize, Serialize, Debug, Display, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum FleetPhaseName {
    InitialExploration,
    ConstructJumpGate,
    TradeProfitably,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct FleetPhase {
    pub name: FleetPhaseName,
    pub shopping_list_in_order: Vec<(ShipType, FleetTask)>,
    pub tasks: Vec<FleetTask>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ShipPriceInfo {
    pub price_infos: Vec<(WaypointSymbol, Vec<ShipyardShip>)>,
    pub latest_shipyard_infos: Vec<ShipyardData>,
}

impl ShipPriceInfo {
    pub fn guess_price_for_ship(ship_type: &ShipType) -> Option<u32> {
        match ship_type {
            ShipType::SHIP_PROBE => Some(25_000),
            ShipType::SHIP_LIGHT_HAULER => Some(277_000),
            ShipType::SHIP_LIGHT_SHUTTLE => Some(90_000),
            ShipType::SHIP_SIPHON_DRONE => Some(40_000),
            ShipType::SHIP_MINING_DRONE => Some(42_000),
            ShipType::SHIP_SURVEYOR => Some(30_000),
            ShipType::SHIP_INTERCEPTOR => None,
            ShipType::SHIP_COMMAND_FRIGATE => None,
            ShipType::SHIP_EXPLORER => None,
            ShipType::SHIP_HEAVY_FREIGHTER => None,
            ShipType::SHIP_ORE_HOUND => None,
            ShipType::SHIP_REFINING_FREIGHTER => None,
            ShipType::SHIP_BULK_FREIGHTER => None,
        }
    }
    pub fn compute_ship_type_purchase_location_map(&self) -> HashMap<ShipType, Vec<WaypointSymbol>> {
        self.latest_shipyard_infos
            .iter()
            .flat_map(|shipyard_data| shipyard_data.shipyard.ship_types.iter().map(|st| (st.r#type, shipyard_data.waypoint_symbol.clone())))
            .into_group_map_by(|(st, _)| *st)
            .into_iter()
            .map(|(k, values)| (k, values.into_iter().map(|tup| tup.1).collect_vec()))
            .collect()
    }

    pub fn compute_ship_type_purchase_price_map(&self) -> HashMap<ShipType, Vec<(WaypointSymbol, u32)>> {
        self.price_infos
            .iter()
            .flat_map(|(wps, shipyard_ships)| shipyard_ships.iter().map(|shipyard_ship| (shipyard_ship.r#type, (wps.clone(), shipyard_ship.purchase_price))))
            .into_group_map_by(|(st, _)| *st)
            .into_iter()
            .map(|(k, values)| (k, values.into_iter().map(|tup| tup.1).collect_vec()))
            .collect()
    }

    fn get_price_and_location(
        ship_type: &ShipType,
        purchase_price_map: &HashMap<ShipType, Vec<(WaypointSymbol, u32)>>,
        purchase_location_map: &HashMap<ShipType, Vec<WaypointSymbol>>,
    ) -> Option<(WaypointSymbol, u32)> {
        purchase_price_map.get(ship_type).and_then(|prices| prices.iter().min_by_key(|(wps, p)| p)).cloned().or_else(|| {
            purchase_location_map
                .get(ship_type)
                .and_then(|waypoints| waypoints.first().and_then(|wps| Self::guess_price_for_ship(ship_type).map(|guessed_price| (wps.clone(), guessed_price))))
        })
    }

    pub fn get_best_purchase_location(&self, ship_type: &ShipType) -> Option<(ShipType, (WaypointSymbol, u32))> {
        let purchase_price_map: HashMap<ShipType, Vec<(WaypointSymbol, u32)>> = self.compute_ship_type_purchase_price_map();
        let purchase_location_map: HashMap<ShipType, Vec<WaypointSymbol>> = self.compute_ship_type_purchase_location_map();
        Self::get_price_and_location(ship_type, &purchase_price_map, &purchase_location_map)
            .map(|(wps, p)| (ship_type.clone(), wps, p))
            .map(|(st, wps, p)| (st, (wps, p)))
    }

    pub fn get_running_total_of_all_ship_purchases(&self, shopping_list: Vec<ShipType>) -> Vec<(ShipType, WaypointSymbol, u32, u32)> {
        let purchase_price_map: HashMap<ShipType, Vec<(WaypointSymbol, u32)>> = self.compute_ship_type_purchase_price_map();
        let purchase_location_map: HashMap<ShipType, Vec<WaypointSymbol>> = self.compute_ship_type_purchase_location_map();

        let purchase_locations: HashMap<_, _> = shopping_list
            .iter()
            .unique()
            .filter_map(|ship_type| {
                Self::get_price_and_location(ship_type, &purchase_price_map, &purchase_location_map)
                    .map(|(wps, p)| (ship_type.clone(), wps, p))
                    .map(|(st, wps, p)| (st, (wps, p)))
            })
            .collect();

        shopping_list
            .into_iter()
            .map(|ship_type| {
                let (wps, price) = purchase_locations.get(&ship_type).unwrap();
                (ship_type, wps, price)
            })
            .scan(0, |acc, (ship_type, wps, price)| {
                *acc += price;
                Some((ship_type, wps, price, *acc))
            })
            .map(|(ship_type, wps, price, running_total)| (ship_type, wps.clone(), *price, running_total))
            .collect()
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Display)]
pub enum TransactionActionEvent {
    PurchasedTradeGoods {
        ticket_details: PurchaseTradeGoodsTransactionGoal,
        response: PurchaseTradeGoodResponse,
    },
    SoldTradeGoods {
        ticket_details: SellTradeGoodsTransactionGoal,
        response: SellTradeGoodResponse,
    },
    SuppliedConstructionSite {
        ticket_details: DeliverConstructionMaterialTicketDetails,
        response: SupplyConstructionSiteResponse,
    },
    PurchasedShip {
        ticket_details: PurchaseShipTransactionGoal,
        response: PurchaseShipResponse,
    },
}

#[derive(Deserialize, Serialize, Debug, Clone, Display)]
pub enum OperationExpenseEvent {
    RefueledShip { response: RefuelShipResponse },
}

impl TransactionActionEvent {
    pub fn maybe_updated_agent_credits(&self) -> Option<i64> {
        match self {
            TransactionActionEvent::PurchasedTradeGoods {
                ticket_details: _,
                response: resp,
            } => Some(resp.data.agent.credits),
            TransactionActionEvent::SoldTradeGoods {
                ticket_details: _,
                response: resp,
            } => Some(resp.data.agent.credits),
            TransactionActionEvent::SuppliedConstructionSite {
                ticket_details: _,
                response: resp,
            } => None,
            TransactionActionEvent::PurchasedShip {
                ticket_details: _,
                response: resp,
            } => Some(resp.data.agent.credits),
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct TransactionSummary {
    pub ship_symbol: ShipSymbol,
    pub transaction_action_event: TransactionActionEvent,
    pub trade_ticket: TransactionTicket,
    pub total_price: i64,
    pub transaction_ticket_id: TransactionTicketId,
}

/// What observation to do once a ship is present at this waypoint
#[derive(Eq, PartialEq, Clone, Debug, Display, Serialize, Deserialize)]
pub enum ExplorationTask {
    GetMarket,
    GetJumpGate,
    CreateChart,
    GetShipyard,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct StationaryProbeLocation {
    pub waypoint_symbol: WaypointSymbol,
    pub probe_ship_symbol: ShipSymbol,
    pub exploration_tasks: Vec<ExplorationTask>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Hash, PartialEq, Eq, Display)]
pub enum TravelAction {
    Navigate {
        from: WaypointSymbol,
        to: WaypointSymbol,
        distance: u32,
        travel_time: u32,
        fuel_consumption: u32,
        mode: FlightMode,
        total_time: u32,
    },
    Refuel {
        at: WaypointSymbol,
        total_time: u32,
    },
}

impl TravelAction {
    pub fn total_time(&self) -> u32 {
        match self {
            TravelAction::Navigate { total_time, .. } => *total_time,
            TravelAction::Refuel { total_time, .. } => *total_time,
        }
    }

    pub fn waypoint_and_time(&self) -> (&WaypointSymbol, &u32) {
        match self {
            TravelAction::Navigate { to, total_time, .. } => (to, total_time),
            TravelAction::Refuel { at, total_time } => (at, total_time),
        }
    }
}

#[derive(Serialize, Clone, Debug, Deserialize)]
pub struct MarketEntry {
    pub waypoint_symbol: WaypointSymbol,
    pub market_data: MarketData,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, Clone, Debug, Deserialize)]
pub struct ShipyardData {
    pub waypoint_symbol: WaypointSymbol,
    pub shipyard: Shipyard,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, Clone, Debug, Deserialize)]
pub struct JumpGateEntry {
    pub system_symbol: SystemSymbol,
    pub waypoint_symbol: WaypointSymbol,
    pub jump_gate: JumpGate,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
