use crate::{GetConstructionResponseData, MaterializedSupplyChain, Ship, ShipSymbol, ShipType, SystemSymbol, TradeGoodSymbol, WaypointSymbol};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use strum_macros::Display;

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
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ShipTask {
    PurchaseShip {
        r#type: ShipType,
        max_amount: u32,
        waypoint_symbol: WaypointSymbol,
    },

    ObserveWaypointDetails {
        waypoint_symbol: WaypointSymbol,
    },

    ObserveAllWaypointsOnce {
        waypoint_symbols: Vec<WaypointSymbol>,
    },

    MineMaterialsAtWaypoint {
        mining_waypoint: WaypointSymbol,
    },

    DeliverMaterials {
        delivery_locations: HashMap<TradeGoodSymbol, WaypointSymbol>,
    },

    SurveyAsteroid {
        waypoint_symbol: WaypointSymbol,
    },
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
