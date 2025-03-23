use crate::{GetConstructionResponseData, MaterializedSupplyChain, Ship, SystemSymbol, WaypointSymbol};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum FleetUpdateMessage {}

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
pub enum FleetTask {
    CollectMarketInfosOnce { system_symbol: SystemSymbol }, // will be done by SystemSpawningFleet
    ObserveAllWaypointsOfSystemWithProbes { system_symbol: SystemSymbol }, // will be done by MarketObservationFleet
    ConstructJumpGate { system_symbol: SystemSymbol },      // will be done by TradingFleet
    TradeProfitably { system_symbol: SystemSymbol },
    MineOres { system_symbol: SystemSymbol },
    SiphonGases { system_symbol: SystemSymbol },
}
