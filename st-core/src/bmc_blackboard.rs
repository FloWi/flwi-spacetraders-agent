use crate::pathfinder::pathfinder;
use crate::survey_manager;
use anyhow::anyhow;
use async_trait::async_trait;
use chrono::Utc;
use itertools::Itertools;
use st_domain::blackboard_ops::BlackboardOps;
use st_domain::{
    Construction, Contract, CreateSurveyResponse, Extraction, JumpGate, LabelledCoordinate, MarketData, MarketEntry, MaterializedSupplyChain,
    MiningOpsConfig, Ship, Shipyard, Survey, SystemSymbol, TravelAction, Waypoint, WaypointModifier, WaypointSymbol,
};
use st_store::bmc::Bmc;
use st_store::Ctx;
use std::sync::Arc;

pub struct BmcBlackboard {
    bmc: Arc<dyn Bmc>,
}

impl BmcBlackboard {
    pub(crate) fn new(bmc: Arc<dyn Bmc>) -> Self {
        Self { bmc }
    }
}

#[async_trait]
impl BlackboardOps for BmcBlackboard {
    async fn compute_path(
        &self,
        from: WaypointSymbol,
        to: WaypointSymbol,
        engine_speed: u32,
        current_fuel: u32,
        fuel_capacity: u32,
    ) -> anyhow::Result<Vec<TravelAction>> {
        assert_eq!(from.system_symbol(), to.system_symbol(), "Pathfinder currently only works in same system");

        let waypoints_of_system: Vec<Waypoint> = self
            .bmc
            .system_bmc()
            .get_waypoints_of_system(&Ctx::Anonymous, &from.system_symbol())
            .await?;

        let market_entries_of_system = self
            .bmc
            .market_bmc()
            .get_latest_market_data_for_system(&Ctx::Anonymous, &from.system_symbol())
            .await?;
        let market_data = market_entries_of_system
            .iter()
            .map(|me| me.market_data.clone())
            .collect_vec();

        match pathfinder::compute_path(
            from.clone(),
            to.clone(),
            waypoints_of_system,
            market_data,
            engine_speed,
            current_fuel,
            fuel_capacity,
        ) {
            Some(path) => Ok(path),
            None => Err(anyhow!("No path found from {:?} to {:?}", from, to)),
        }
    }

    async fn upsert_ship(&self, ship: &Ship) -> anyhow::Result<()> {
        self.bmc
            .ship_bmc()
            .upsert_ships(&Ctx::Anonymous, &vec![ship.clone()], Utc::now())
            .await
    }

    async fn insert_waypoint(&self, waypoint: &Waypoint) -> anyhow::Result<()> {
        self.bmc
            .system_bmc()
            .upsert_waypoint(&Ctx::Anonymous, waypoint.clone())
            .await
    }

    async fn insert_market(&self, market_data: MarketData) -> anyhow::Result<()> {
        self.bmc
            .market_bmc()
            .save_market_data(&Ctx::Anonymous, vec![market_data], Utc::now())
            .await
    }

    async fn insert_jump_gate(&self, jump_gate: JumpGate) -> anyhow::Result<()> {
        self.bmc
            .jump_gate_bmc()
            .save_jump_gate_data(&Ctx::Anonymous, jump_gate, Utc::now())
            .await
    }

    async fn insert_shipyard(&self, shipyard: Shipyard) -> anyhow::Result<()> {
        self.bmc
            .shipyard_bmc()
            .save_shipyard_data(&Ctx::Anonymous, shipyard, Utc::now())
            .await
    }

    async fn get_closest_waypoint(&self, current_waypoint: &WaypointSymbol, candidates: &[WaypointSymbol]) -> anyhow::Result<Option<WaypointSymbol>> {
        let waypoints: Vec<Waypoint> = self
            .bmc
            .system_bmc()
            .get_waypoints_of_system(&Ctx::Anonymous, &current_waypoint.system_symbol())
            .await?;
        let current_waypoint = waypoints
            .iter()
            .find(|wp| wp.symbol == *current_waypoint)
            .expect("Current location waypoint");

        Ok(candidates
            .iter()
            .map(|wps| {
                let wp = waypoints
                    .iter()
                    .find(|wp| wp.symbol == *wps)
                    .expect("candidate waypoint");
                (wps.clone(), current_waypoint.distance_to(wp))
            })
            .sorted_by_key(|(_, distance)| *distance)
            .take(1)
            .next()
            .map(|(best, _)| best))
    }

    async fn get_waypoint(&self, waypoint_symbol: &WaypointSymbol) -> anyhow::Result<Waypoint> {
        let waypoints: Vec<Waypoint> = self
            .bmc
            .system_bmc()
            .get_waypoints_of_system(&Ctx::Anonymous, &waypoint_symbol.system_symbol())
            .await?;
        waypoints
            .into_iter()
            .find(|wp| wp.symbol == *waypoint_symbol)
            .ok_or(anyhow!("Waypoint not found"))
    }

