use crate::st_client::StClientTrait;
use anyhow::*;
use chrono::{DateTime, Utc};
use st_domain::budgeting::budgeting::TransactionTicket;
use st_domain::{
    CreateChartBody, DeliverConstructionMaterialTicketDetails, FlightMode, Fuel, JumpGate, MarketData, Nav, NavAndFuelResponse, PurchaseGoodTicketDetails,
    PurchaseShipResponse, PurchaseShipTicketDetails, PurchaseTradeGoodResponse, RefuelShipResponse, SellGoodTicketDetails, SellTradeGoodResponse, Ship,
    ShipType, Shipyard, SupplyConstructionSiteResponse, TradeGoodSymbol, TradeTicket, TransactionTicketId, TravelAction, WaypointSymbol,
};
use std::collections::VecDeque;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ShipOperations {
    pub ship: Ship,
    client: Arc<dyn StClientTrait>,
    pub travel_action_queue: VecDeque<TravelAction>,
    pub current_navigation_destination: Option<WaypointSymbol>,
    pub explore_location_queue: VecDeque<WaypointSymbol>,
    pub permanent_observation_location: Option<WaypointSymbol>,
    pub maybe_next_observation_time: Option<DateTime<Utc>>,
    pub maybe_trade: Option<TransactionTicket>,
}

impl PartialEq for ShipOperations {
    fn eq(&self, other: &Self) -> bool {
        self.ship.eq(&other.ship)
    }
}

impl ShipOperations {
    pub(crate) fn to_debug_string(&self) -> String {
        format!(
            r#"
ship: {:?}
travel_action_queue: {:?},
current_navigation_destination: {:?},
explore_location_queue: {:?},
permanent_observation_location: {:?},
maybe_next_observation_time: {:?},
maybe_trade: {:?},
        "#,
            &self.ship,
            &self.travel_action_queue,
            &self.current_navigation_destination,
            &self.explore_location_queue,
            &self.permanent_observation_location,
            &self.maybe_next_observation_time,
            &self.maybe_trade,
        )
        .trim()
        .to_string()
    }

    pub(crate) fn current_location(&self) -> WaypointSymbol {
        self.ship.nav.waypoint_symbol.clone()
    }

    pub(crate) fn remove_trade_ticket_if_complete(&mut self) {
        if let Some(trade) = self.maybe_trade.clone() {
            if trade.completed_at.is_some() {
                self.maybe_trade = None;
            }
        }
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

    pub fn new(ship: Ship, client: Arc<dyn StClientTrait>) -> Self {
        ShipOperations {
            ship,
            client,
            travel_action_queue: VecDeque::new(),
            current_navigation_destination: None,
            explore_location_queue: VecDeque::new(),
            permanent_observation_location: None,
            maybe_next_observation_time: None,
            maybe_trade: None,
        }
    }

    pub fn pop_travel_action(&mut self) {
        let _ = self.travel_action_queue.pop_front();
    }

    pub fn set_destination(&mut self, destination: WaypointSymbol) {
        self.current_navigation_destination = Some(destination)
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

    pub fn set_trade_ticket(&mut self, trade_ticket: TransactionTicket) {
        self.maybe_trade = Some(trade_ticket);
    }

    pub async fn dock(&mut self) -> Result<Nav> {
        let response = self.client.dock_ship(self.ship.symbol.clone()).await?;
        //println!("{:?}", response);
        Ok(response.data.nav)
    }

    pub(crate) async fn get_market(&self) -> Result<MarketData> {
        let response = self.client.get_marketplace(self.nav.waypoint_symbol.clone()).await?;
        //println!("{:?}", response);
        Ok(response.data)
    }

    pub(crate) async fn get_jump_gate(&self) -> Result<JumpGate> {
        let response = self.client.get_jump_gate(self.nav.waypoint_symbol.clone()).await?;
        //println!("{:?}", response);
        Ok(response.data)
    }

    pub(crate) async fn get_shipyard(&self) -> Result<Shipyard> {
        let response = self.client.get_shipyard(self.nav.waypoint_symbol.clone()).await?;
        //println!("{:?}", response);
        Ok(response.data)
    }

    pub(crate) async fn chart_waypoint(&self) -> Result<CreateChartBody> {
        let response = self.client.create_chart(self.symbol.clone()).await?;
        //println!("{:?}", response);
        Ok(response.data)
    }

    pub(crate) async fn set_flight_mode(&self, mode: &FlightMode) -> Result<NavAndFuelResponse> {
        let response = self.client.set_flight_mode(self.ship.symbol.clone(), mode).await?;
        //println!("{:?}", response);
        Ok(response.data)
    }

    pub async fn orbit(&mut self) -> Result<Nav> {
        let response = self.client.orbit_ship(self.ship.symbol.clone()).await?;
        //println!("{:?}", response);
        Ok(response.data.nav)
    }

    pub async fn navigate(&self, to: &WaypointSymbol) -> Result<NavAndFuelResponse> {
        let response = self.client.navigate(self.ship.symbol.clone(), to).await?;
        //println!("{:?}", response);
        Ok(response.data)
    }

    pub(crate) async fn refuel(&self, from_cargo: bool) -> Result<RefuelShipResponse> {
        let amount = self.fuel.capacity - self.fuel.current;

        let response = self.client.refuel(self.ship.symbol.clone(), amount as u32, from_cargo).await?;
        //println!("{:?}", response);
        Ok(response)
    }

    pub async fn sell_trade_good(&mut self, quantity: u32, trade_good: TradeGoodSymbol) -> Result<SellTradeGoodResponse> {
        let response = self.client.sell_trade_good(self.symbol.clone(), quantity, trade_good.clone()).await?;
        self.cargo = response.data.cargo.clone();

        //println!("{:?}", response);

        Ok(response)
    }

    pub async fn purchase_trade_good(&mut self, quantity: u32, trade_good_symbol: TradeGoodSymbol) -> Result<PurchaseTradeGoodResponse> {
        let response = self.client.purchase_trade_good(self.symbol.clone(), quantity, trade_good_symbol).await?;
        self.cargo = response.data.cargo.clone();
        //println!("{:?}", response);

        Ok(response)
    }

    pub async fn supply_construction_site(&mut self, ticket: &DeliverConstructionMaterialTicketDetails) -> Result<SupplyConstructionSiteResponse> {
        let response = self
            .client
            .supply_construction_site(
                self.symbol.clone(),
                ticket.quantity,
                ticket.trade_good.clone(),
                ticket.construction_site_waypoint_symbol.clone(),
            )
            .await?;
        self.cargo = response.data.cargo.clone();
        //println!("{:?}", response);

        Ok(response)
    }

    pub async fn purchase_ship(&self, ship_type: &ShipType, waypoint_symbol: &WaypointSymbol) -> Result<PurchaseShipResponse> {
        let response = self.client.purchase_ship(*ship_type, waypoint_symbol.clone()).await?;

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
