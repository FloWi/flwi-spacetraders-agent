use crate::ctx::Ctx;
use crate::DbModelManager;
use anyhow::*;
use async_trait::async_trait;
use st_domain::StStatusResponse;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct StatusBmc;

#[async_trait]
pub trait StatusBmcTrait: Send + Sync + Debug {
    async fn get_status(&self, ctx: &Ctx) -> Result<Option<StStatusResponse>>;
}

#[derive(Debug)]
pub struct DbStatusBmc {
    pub(crate) mm: DbModelManager,
}

#[async_trait]
impl StatusBmcTrait for DbStatusBmc {
    async fn get_status(&self, _ctx: &Ctx) -> Result<Option<StStatusResponse>> {
        Ok(crate::db::load_status(self.mm.pool())
            .await?
            .map(|db_status| db_status.entry.0))
    }
}

#[derive(Debug)]
pub struct InMemoryStatus {
    status_response: Option<StStatusResponse>,
}

#[derive(Debug)]
pub struct InMemoryStatusBmc {
    in_memory_status: Arc<RwLock<crate::InMemoryStatus>>,
}

impl Default for InMemoryStatusBmc {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryStatusBmc {
    pub fn new() -> Self {
        Self {
            in_memory_status: Arc::new(RwLock::new(InMemoryStatus { status_response: None })),
        }
    }
}

#[async_trait]
impl StatusBmcTrait for InMemoryStatusBmc {
    async fn get_status(&self, _ctx: &Ctx) -> Result<Option<StStatusResponse>> {
        Ok(self.in_memory_status.read().await.status_response.clone())
    }
}