    async fn get_waypoints_of_system(&self, system_symbol: &SystemSymbol) -> anyhow::Result<Vec<Waypoint>> {
        let waypoints: Vec<Waypoint> = self
            .bmc
            .system_bmc()
            .get_waypoints_of_system(&Ctx::Anonymous, system_symbol)
            .await?;

        Ok(waypoints)
    }

    async fn get_available_agent_credits(&self) -> anyhow::Result<i64> {
        Ok(self
            .bmc
            .agent_bmc()
            .load_agent(&Ctx::Anonymous)
            .await?
            .credits)
    }

    async fn update_construction_site(&self, construction: &Construction) -> anyhow::Result<()> {
        Ok(self
            .bmc
            .construction_bmc()
            .save_construction_site(&Ctx::Anonymous, construction.clone())
            .await?)
    }

    async fn get_best_survey_for_current_demand(
        &self,
        mining_config: &MiningOpsConfig,
        materialized_supply_chain: &MaterializedSupplyChain,
    ) -> anyhow::Result<Option<Survey>> {
        let available_surveys = self
            .bmc
            .survey_bmc()
            .get_all_valid_surveys_for_waypoint(&Ctx::Anonymous, &mining_config.mining_waypoint)
            .await?;

        let maybe_best_survey_with_score = survey_manager::pick_best_survey(available_surveys, materialized_supply_chain);

        Ok(maybe_best_survey_with_score.map(|(survey, _score)| survey.clone()))
    }

    async fn mark_survey_as_exhausted(&self, survey: &Survey) -> anyhow::Result<()> {
        self.bmc
            .survey_bmc()
            .mark_survey_as_exhausted(&Ctx::Anonymous, &survey.waypoint_symbol, &survey.signature)
            .await?;

        Ok(())
    }

    async fn save_survey_response(&self, create_survey_response: CreateSurveyResponse) -> anyhow::Result<()> {
        let surveys = create_survey_response.data.surveys;
        Ok(self
            .bmc
            .survey_bmc()
            .save_surveys(&Ctx::Anonymous, surveys)
            .await?)
    }

    async fn is_survey_necessary(&self, maybe_mining_waypoint: Option<WaypointSymbol>) -> anyhow::Result<bool> {
        // FIXME: remove duplicated code
        if let Some(mining_waypoint) = maybe_mining_waypoint {
            let available_surveys = self
                .bmc
                .survey_bmc()
                .get_all_valid_surveys_for_waypoint(&Ctx::Anonymous, &mining_waypoint)
                .await?;
            Ok(available_surveys.len() < 4)
        } else {
            Ok(false)
        }
    }

    async fn log_survey_usage(&self, survey: Survey, extraction: Extraction) -> anyhow::Result<()> {
        self.bmc
            .survey_bmc()
            .log_survey_usage(&Ctx::Anonymous, &survey.signature, &extraction)
            .await?;

        Ok(())
    }

    async fn mark_asteroid_has_reached_critical_limit(&self, mining_waypoint: &WaypointSymbol, waypoint_modifier: &WaypointModifier) -> anyhow::Result<()> {
        if let Ok(waypoint) = self
            .bmc
            .system_bmc()
            .get_waypoint(&Ctx::Anonymous, mining_waypoint)
            .await
        {
            if waypoint.has_reached_critical_limit() {
                Ok(())
            } else {
                let mut updated = waypoint.clone();
                updated.modifiers.push(waypoint_modifier.clone());

                self.bmc
                    .system_bmc()
                    .upsert_waypoint(&Ctx::Anonymous, updated)
                    .await?;

                Ok(())
            }
        } else {
            anyhow::bail!("Waypoint {} not found in system_bmc", mining_waypoint);
        }
    }

    async fn upsert_contract(&self, system_symbol: &SystemSymbol, contract: &Contract) -> anyhow::Result<()> {
        self.bmc
            .contract_bmc()
            .upsert_contract(&Ctx::Anonymous, system_symbol, contract.clone(), Utc::now())
            .await
    }

    async fn get_latest_market_entries(&self, system_symbol: &SystemSymbol) -> anyhow::Result<Vec<MarketEntry>> {
        self.bmc
            .market_bmc()
            .get_latest_market_data_for_system(&Ctx::Anonymous, system_symbol)
            .await
    }

    async fn get_youngest_contract(&self, system_symbol: &SystemSymbol) -> anyhow::Result<Option<Contract>> {
        self.bmc
            .contract_bmc()
            .get_youngest_contract(&Ctx::Anonymous, system_symbol)
            .await
    }
}
