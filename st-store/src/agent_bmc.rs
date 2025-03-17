use crate::ctx::Ctx;
use crate::{db, DbMarketEntry, DbModelManager, DbShipEntry};
use anyhow::*;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use sqlx::types::Json;
use sqlx::{Pool, Postgres};
use st_domain::{Agent, MarketData, Ship, StStatusResponse};

pub struct AgentBmc;

impl AgentBmc {
    pub async fn get_initial_agent(_ctx: &Ctx, mm: &DbModelManager) -> Result<Agent> {
        let registration_response = db::load_registration(mm.pool()).await?;

        Ok(registration_response.unwrap().entry.data.agent.clone())
    }
}
