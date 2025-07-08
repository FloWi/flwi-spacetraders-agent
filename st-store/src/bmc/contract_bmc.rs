use crate::{db, Ctx, DbModelManager};
use anyhow::*;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use st_domain::{Contract, ContractId, SystemSymbol};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::RwLock;

#[async_trait]
pub trait ContractBmcTrait: Send + Sync + Debug {
    async fn upsert_contract(&self, ctx: &Ctx, system_symbol: &SystemSymbol, contract: Contract, now: DateTime<Utc>) -> Result<()>;
    async fn get_youngest_contract(&self, ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Option<Contract>>;
}

#[derive(Debug)]
pub struct DbContractBmc {
    pub(crate) mm: DbModelManager,
}

#[async_trait]
impl ContractBmcTrait for DbContractBmc {
    async fn upsert_contract(&self, _ctx: &Ctx, system_symbol: &SystemSymbol, contract: Contract, now: DateTime<Utc>) -> Result<()> {
        db::upsert_contract(self.mm.pool(), system_symbol, &contract, now).await
    }

    async fn get_youngest_contract(&self, _ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Option<Contract>> {
        db::get_youngest_contract(self.mm.pool(), system_symbol).await
    }
}

#[derive(Debug)]
pub struct InMemoryContracts {
    contracts: HashMap<SystemSymbol, HashMap<ContractId, (Contract, DateTime<Utc>)>>,
}

impl InMemoryContracts {
    fn new() -> Self {
        Self { contracts: Default::default() }
    }
}

#[derive(Debug)]
pub struct InMemoryContractBmc {
    in_memory_contracts: Arc<RwLock<InMemoryContracts>>,
}

impl Default for InMemoryContractBmc {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryContractBmc {
    pub fn new() -> Self {
        Self {
            in_memory_contracts: Arc::new(RwLock::new(InMemoryContracts::new())),
        }
    }
}

#[async_trait]
impl ContractBmcTrait for InMemoryContractBmc {
    async fn upsert_contract(&self, _ctx: &Ctx, system_symbol: &SystemSymbol, contract: Contract, now: DateTime<Utc>) -> Result<()> {
        let mut guard = self.in_memory_contracts.write().await;

        guard
            .contracts
            .entry(system_symbol.clone())
            .or_default()
            .insert(contract.id.clone(), (contract.clone(), now));

        Ok(())
    }

    async fn get_youngest_contract(&self, _ctx: &Ctx, system_symbol: &SystemSymbol) -> Result<Option<Contract>> {
        let guard = self.in_memory_contracts.read().await;

        let contracts_of_system = guard
            .contracts
            .get(system_symbol)
            .cloned()
            .unwrap_or_default();

        Ok(contracts_of_system
            .values()
            .cloned()
            .max_by_key(|(_contract, created_at)| *created_at)
            .map(|(contract, _)| contract.clone()))
    }
}
