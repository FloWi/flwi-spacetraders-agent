use crate::{
    Agent, EvaluatedTradingOpportunity, GetConstructionResponseData, MaterializedSupplyChain, Ship, ShipSymbol, ShipType, ShipyardShip, SystemSymbol,
    TradeGoodSymbol, WaypointSymbol,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use strum_macros::Display;
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
    pub construction_site: Option<GetConstructionResponseData>,
    pub ships: Vec<Ship>,
    pub materialized_supply_chain: Option<MaterializedSupplyChain>,
    pub agent_info: Agent,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum RefuelingType {
    RefuelDirectly,
    StoreFuelBarrelsInCargo,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum TradeTicket {
    TradeCargo {
        purchase_completion_status: Vec<(PurchaseGoodTicketDetails, bool)>,
        sale_completion_status: Vec<(SellGoodTicketDetails, bool)>,
        evaluation_result: Vec<EvaluatedTradingOpportunity>,
    },
    DeliverConstructionMaterials {
        purchase_completion_status: Vec<(PurchaseGoodTicketDetails, bool)>,
    },
    PurchaseShipTicket {
        details: PurchaseShipTicketDetails,
    },
    RefuelShip {
        details: PurchaseGoodTicketDetails,
        refueling_type: RefuelingType,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PurchaseGoodTicketDetails {
    pub ticket_id: Uuid,
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
            ticket_id: Uuid::new_v4(),
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
            ticket_id: Uuid::new_v4(),
            ship_symbol: opp.ship_symbol.clone(),
            waypoint_symbol: opp.trading_opportunity.purchase_waypoint_symbol.clone(),
            trade_good: sell_mtg.symbol.clone(),
            quantity: opp.units,
            price_per_unit: sell_mtg.sell_price as u64,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SellGoodTicketDetails {
    pub ticket_id: Uuid,
    pub ship_symbol: ShipSymbol,
    pub waypoint_symbol: WaypointSymbol,
    pub trade_good: TradeGoodSymbol,
    pub quantity: u32,
    pub price_per_unit: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PurchaseShipTicketDetails {
    pub ticket_id: Uuid,
    pub ship_symbol: ShipSymbol,
    pub waypoint_symbol: WaypointSymbol,
    pub ship_type: ShipType,
    pub price: u64,
    pub allocated_credits: u64,
    pub assigned_fleet_id: FleetId,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum PurchaseReason {
    Trading,
    ConstructionSiteSupply,
    ContractFulfilment,
    ShipUpgrade,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ShipTask {
    PurchaseShip { ticket: PurchaseShipTicketDetails },

    ObserveWaypointDetails { waypoint_symbol: WaypointSymbol },

    ObserveAllWaypointsOnce { waypoint_symbols: Vec<WaypointSymbol> },

    MineMaterialsAtWaypoint { mining_waypoint: WaypointSymbol },

    DeliverGoods { tickets: Vec<PurchaseGoodTicketDetails> },

    PurchaseGoods { purchase_tickets: Vec<PurchaseGoodTicketDetails> },

    SurveyAsteroid { waypoint_symbol: WaypointSymbol },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SystemSpawningFleetConfig {
    pub system_symbol: SystemSymbol,
    pub marketplace_waypoints_of_interest: Vec<WaypointSymbol>,
    pub shipyard_waypoints_of_interest: Vec<WaypointSymbol>,
    pub desired_fleet_config: Vec<ShipType>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MarketObservationFleetConfig {
    pub system_symbol: SystemSymbol,
    pub marketplace_waypoints_of_interest: Vec<WaypointSymbol>,
    pub shipyard_waypoints_of_interest: Vec<WaypointSymbol>,
    pub desired_fleet_config: Vec<ShipType>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TradingFleetConfig {
    pub system_symbol: SystemSymbol,
    pub materialized_supply_chain: Option<MaterializedSupplyChain>,
    pub desired_fleet_config: Vec<ShipType>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ConstructJumpGateFleetConfig {
    pub system_symbol: SystemSymbol,
    pub jump_gate_waypoint: WaypointSymbol,
    pub materialized_supply_chain: Option<MaterializedSupplyChain>,
    pub desired_fleet_config: Vec<ShipType>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MiningFleetConfig {
    pub system_symbol: SystemSymbol,
    pub mining_waypoint: WaypointSymbol,
    pub materials: Vec<TradeGoodSymbol>,
    pub delivery_locations: HashMap<TradeGoodSymbol, WaypointSymbol>,
    pub materialized_supply_chain: Option<MaterializedSupplyChain>,
    pub desired_fleet_config: Vec<ShipType>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SiphoningFleetConfig {
    pub system_symbol: SystemSymbol,
    pub siphoning_waypoint: WaypointSymbol,
    pub materials: Vec<TradeGoodSymbol>,
    pub delivery_locations: HashMap<TradeGoodSymbol, WaypointSymbol>,
    pub materialized_supply_chain: Option<MaterializedSupplyChain>,
    pub desired_fleet_config: Vec<ShipType>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
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

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct FleetTaskCompletion {
    pub task: FleetTask,
    pub completed_at: DateTime<Utc>,
}

#[derive(Deserialize, Serialize, Debug, Display, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum FleetTask {
    CollectMarketInfosOnce { system_symbol: SystemSymbol },
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
}
