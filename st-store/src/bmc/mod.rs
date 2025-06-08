use crate::bmc::contract_bmc::{ContractBmcTrait, DbContractBmc, InMemoryContractBmc};
use crate::bmc::jump_gate_bmc::{DbJumpGateBmc, InMemoryJumpGateBmc, JumpGateBmcTrait};
use crate::bmc::ship_bmc::{DbShipBmc, InMemoryShipsBmc, ShipBmcTrait};
use crate::ledger_bmc::{DbLedgerBmc, InMemoryLedgerBmc, LedgerBmcTrait};
use crate::shipyard_bmc::{DbShipyardBmc, InMemoryShipyardBmc, ShipyardBmcTrait};
use crate::survey_bmc::{DbSurveyBmc, InMemorySurveyBmc, SurveyBmcTrait};
use crate::trade_bmc::{DbTradeBmc, InMemoryTradeBmc, TradeBmcTrait};
use crate::{
    AgentBmcTrait, ConstructionBmcTrait, DbAgentBmc, DbConstructionBmc, DbFleetBmc, DbMarketBmc, DbModelManager, DbStatusBmc, DbSupplyChainBmc, DbSystemBmc,
    FleetBmcTrait, InMemoryAgentBmc, InMemoryConstructionBmc, InMemoryFleetBmc, InMemoryMarketBmc, InMemoryStatusBmc, InMemorySupplyChainBmc,
    InMemorySystemsBmc, MarketBmcTrait, StatusBmcTrait, SupplyChainBmcTrait, SystemBmcTrait,
};
use mockall::automock;
use std::fmt::Debug;
use std::sync::Arc;

pub mod contract_bmc;
pub mod jump_gate_bmc;
pub mod ship_bmc;

#[automock]
pub trait Bmc: Send + Sync + Debug {
    fn ship_bmc(&self) -> Arc<dyn ShipBmcTrait>;
    fn fleet_bmc(&self) -> Arc<dyn FleetBmcTrait>;
    fn trade_bmc(&self) -> Arc<dyn TradeBmcTrait>;
    fn system_bmc(&self) -> Arc<dyn SystemBmcTrait>;
    fn agent_bmc(&self) -> Arc<dyn AgentBmcTrait>;
    fn construction_bmc(&self) -> Arc<dyn ConstructionBmcTrait>;
    fn survey_bmc(&self) -> Arc<dyn SurveyBmcTrait>;
    fn market_bmc(&self) -> Arc<dyn MarketBmcTrait>;
    fn jump_gate_bmc(&self) -> Arc<dyn JumpGateBmcTrait>;
    fn shipyard_bmc(&self) -> Arc<dyn ShipyardBmcTrait>;
    fn supply_chain_bmc(&self) -> Arc<dyn SupplyChainBmcTrait>;
    fn status_bmc(&self) -> Arc<dyn StatusBmcTrait>;
    fn ledger_bmc(&self) -> Arc<dyn LedgerBmcTrait>;
    fn contract_bmc(&self) -> Arc<dyn ContractBmcTrait>;
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
    survey_bmc: Arc<DbSurveyBmc>,
    market_bmc: Arc<DbMarketBmc>,
    jump_gate_bmc: Arc<DbJumpGateBmc>,
    shipyard_bmc: Arc<DbShipyardBmc>,
    supply_chain_bmc: Arc<DbSupplyChainBmc>,
    status_bmc: Arc<DbStatusBmc>,
    ledger_bmc: Arc<DbLedgerBmc>,
    contract_bmc: Arc<DbContractBmc>,
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
            survey_bmc: Arc::new(DbSurveyBmc { mm: mm.clone() }),
            market_bmc: Arc::new(DbMarketBmc { mm: mm.clone() }),
            jump_gate_bmc: Arc::new(DbJumpGateBmc { mm: mm.clone() }),
            shipyard_bmc: Arc::new(DbShipyardBmc { mm: mm.clone() }),
            supply_chain_bmc: Arc::new(DbSupplyChainBmc { mm: mm.clone() }),
            status_bmc: Arc::new(DbStatusBmc { mm: mm.clone() }),
            ledger_bmc: Arc::new(DbLedgerBmc { mm: mm.clone() }),
            contract_bmc: Arc::new(DbContractBmc { mm: mm.clone() }),
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

    fn survey_bmc(&self) -> Arc<dyn SurveyBmcTrait> {
        self.survey_bmc.clone() as Arc<dyn SurveyBmcTrait>
    }

    fn market_bmc(&self) -> Arc<dyn MarketBmcTrait> {
        self.market_bmc.clone() as Arc<dyn MarketBmcTrait>
    }

    fn jump_gate_bmc(&self) -> Arc<dyn JumpGateBmcTrait> {
        self.jump_gate_bmc.clone() as Arc<dyn JumpGateBmcTrait>
    }

    fn shipyard_bmc(&self) -> Arc<dyn ShipyardBmcTrait> {
        self.shipyard_bmc.clone() as Arc<dyn ShipyardBmcTrait>
    }

