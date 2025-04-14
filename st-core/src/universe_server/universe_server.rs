use crate::pagination::{PaginatedResponse, PaginationInput};
use crate::st_client::StClientTrait;
use crate::universe_server::universe_snapshot::load_universe;
use crate::{calculate_fuel_consumption, calculate_time};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::{TimeDelta, Utc};
use itertools::Itertools;
use st_domain::{
    Agent, AgentResponse, AgentSymbol, Construction, CreateChartResponse, Data, DockShipResponse, FlightMode, FuelConsumed, GetConstructionResponse,
    GetJumpGateResponse, GetMarketResponse, GetShipyardResponse, GetSupplyChainResponse, GetSystemResponse, LabelledCoordinate, ListAgentsResponse, MarketData,
    Meta, NavAndFuelResponse, NavRouteWaypoint, NavStatus, NavigateShipResponse, OrbitShipResponse, PurchaseShipResponse, PurchaseTradeGoodResponse,
    RefuelShipResponse, RegistrationRequest, RegistrationResponse, Route, SellTradeGoodResponse, SetFlightModeResponse, Ship, ShipSymbol, ShipType, Shipyard,
    StStatusResponse, SupplyConstructionSiteResponse, SystemSymbol, SystemsPageData, TradeGoodSymbol, Waypoint, WaypointSymbol,
};
use std::collections::HashMap;
use std::ops::Add;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug)]
pub struct InMemoryUniverse {
    pub(crate) systems: HashMap<SystemSymbol, SystemsPageData>,
    pub(crate) waypoints: HashMap<WaypointSymbol, Waypoint>,
    pub(crate) ships: HashMap<ShipSymbol, Ship>,
    pub(crate) marketplaces: HashMap<WaypointSymbol, MarketData>,
    pub(crate) shipyards: HashMap<WaypointSymbol, Shipyard>,
    pub(crate) construction_sites: HashMap<WaypointSymbol, Construction>,
    pub(crate) agent: Agent,
}

impl InMemoryUniverse {
    pub fn from_snapshot<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        load_universe(path)
    }

    pub fn load_from_file() -> Result<InMemoryUniverse> {
        let snapshot_path = "./universe_snapshot.json";

        // Try to load from snapshot, fall back to empty universe if file doesn't exist
        match InMemoryUniverse::from_snapshot(snapshot_path) {
            Ok(universe) => {
                println!("Loaded universe from snapshot");
                Ok(universe)
            }
            Err(e) => Err(anyhow!("Failed to load universe snapshot: {}", e)),
        }
    }
}

