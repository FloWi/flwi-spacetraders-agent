use crate::ctx::Ctx;
use crate::{db, DbModelManager};
use anyhow::*;
use async_trait::async_trait;
use chrono::Utc;
use itertools::Itertools;
use mockall::automock;
use st_domain::{Survey, SurveySignature, WaypointSymbol};
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
    async fn get_all_valid_surveys_for_waypoint(&self, ctx: &Ctx, waypoint_symbol: &WaypointSymbol) -> Result<Vec<Survey>>;
    async fn mark_survey_as_exhausted(&self, ctx: &Ctx, waypoint_symbol: &WaypointSymbol, survey_signature: &SurveySignature) -> Result<()>;
}

#[async_trait]
impl SurveyBmcTrait for DbSurveyBmc {
    async fn save_surveys(&self, ctx: &Ctx, surveys: Vec<Survey>) -> Result<()> {
        db::upsert_surveys(self.mm.pool(), surveys, Utc::now()).await
    }

    async fn get_all_valid_surveys_for_waypoint(&self, ctx: &Ctx, waypoint_symbol: &WaypointSymbol) -> Result<Vec<Survey>> {
        db::get_valid_surveys_for_waypoint(self.mm.pool(), waypoint_symbol.clone(), Utc::now()).await
    }

    async fn mark_survey_as_exhausted(&self, ctx: &Ctx, _waypoint_symbol: &WaypointSymbol, survey_signature: &SurveySignature) -> Result<()> {
        db::mark_survey_as_exhausted(self.mm.pool(), survey_signature.clone()).await
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
        let now = Utc::now();

        for survey in surveys {
            let mut surveys_at_wp = in_memory_surveys
                .surveys
                .entry(survey.waypoint_symbol.clone())
                .or_default();

            surveys_at_wp.retain(|_signature, survey| survey.expiration > now);

            surveys_at_wp
                .entry(survey.signature.clone())
                .insert_entry(survey.clone());
        }

        Ok(())
    }

    async fn get_all_valid_surveys_for_waypoint(&self, _ctx: &Ctx, waypoint_symbol: &WaypointSymbol) -> Result<Vec<Survey>> {
        let mut in_memory_surveys = self.in_memory_surveys.write().await;

        let now = Utc::now();

        let result = if let Some(surveys_at_wp) = in_memory_surveys.surveys.get_mut(waypoint_symbol) {
            // clean up expired ones
            surveys_at_wp.retain(|_signature, survey| survey.expiration > now);
            Ok(surveys_at_wp.values().cloned().collect_vec())
        } else {
            Ok(Vec::new())
        };
        result
    }

    async fn mark_survey_as_exhausted(&self, ctx: &Ctx, waypoint_symbol: &WaypointSymbol, survey_signature: &SurveySignature) -> Result<()> {
        let mut in_memory_surveys = self.in_memory_surveys.write().await;

        let result = if let Some(surveys_at_wp) = in_memory_surveys.surveys.get_mut(waypoint_symbol) {
            surveys_at_wp.remove(survey_signature);
            Ok(())
        } else {
            Err(anyhow!("Survey not found"))
        };
        result
    }
}