    fn supply_chain_bmc(&self) -> Arc<dyn SupplyChainBmcTrait> {
        self.supply_chain_bmc.clone() as Arc<dyn SupplyChainBmcTrait>
    }

    fn status_bmc(&self) -> Arc<dyn StatusBmcTrait> {
        todo!()
    }

    fn ledger_bmc(&self) -> Arc<dyn LedgerBmcTrait> {
        self.ledger_bmc.clone() as Arc<dyn LedgerBmcTrait>
    }

    fn contract_bmc(&self) -> Arc<dyn ContractBmcTrait> {
        self.contract_bmc.clone() as Arc<dyn ContractBmcTrait>
    }
}

#[derive(Debug)]
pub struct InMemoryBmc {
    pub in_mem_ship_bmc: Arc<InMemoryShipsBmc>,
    pub in_mem_fleet_bmc: Arc<InMemoryFleetBmc>,
    pub in_mem_trade_bmc: Arc<InMemoryTradeBmc>,
    pub in_mem_system_bmc: Arc<InMemorySystemsBmc>,
    pub in_mem_agent_bmc: Arc<InMemoryAgentBmc>,
    pub in_mem_construction_bmc: Arc<InMemoryConstructionBmc>,
    pub in_mem_survey_bmc: Arc<InMemorySurveyBmc>,
    pub in_mem_market_bmc: Arc<InMemoryMarketBmc>,
    pub in_mem_jump_gate_bmc: Arc<InMemoryJumpGateBmc>,
    pub in_mem_shipyard_bmc: Arc<InMemoryShipyardBmc>,
    pub in_mem_supply_chain_bmc: Arc<InMemorySupplyChainBmc>,
    pub in_mem_status_bmc: Arc<InMemoryStatusBmc>,
    pub in_mem_ledger_bmc: Arc<InMemoryLedgerBmc>,
    pub in_mem_contract_bmc: Arc<InMemoryContractBmc>,
}

impl Bmc for InMemoryBmc {
    fn ship_bmc(&self) -> Arc<dyn ShipBmcTrait> {
        Arc::clone(&self.in_mem_ship_bmc) as Arc<dyn ShipBmcTrait>
    }

    fn fleet_bmc(&self) -> Arc<dyn FleetBmcTrait> {
        Arc::clone(&self.in_mem_fleet_bmc) as Arc<dyn FleetBmcTrait>
    }

    fn trade_bmc(&self) -> Arc<dyn TradeBmcTrait> {
        Arc::clone(&self.in_mem_trade_bmc) as Arc<dyn TradeBmcTrait>
    }

    fn system_bmc(&self) -> Arc<dyn SystemBmcTrait> {
        Arc::clone(&self.in_mem_system_bmc) as Arc<dyn SystemBmcTrait>
    }

    fn agent_bmc(&self) -> Arc<dyn AgentBmcTrait> {
        Arc::clone(&self.in_mem_agent_bmc) as Arc<dyn AgentBmcTrait>
    }

    fn construction_bmc(&self) -> Arc<dyn ConstructionBmcTrait> {
        Arc::clone(&self.in_mem_construction_bmc) as Arc<dyn ConstructionBmcTrait>
    }

    fn survey_bmc(&self) -> Arc<dyn SurveyBmcTrait> {
        Arc::clone(&self.in_mem_survey_bmc) as Arc<dyn SurveyBmcTrait>
    }

    fn market_bmc(&self) -> Arc<dyn MarketBmcTrait> {
        Arc::clone(&self.in_mem_market_bmc) as Arc<dyn MarketBmcTrait>
    }

    fn jump_gate_bmc(&self) -> Arc<dyn JumpGateBmcTrait> {
        Arc::clone(&self.in_mem_jump_gate_bmc) as Arc<dyn JumpGateBmcTrait>
    }

    fn shipyard_bmc(&self) -> Arc<dyn ShipyardBmcTrait> {
        Arc::clone(&self.in_mem_shipyard_bmc) as Arc<dyn ShipyardBmcTrait>
    }

    fn supply_chain_bmc(&self) -> Arc<dyn SupplyChainBmcTrait> {
        Arc::clone(&self.in_mem_supply_chain_bmc) as Arc<dyn SupplyChainBmcTrait>
    }

    fn status_bmc(&self) -> Arc<dyn StatusBmcTrait> {
        Arc::clone(&self.in_mem_status_bmc) as Arc<dyn StatusBmcTrait>
    }

    fn ledger_bmc(&self) -> Arc<dyn LedgerBmcTrait> {
        Arc::clone(&self.in_mem_ledger_bmc) as Arc<dyn LedgerBmcTrait>
    }

    fn contract_bmc(&self) -> Arc<dyn ContractBmcTrait> {
        Arc::clone(&self.in_mem_contract_bmc) as Arc<dyn ContractBmcTrait>
    }
}
