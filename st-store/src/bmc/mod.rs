use crate::bmc::ship_bmc::{DbShipBmc, ShipBmcTrait};
use crate::shipyard_bmc::{DbShipyardBmc, ShipyardBmcTrait};
use crate::trade_bmc::{DbTradeBmc, TradeBmcTrait};
use crate::{
    AgentBmcTrait, ConstructionBmcTrait, DbAgentBmc, DbConstructionBmc, DbFleetBmc, DbMarketBmc, DbModelManager, DbSystemBmc, FleetBmcTrait, MarketBmcTrait,
    SystemBmcTrait,
};
use mockall::automock;
use std::fmt::Debug;
use std::sync::Arc;

pub mod ship_bmc;

#[automock]
pub trait Bmc: Send + Sync + Debug {
    fn ship_bmc(&self) -> Arc<dyn ShipBmcTrait>;
    fn fleet_bmc(&self) -> Arc<dyn FleetBmcTrait>;
    fn trade_bmc(&self) -> Arc<dyn TradeBmcTrait>;
    fn system_bmc(&self) -> Arc<dyn SystemBmcTrait>;
    fn agent_bmc(&self) -> Arc<dyn AgentBmcTrait>;
    fn construction_bmc(&self) -> Arc<dyn ConstructionBmcTrait>;
    fn market_bmc(&self) -> Arc<dyn MarketBmcTrait>;
    fn shipyard_bmc(&self) -> Arc<dyn ShipyardBmcTrait>;
}

#[derive(Debug, Clone)]
pub struct DbBmc {
    pub db_model_manager: DbModelManager,
    ship_bmc: Arc<DbShipBmc>,
    fleet_bmc: Arc<DbFleetBmc>,
    trade_bmc: Arc<DbTradeBmc>,
    system_bmc: Arc<DbSystemBmc>,
    agent_bmc: Arc<DbAgentBmc>,
    construction_bmc: Arc<DbConstructionBmc>,
    market_bmc: Arc<DbMarketBmc>,
    shipyard_bmc: Arc<DbShipyardBmc>,
}

impl DbBmc {
    pub fn new(mm: DbModelManager) -> Self {
        Self {
            db_model_manager: mm.clone(),
            ship_bmc: Arc::new(DbShipBmc { mm: mm.clone() }),
            fleet_bmc: Arc::new(DbFleetBmc { mm: mm.clone() }),
            trade_bmc: Arc::new(DbTradeBmc { mm: mm.clone() }),
            system_bmc: Arc::new(DbSystemBmc { mm: mm.clone() }),
            agent_bmc: Arc::new(DbAgentBmc { mm: mm.clone() }),
            construction_bmc: Arc::new(DbConstructionBmc { mm: mm.clone() }),
            market_bmc: Arc::new(DbMarketBmc { mm: mm.clone() }),
            shipyard_bmc: Arc::new(DbShipyardBmc { mm: mm.clone() }),
        }
    }
}

impl Bmc for DbBmc {
    fn ship_bmc(&self) -> Arc<dyn ShipBmcTrait> {
        self.ship_bmc.clone() as Arc<dyn ShipBmcTrait>
    }

    fn fleet_bmc(&self) -> Arc<dyn FleetBmcTrait> {
        self.fleet_bmc.clone() as Arc<dyn FleetBmcTrait>
    }

    fn trade_bmc(&self) -> Arc<dyn TradeBmcTrait> {
        self.trade_bmc.clone() as Arc<dyn TradeBmcTrait>
    }

    fn system_bmc(&self) -> Arc<dyn SystemBmcTrait> {
        self.system_bmc.clone() as Arc<dyn SystemBmcTrait>
    }

    fn agent_bmc(&self) -> Arc<dyn AgentBmcTrait> {
        self.agent_bmc.clone() as Arc<dyn AgentBmcTrait>
    }

    fn construction_bmc(&self) -> Arc<dyn ConstructionBmcTrait> {
        self.construction_bmc.clone() as Arc<dyn ConstructionBmcTrait>
    }

    fn market_bmc(&self) -> Arc<dyn MarketBmcTrait> {
        self.market_bmc.clone() as Arc<dyn MarketBmcTrait>
    }

    fn shipyard_bmc(&self) -> Arc<dyn ShipyardBmcTrait> {
        self.shipyard_bmc.clone() as Arc<dyn ShipyardBmcTrait>
    }
}
