use crate::st_client::StClientTrait;
use anyhow::*;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use serde::Serialize;
use st_domain::budgeting::treasury_redesign::FinanceTicket;
use st_domain::{
    CreateChartBody, CreateSurveyResponse, ExtractResourcesResponse, FleetId, FlightMode, Fuel, JettisonCargoResponse, JumpGate, MarketData, MiningOpsConfig,
    Nav, NavAndFuelResponse, PurchaseShipResponse, PurchaseTradeGoodResponse, RawDeliveryRoute, RefuelShipResponse, SellTradeGoodResponse, Ship, ShipSymbol,
    ShipType, Shipyard, SiphonResourcesResponse, SiphoningOpsConfig, SupplyConstructionSiteResponse, Survey, TradeGoodSymbol, TransferCargoResponse,
    TravelAction, WaypointSymbol,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::{Deref, DerefMut, Not};
use std::sync::Arc;
use tracing::debug;

#[derive(Clone, Debug, Serialize)]
pub struct ShipOperations {
    pub ship: Ship,
    #[serde(skip_serializing)]
    pub client: Arc<dyn StClientTrait>,
    pub travel_action_queue: VecDeque<TravelAction>,
    pub current_navigation_destination: Option<WaypointSymbol>,
    pub explore_location_queue: VecDeque<WaypointSymbol>,
    pub permanent_observation_location: Option<WaypointSymbol>,
    pub maybe_next_observation_time: Option<DateTime<Utc>>,
    pub my_fleet: FleetId,
    pub maybe_mining_waypoint: Option<WaypointSymbol>,
    pub maybe_siphoning_waypoint: Option<WaypointSymbol>,
}

impl PartialEq for ShipOperations {
    fn eq(&self, other: &Self) -> bool {
        self.ship.eq(&other.ship)
    }
}

impl ShipOperations {
    pub(crate) fn to_debug_string(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub(crate) fn current_location(&self) -> WaypointSymbol {
        self.ship.nav.waypoint_symbol.clone()
    }

    pub(crate) fn current_travel_action(&self) -> Option<&TravelAction> {
        self.travel_action_queue.front()
    }

    pub fn last_travel_action(&self) -> Option<&TravelAction> {
        self.travel_action_queue.back()
    }

    pub(crate) fn set_nav(&mut self, new_nav: Nav) {
        self.nav = new_nav;
    }

    pub(crate) fn set_fuel(&mut self, new_fuel: Fuel) {
        self.fuel = new_fuel;
    }

    pub fn set_route(&mut self, new_route: Vec<TravelAction>) {
        self.travel_action_queue = VecDeque::from(new_route);
    }

    pub fn new(ship: Ship, client: Arc<dyn StClientTrait>, my_fleet: FleetId) -> Self {
        ShipOperations {
            ship,
            client,
            travel_action_queue: VecDeque::new(),
            current_navigation_destination: None,
            explore_location_queue: VecDeque::new(),
            permanent_observation_location: None,
            maybe_next_observation_time: None,
            my_fleet,
            maybe_mining_waypoint: None,
            maybe_siphoning_waypoint: None,
        }
    }

    pub fn pop_travel_action(&mut self) {
        let _ = self.travel_action_queue.pop_front();
    }

    pub fn set_destination(&mut self, destination: WaypointSymbol) {
        self.current_navigation_destination = Some(destination)
    }

    pub fn set_siphoning_waypoint(&mut self, siphoning_waypoint: WaypointSymbol) {
        self.maybe_siphoning_waypoint = Some(siphoning_waypoint);
    }

    pub fn set_mining_waypoint(&mut self, mining_waypoint: WaypointSymbol) {
        self.maybe_mining_waypoint = Some(mining_waypoint);
    }

    pub fn is_at_mining_waypoint(&self) -> bool {
        self.is_stationary()
            && self
                .get_mining_site()
                .map(|wps| wps == self.nav.waypoint_symbol)
                .unwrap_or(false)
    }

    pub fn get_mining_site(&self) -> Option<WaypointSymbol> {
        self.maybe_mining_waypoint.clone()
    }

    pub fn set_next_observation_time(&mut self, next_time: DateTime<Utc>) {
        self.maybe_next_observation_time = Some(next_time)
    }

    pub fn pop_explore_location_as_destination(&mut self) {
        self.current_navigation_destination = self.explore_location_queue.pop_front();
    }

    pub fn set_explore_locations(&mut self, waypoint_symbols: Vec<WaypointSymbol>) {
        let deque = VecDeque::from(waypoint_symbols);
        self.explore_location_queue = deque;
    }

    pub fn set_permanent_observation_location(&mut self, waypoint_symbol: WaypointSymbol) {
        self.permanent_observation_location = Some(waypoint_symbol);
    }

    pub async fn dock(&mut self) -> Result<Nav> {
        let response = self.client.dock_ship(self.ship.symbol.clone()).await?;
        Ok(response.data.nav)
    }

    pub async fn perform_survey(&mut self) -> Result<CreateSurveyResponse> {
        let response = self.client.survey(self.ship.symbol.clone()).await?;
        self.cooldown = response.data.cooldown.clone();
        Ok(response)
    }

    pub async fn siphon_resources(&mut self) -> Result<SiphonResourcesResponse> {
        let response = self
            .client
            .siphon_resources(self.ship.symbol.clone())
            .await?;

        self.cargo = response.data.cargo.clone();
        self.cooldown = response.data.cooldown.clone();

        Ok(response)
    }

    pub(crate) async fn jettison_everything_not_on_list(&mut self, allow_list: HashSet<TradeGoodSymbol>) -> Result<Vec<JettisonCargoResponse>> {
        let cargo_units_before = self.cargo.units;

        let items_to_jettison = self
            .cargo
            .inventory
            .iter()
            .filter_map(|inventory_entry| {
                allow_list
                    .contains(&inventory_entry.symbol)
                    .not()
                    .then_some(inventory_entry.clone())
            })
            .collect_vec();

        let mut responses = vec![];

        for item in items_to_jettison.iter() {
            let response = self.jettison_cargo(&item.symbol, item.units).await?;
            responses.push(response);
        }
        let cargo_units_after = self.cargo.units;

        if items_to_jettison.is_empty().not() {
            let jettison_log_str = items_to_jettison
                .iter()
                .map(|inv| format!("{}x {}", inv.units, inv.symbol))
                .join(", ");

            let allow_list_str = allow_list
                .iter()
                .map(|tg| tg.to_string())
                .sorted()
                .join(", ");

            debug!(
                message = "Jettisoned items currently not in demand",
                items = jettison_log_str,
                cargo_units_before,
                cargo_units_after,
                allow_list = allow_list_str
            );
        }

        Ok(responses)
    }

    pub async fn jettison_cargo(&mut self, trade_good_symbol: &TradeGoodSymbol, units: u32) -> Result<JettisonCargoResponse> {
        let response = self
            .client
            .jettison_cargo(self.ship.symbol.clone(), trade_good_symbol.clone(), units)
            .await?;

        self.cargo = response.data.cargo.clone();

        Ok(response)
    }

    pub(crate) async fn get_market(&self) -> Result<MarketData> {
        let response = self
            .client
            .get_marketplace(self.nav.waypoint_symbol.clone())
            .await?;
        Ok(response.data)
    }

    pub(crate) async fn transfer_cargo(&mut self, to_ship_id: ShipSymbol, trade_symbol: TradeGoodSymbol, units: u32) -> Result<TransferCargoResponse> {
        let response = self
            .client
            .transfer_cargo(self.symbol.clone(), to_ship_id, trade_symbol, units)
            .await?;

        self.cargo = response.data.cargo.clone();
        Ok(response)
    }

    pub(crate) async fn get_jump_gate(&self) -> Result<JumpGate> {
        let response = self
            .client
            .get_jump_gate(self.nav.waypoint_symbol.clone())
            .await?;
        Ok(response.data)
    }

    pub(crate) async fn get_shipyard(&self) -> Result<Shipyard> {
        let response = self
            .client
            .get_shipyard(self.nav.waypoint_symbol.clone())
            .await?;
        Ok(response.data)
    }

    pub(crate) async fn chart_waypoint(&self) -> Result<CreateChartBody> {
        let response = self.client.create_chart(self.symbol.clone()).await?;
        Ok(response.data)
    }

    pub(crate) async fn set_flight_mode(&self, mode: &FlightMode) -> Result<NavAndFuelResponse> {
        let response = self
            .client
            .set_flight_mode(self.ship.symbol.clone(), mode)
            .await?;
        Ok(response.data)
    }

    pub async fn orbit(&mut self) -> Result<Nav> {
        let response = self.client.orbit_ship(self.ship.symbol.clone()).await?;
        Ok(response.data.nav)
    }

    pub async fn navigate(&self, to: &WaypointSymbol) -> Result<NavAndFuelResponse> {
        let response = self.client.navigate(self.ship.symbol.clone(), to).await?;
        Ok(response.data)
    }

    pub(crate) async fn refuel(&mut self, from_cargo: bool) -> Result<RefuelShipResponse> {
        let amount = self.fuel.capacity - self.fuel.current;

        let response = self
            .client
            .refuel(self.ship.symbol.clone(), amount as u32, from_cargo)
            .await?;

        self.fuel = response.data.fuel.clone();

        Ok(response)
    }

    pub async fn sell_trade_good(&mut self, quantity: u32, trade_good: TradeGoodSymbol) -> Result<SellTradeGoodResponse> {
        let response = self
            .client
            .sell_trade_good(self.symbol.clone(), quantity, trade_good.clone())
            .await?;
        self.cargo = response.data.cargo.clone();

        Ok(response)
    }

    pub async fn purchase_trade_good(&mut self, quantity: u32, trade_good_symbol: TradeGoodSymbol) -> Result<PurchaseTradeGoodResponse> {
        let response = self
            .client
            .purchase_trade_good(self.symbol.clone(), quantity, trade_good_symbol)
            .await?;
        self.cargo = response.data.cargo.clone();

        Ok(response)
    }

    pub async fn supply_construction_site(
        &mut self,
        quantity: u32,
        trade_good: &TradeGoodSymbol,
        construction_site_waypoint_symbol: &WaypointSymbol,
    ) -> Result<SupplyConstructionSiteResponse> {
        let response = self
            .client
            .supply_construction_site(self.symbol.clone(), quantity, trade_good.clone(), construction_site_waypoint_symbol.clone())
            .await?;
        self.cargo = response.data.cargo.clone();

        Ok(response)
    }

    pub async fn extract_resources(&mut self, maybe_survey: Option<Survey>) -> Result<ExtractResourcesResponse> {
        let response = match maybe_survey {
            Some(survey) => {
                self.client
                    .extract_resources_with_survey(self.symbol.clone(), survey)
                    .await?
            }
            None => self.client.extract_resources(self.symbol.clone()).await?,
        };

        self.cargo = response.data.cargo.clone();

        Ok(response)
    }

    pub async fn purchase_ship(&self, ship_type: &ShipType, waypoint_symbol: &WaypointSymbol) -> Result<PurchaseShipResponse> {
        let response = self
            .client
            .purchase_ship(*ship_type, waypoint_symbol.clone())
            .await?;

        Ok(response)
    }

    // Other methods that require API access...

    pub fn get_ship(&self) -> &Ship {
        &self.ship
    }

    pub fn get_ship_mut(&mut self) -> &mut Ship {
        &mut self.ship
    }
}

impl Deref for ShipOperations {
    type Target = Ship;

    fn deref(&self) -> &Self::Target {
        &self.ship
    }
}

// If you need mutable access, you can also implement DerefMut
impl DerefMut for ShipOperations {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ship
    }
}
