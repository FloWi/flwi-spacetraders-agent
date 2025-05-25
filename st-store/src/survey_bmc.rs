use crate::ctx::Ctx;
use crate::{db, DbConstructionSiteEntry, DbModelManager};
use anyhow::*;
use async_trait::async_trait;
use chrono::Utc;
use itertools::Itertools;
use mockall::automock;
use sqlx::types::Json;
use st_domain::{Construction, Survey, SurveySignature, SystemSymbol, WaypointSymbol};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug)]
pub struct DbSurveyBmc {
    pub(crate) mm: DbModelManager,
}

#[automock]
#[async_trait]
pub trait SurveyBmcTrait: Send + Sync + Debug {
    async fn save_surveys(&self, ctx: &Ctx, surveys: Vec<Survey>) -> Result<()>;
}

#[async_trait]
impl SurveyBmcTrait for DbSurveyBmc {
    async fn save_surveys(&self, ctx: &Ctx, surveys: Vec<Survey>) -> Result<()> {
        db::upsert_surveys(self.mm.pool(), surveys, Utc::now()).await
    }
}

#[derive(Debug)]
pub struct InMemorySurveys {
    surveys: HashMap<WaypointSymbol, HashMap<SurveySignature, Survey>>,
}

impl Default for InMemorySurveys {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemorySurveys {
    pub fn new() -> Self {
        Self { surveys: Default::default() }
    }
}

#[derive(Debug)]
pub struct InMemorySurveyBmc {
    in_memory_surveys: Arc<RwLock<InMemorySurveys>>,
}

impl Default for InMemorySurveyBmc {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemorySurveyBmc {
    pub fn new() -> Self {
        Self {
            in_memory_surveys: Arc::new(RwLock::new(InMemorySurveys::new())),
        }
    }
}

#[async_trait]
impl SurveyBmcTrait for InMemorySurveyBmc {
    async fn save_surveys(&self, ctx: &Ctx, surveys: Vec<Survey>) -> Result<()> {
        let mut in_memory_surveys = self.in_memory_surveys.write().await;
        for survey in surveys {
            let mut surveys_at_wp = in_memory_surveys
                .surveys
                .entry(survey.waypoint_symbol.clone())
                .or_default();

            surveys_at_wp
                .entry(survey.signature.clone())
                .insert_entry(survey.clone());
        }

        Ok(())
    }
}
