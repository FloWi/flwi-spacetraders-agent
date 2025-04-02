use crate::ctx::Ctx;
use crate::{db, DbConstructionSiteEntry, DbMarketEntry, DbModelManager, DbShipEntry};
use anyhow::*;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use sqlx::types::Json;
use sqlx::{Pool, Postgres};
use st_domain::{Agent, MarketData, Ship, StStatusResponse};

pub struct AgentBmc;

struct DbAgentEntry {
    entry: Json<Agent>,
}

impl AgentBmc {
    pub async fn get_initial_agent(_ctx: &Ctx, mm: &DbModelManager) -> Result<Agent> {
        let registration_response = db::load_registration(mm.pool()).await?;

        Ok(registration_response.unwrap().entry.data.agent.clone())
    }

    pub async fn load_agent(_ctx: &Ctx, mm: &DbModelManager) -> Result<Agent> {
        let agent_entry: DbAgentEntry = sqlx::query_as!(
            DbAgentEntry,
            r#"
SELECT entry as "entry: Json<Agent>"
  from agent

        "#,
        )
        .fetch_one(mm.pool())
        .await?;

        Ok(agent_entry.entry.0)
    }

    pub async fn store_agent(_ctx: &Ctx, mm: &DbModelManager, agent: &Agent) -> Result<()> {
        sqlx::query!(
            r#"
insert into agent (agent_symbol, entry)
values ($1, $2)
on conflict (agent_symbol) do update set entry = excluded.entry
        "#,
            agent.symbol.0,
            Json(agent.clone()) as _
        )
        .execute(mm.pool())
        .await?;

        Ok(())
    }
}
