use crate::{db, Ctx, DbModelManager};
use async_trait::async_trait;
use chrono::Utc;
use itertools::Itertools;
use mockall::automock;
use st_domain::budgeting::treasury_redesign::{LedgerArchiveTask, LedgerEntry};
use std::collections::VecDeque;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::Mutex;

#[automock]
#[async_trait]
pub trait LedgerBmcTrait: Send + Sync + Debug {
    async fn archive_ledger_entry(&self, _ctx: &Ctx, ledger_entry: &LedgerEntry) -> anyhow::Result<()>;
    async fn get_ledger_entries_in_order(&self, _ctx: &Ctx) -> anyhow::Result<Vec<LedgerEntry>>;
}

#[derive(Debug)]
pub struct DbLedgerBmc {
    pub mm: DbModelManager,
}

#[async_trait]
impl LedgerBmcTrait for DbLedgerBmc {
    async fn archive_ledger_entry(&self, _ctx: &Ctx, ledger_entry: &LedgerEntry) -> anyhow::Result<()> {
        db::archive_ledger_entry(self.mm.pool(), ledger_entry, Utc::now()).await?;

        Ok(())
    }

    async fn get_ledger_entries_in_order(&self, _ctx: &Ctx) -> anyhow::Result<Vec<LedgerEntry>> {
        let entries = db::get_ledger_entries_in_order(self.mm.pool(), Utc::now()).await?;

        Ok(entries)
    }
}

#[derive(Debug)]
pub struct InMemoryLedger {
    archived: VecDeque<LedgerEntry>,
}

impl InMemoryLedger {
    fn new() -> Self {
        Self { archived: Default::default() }
    }
}

#[derive(Debug)]
pub struct InMemoryLedgerBmc {
    in_memory_ledger: Arc<Mutex<InMemoryLedger>>,
}

impl InMemoryLedgerBmc {
    pub fn new() -> Self {
        Self {
            in_memory_ledger: Arc::new(Mutex::new(InMemoryLedger::new())),
        }
    }
}

impl Default for InMemoryLedgerBmc {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LedgerBmcTrait for InMemoryLedgerBmc {
    async fn archive_ledger_entry(&self, _ctx: &Ctx, ledger_entry: &LedgerEntry) -> anyhow::Result<()> {
        let mut guard = self.in_memory_ledger.lock().await;
        guard.archived.push_back(ledger_entry.clone());

        Ok(())
    }

    async fn get_ledger_entries_in_order(&self, _ctx: &Ctx) -> anyhow::Result<Vec<LedgerEntry>> {
        let guard = self.in_memory_ledger.lock().await;
        Ok(guard.archived.iter().cloned().collect_vec())
    }
}