// Custom error type
#[derive(Debug, thiserror::Error)]
pub enum UniverseClientError {
    #[error("Resource not found: {0}")]
    NotFound(String),
    #[error("Bad request: {0}")]
    BadRequest(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Client implementation using InMemoryUniverse with interior mutability
#[derive(Debug)]
pub struct InMemoryUniverseClient {
    universe: Arc<RwLock<InMemoryUniverse>>,
}

impl InMemoryUniverseClient {
    /// Create a new InMemoryUniverseClient
    pub fn new(universe: InMemoryUniverse) -> Self {
        Self {
            universe: Arc::new(RwLock::new(universe)),
        }
    }

    /// Get a clone of the Arc for sharing
    pub fn clone_universe_handle(&self) -> Arc<RwLock<InMemoryUniverse>> {
        Arc::clone(&self.universe)
    }
}

#[async_trait]
impl StClientTrait for InMemoryUniverseClient {
    async fn register(&self, registration_request: RegistrationRequest) -> anyhow::Result<Data<RegistrationResponse>> {
        todo!()
    }

    async fn get_public_agent(&self, agent_symbol: &AgentSymbol) -> anyhow::Result<AgentResponse> {
        todo!()
    }

    async fn get_agent(&self) -> anyhow::Result<AgentResponse> {
        Ok(AgentResponse {
            data: self.universe.read().await.agent.clone(),
        })
    }

    async fn get_construction_site(&self, waypoint_symbol: &WaypointSymbol) -> anyhow::Result<GetConstructionResponse> {
        match self.universe.read().await.construction_sites.get(&waypoint_symbol) {
            None => {
                anyhow::bail!("Marketplace not found")
            }
            Some(cs) => Ok(GetConstructionResponse { data: cs.clone() }),
        }
    }

    async fn get_supply_chain(&self) -> anyhow::Result<GetSupplyChainResponse> {
        todo!()
    }

    async fn dock_ship(&self, ship_symbol: ShipSymbol) -> anyhow::Result<DockShipResponse> {
        todo!()
    }

    async fn set_flight_mode(&self, ship_symbol: ShipSymbol, mode: &FlightMode) -> anyhow::Result<SetFlightModeResponse> {
        todo!()
    }

    async fn navigate(&self, ship_symbol: ShipSymbol, to: &WaypointSymbol) -> anyhow::Result<NavigateShipResponse> {
        let read_universe = self.universe.read().await;
        let mut universe = self.universe.write().await;
        if let Some(ship) = universe.ships.get_mut(&ship_symbol) {
            let from_wp = read_universe.waypoints.get(&ship.nav.waypoint_symbol).ok_or(anyhow!("from_wp not found"))?;
            let to_wp = read_universe.waypoints.get(to).ok_or(anyhow!("to_wp not found"))?;
            let distance = from_wp.distance_to(to_wp);
            let fuel = calculate_fuel_consumption(&ship.nav.flight_mode, distance);
            let time = calculate_time(&ship.nav.flight_mode, distance, ship.engine.speed as u32);

            let maybe_cannot_fly_reason: Result<()> = match ship.nav.status {
                NavStatus::InTransit => {
                    if Utc::now() < ship.nav.route.arrival {
                        Err(anyhow!("Ship is still in transit"))
                    } else {
                        Ok(())
                    }
                }
                NavStatus::InOrbit => Ok(()),
                NavStatus::Docked => Err(anyhow!("Ship is docked")),
            }
            .or({
                if ship.fuel.current >= fuel as i32 {
                    Ok(())
                } else {
                    Err(anyhow!(
                        "Ship does not not have enough fuel. Required: {}, current: {}",
                        fuel,
                        ship.fuel.current
                    ))
                }
            });

            match maybe_cannot_fly_reason {
                Ok(_) => {
                    ship.nav.status = NavStatus::InTransit;
                    ship.fuel.consumed = FuelConsumed {
                        amount: fuel as i32,
                        timestamp: Utc::now(),
                    };
                    ship.fuel.current -= fuel as i32;
                    ship.nav.route = Route {
                        origin: NavRouteWaypoint {
                            symbol: from_wp.symbol.clone(),
                            waypoint_type: from_wp.r#type.clone(),
                            system_symbol: from_wp.system_symbol.clone(),
                            x: from_wp.x as i32,
                            y: from_wp.y as i32,
                        },
                        destination: NavRouteWaypoint {
                            symbol: to_wp.symbol.clone(),
                            waypoint_type: to_wp.r#type.clone(),
                            system_symbol: to_wp.system_symbol.clone(),
                            x: to_wp.x as i32,
                            y: to_wp.y as i32,
                        },
                        departure_time: Utc::now(),
                        arrival: Utc::now().add(TimeDelta::milliseconds(time as i64)),
                    };

                    Ok(NavigateShipResponse {
                        data: NavAndFuelResponse {
                            nav: ship.nav.clone(),
                            fuel: ship.fuel.clone(),
                        },
                    })
                }
                Err(err) => Err(err),
            }
        } else {
            anyhow::bail!("Ship not found")
        }
    }

    async fn refuel(&self, ship_symbol: ShipSymbol, amount: u32, from_cargo: bool) -> anyhow::Result<RefuelShipResponse> {
        todo!()
    }

    async fn sell_trade_good(&self, ship_symbol: ShipSymbol, units: u32, trade_good: TradeGoodSymbol) -> anyhow::Result<SellTradeGoodResponse> {
        todo!()
    }

    async fn purchase_trade_good(&self, ship_symbol: ShipSymbol, units: u32, trade_good: TradeGoodSymbol) -> anyhow::Result<PurchaseTradeGoodResponse> {
        todo!()
    }

    async fn supply_construction_site(
        &self,
        ship_symbol: ShipSymbol,
        units: u32,
        trade_good: TradeGoodSymbol,
        waypoint_symbol: WaypointSymbol,
    ) -> anyhow::Result<SupplyConstructionSiteResponse> {
        todo!()
    }

    async fn purchase_ship(&self, ship_type: ShipType, symbol: WaypointSymbol) -> anyhow::Result<PurchaseShipResponse> {
        todo!()
    }

    async fn orbit_ship(&self, ship_symbol: ShipSymbol) -> anyhow::Result<OrbitShipResponse> {
        todo!()
    }

    async fn list_ships(&self, pagination_input: PaginationInput) -> anyhow::Result<PaginatedResponse<Ship>> {
        let read_universe = self.universe.read().await;
        //let mut _universe = self.universe.write().await;

        let start_idx = pagination_input.limit * (pagination_input.page - 1);
        let num_skip = u32::try_from(start_idx as i32 - 1).unwrap_or(0);
        let all_ships = read_universe.ships.values().sorted_by_key(|s| s.symbol.0.clone()).skip(num_skip as usize).take(pagination_input.limit as usize);

        let resp = PaginatedResponse {
            data: all_ships.cloned().collect_vec(),
            meta: Meta {
                total: read_universe.ships.len() as u32,
                page: pagination_input.page,
                limit: pagination_input.limit,
            },
        };
        Ok(resp)
    }

    async fn get_ship(&self, ship_symbol: ShipSymbol) -> anyhow::Result<Data<Ship>> {
        todo!()
    }

    async fn list_waypoints_of_system_page(
        &self,
        system_symbol: &SystemSymbol,
        pagination_input: PaginationInput,
    ) -> anyhow::Result<PaginatedResponse<Waypoint>> {
        let guard = self.universe.read().await;
        //let mut _universe = self.universe.write().await;

        let start_idx = pagination_input.limit * (pagination_input.page - 1);
        let num_skip = u32::try_from(start_idx as i32 - 1).unwrap_or(0);

        let system_waypoints = guard.systems.get(system_symbol).map(|s| s.waypoints.clone()).unwrap_or_default();
        let waypoints =
            system_waypoints.into_iter().filter_map(|s_wp| guard.waypoints.get(&s_wp.symbol).cloned()).sorted_by_key(|wp| wp.symbol.clone()).collect_vec();

        let all_waypoints = waypoints.iter().skip(num_skip as usize).take(pagination_input.limit as usize);

        let resp = PaginatedResponse {
            data: all_waypoints.cloned().collect_vec(),
            meta: Meta {
                total: waypoints.len() as u32,
                page: pagination_input.page,
                limit: pagination_input.limit,
            },
        };
        Ok(resp)
    }

    async fn list_systems_page(&self, pagination_input: PaginationInput) -> anyhow::Result<PaginatedResponse<SystemsPageData>> {
        todo!()
    }

    async fn get_system(&self, system_symbol: &SystemSymbol) -> anyhow::Result<GetSystemResponse> {
        todo!()
    }

    async fn get_marketplace(&self, waypoint_symbol: WaypointSymbol) -> anyhow::Result<GetMarketResponse> {
        let guard = self.universe.read().await;

        match { guard.marketplaces.get(&waypoint_symbol) } {
            None => {
                anyhow::bail!("Marketplace not found")
            }
            Some(mp) => {
                let is_ship_present = guard.ships.iter().any(|(_, s)| s.nav.waypoint_symbol == waypoint_symbol);
                if is_ship_present {
                    Ok(GetMarketResponse { data: mp.clone() })
                } else {
                    let mut reduced_market_infos = mp.clone();
                    reduced_market_infos.transactions = None;
                    reduced_market_infos.trade_goods = None;

                    Ok(GetMarketResponse { data: reduced_market_infos })
                }
            }
        }
    }

    async fn get_jump_gate(&self, waypoint_symbol: WaypointSymbol) -> anyhow::Result<GetJumpGateResponse> {
        todo!()
    }

    async fn get_shipyard(&self, waypoint_symbol: WaypointSymbol) -> anyhow::Result<GetShipyardResponse> {
        match self.universe.read().await.shipyards.get(&waypoint_symbol) {
            None => {
                anyhow::bail!("Marketplace not found")
            }
            Some(sy) => Ok(GetShipyardResponse { data: sy.clone() }),
        }
    }

    async fn create_chart(&self, ship_symbol: ShipSymbol) -> anyhow::Result<CreateChartResponse> {
        todo!()
    }

    async fn list_agents_page(&self, pagination_input: PaginationInput) -> anyhow::Result<ListAgentsResponse> {
        todo!()
    }

    async fn get_status(&self) -> anyhow::Result<StStatusResponse> {
        todo!()
    }
}
