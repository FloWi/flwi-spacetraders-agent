use crate::ctx::Ctx;
use crate::{db, DbModelManager};
use anyhow::*;
use async_trait::async_trait;
use itertools::Itertools;
use mockall::automock;
use sqlx::types::Json;
use st_domain::{Agent, AgentResponse};
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct DbAgentEntry {
    entry: Json<Agent>,
}

#[derive(Debug)]
pub struct DbAgentBmc {
    pub(crate) mm: DbModelManager,
}

#[automock]
#[async_trait]
pub trait AgentBmcTrait: Send + Sync + Debug {
    async fn get_initial_agent(&self, ctx: &Ctx) -> Result<Agent>;
    async fn load_agent(&self, ctx: &Ctx) -> Result<Agent>;
    async fn store_agent(&self, ctx: &Ctx, agent: &Agent) -> Result<()>;
}

#[async_trait]

impl AgentBmcTrait for DbAgentBmc {
    async fn get_initial_agent(&self, _ctx: &Ctx) -> Result<Agent> {
        let registration_response = db::load_registration(self.mm.pool()).await?;

        Ok(registration_response.unwrap().entry.data.agent.clone())
    }

    async fn load_agent(&self, _ctx: &Ctx) -> Result<Agent> {
        let agent_entry: DbAgentEntry = sqlx::query_as!(
            DbAgentEntry,
            r#"
SELECT entry as "entry: Json<Agent>"
  from agent

        "#,
        )
        .fetch_one(self.mm.pool())
        .await?;

        Ok(agent_entry.entry.0)
    }

    async fn store_agent(&self, _ctx: &Ctx, agent: &Agent) -> Result<()> {
        sqlx::query!(
            r#"
insert into agent (agent_symbol, entry)
values ($1, $2)
on conflict (agent_symbol) do update set entry = excluded.entry
        "#,
            agent.symbol.0,
            Json(agent.clone()) as _
        )
        .execute(self.mm.pool())
        .await?;

        Ok(())
    }
}

#[derive(Debug)]
pub struct InMemoryAgentBmc {
    in_memory_agent: Arc<RwLock<Agent>>,
}

#[async_trait]
impl AgentBmcTrait for InMemoryAgentBmc {
    async fn get_initial_agent(&self, ctx: &Ctx) -> Result<Agent> {
        Ok(self.in_memory_agent.read().await.clone())
    }

    async fn load_agent(&self, ctx: &Ctx) -> Result<Agent> {
        Ok(self.in_memory_agent.read().await.clone())
    }

    async fn store_agent(&self, ctx: &Ctx, agent: &Agent) -> Result<()> {
        // println!("Storing agent");
        let mut a = self.in_memory_agent.write().await;
        *a = agent.clone();

        // println!("Stored agent");
        Ok(())
    }
}

impl InMemoryAgentBmc {
    pub fn new(agent: Agent) -> Self {
        Self {
            in_memory_agent: Arc::new(RwLock::new(agent)),
        }
    }
}
