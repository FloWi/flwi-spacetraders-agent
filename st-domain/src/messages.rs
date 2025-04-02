use crate::{Agent, GetConstructionResponseData, MaterializedSupplyChain, Ship, ShipSymbol, ShipType, SystemSymbol, TradeGoodSymbol, WaypointSymbol};
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
pub enum PurchaseTicket {
    PurchaseCargoTicket {
        details: PurchaseGoodTicketDetails,
    },
    PurchaseShipTicket {
        details: PurchaseShipTicket,
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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PurchaseShipTicket {
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
    PurchaseShip { ticket: PurchaseShipTicket },

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

impl FleetPhase {
    pub fn calculate_budget_for_fleet(&self, agent: &Agent, fleet: &Fleet, fleets: &HashMap<FleetId, Fleet>) -> u64 {
        match self.name {
            FleetPhaseName::InitialExploration => 0,
            FleetPhaseName::ConstructJumpGate => match fleet.cfg {
                FleetConfig::ConstructJumpGateCfg(_) => {
                    if agent.credits < 0 {
                        0
                    } else {
                        agent.credits as u64
                    }
                }
                _ => 0,
            },
            FleetPhaseName::TradeProfitably => 0,
        }
    }
}
