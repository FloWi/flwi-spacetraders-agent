use crate::pagination::{PaginatedResponse, PaginationInput};
use crate::st_client::StClientTrait;
use crate::universe_server::universe_server::RefuelTaskAnalysisError::{NotEnoughCredits, ShipNotFound, WaypointDoesntSellFuel};
use crate::universe_server::universe_snapshot::load_universe;
use crate::{calculate_fuel_consumption, calculate_time};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::{TimeDelta, Utc};
use itertools::Itertools;
use st_domain::{
    Agent, AgentResponse, AgentSymbol, Cargo, Construction, CreateChartResponse, Data, DockShipResponse, FlightMode, FuelConsumed, GetConstructionResponse,
    GetJumpGateResponse, GetMarketResponse, GetShipyardResponse, GetSupplyChainResponse, GetSystemResponse, JumpGate, LabelledCoordinate, ListAgentsResponse,
    MarketData, Meta, NavAndFuelResponse, NavOnlyResponse, NavRouteWaypoint, NavStatus, NavigateShipResponse, NotEnoughFuelInCargoError, OrbitShipResponse,
    PurchaseShipResponse, PurchaseTradeGoodResponse, RefuelShipResponse, RefuelShipResponseBody, RegistrationRequest, RegistrationResponse, Route,
    SellTradeGoodResponse, SetFlightModeResponse, Ship, ShipSymbol, ShipType, Shipyard, StStatusResponse, SupplyConstructionSiteResponse, SystemSymbol,
    SystemsPageData, TradeGoodSymbol, Transaction, TransactionType, Waypoint, WaypointSymbol,
};
use std::collections::HashMap;
use std::ops::Add;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use RefuelTaskAnalysisError::NotEnoughFuelInCargo;

#[derive(Debug)]
pub struct InMemoryUniverse {
    pub(crate) systems: HashMap<SystemSymbol, SystemsPageData>,
    pub(crate) waypoints: HashMap<WaypointSymbol, Waypoint>,
    pub(crate) ships: HashMap<ShipSymbol, Ship>,
    pub(crate) marketplaces: HashMap<WaypointSymbol, MarketData>,
    pub(crate) shipyards: HashMap<WaypointSymbol, Shipyard>,
    pub(crate) construction_sites: HashMap<WaypointSymbol, Construction>,
    pub(crate) agent: Agent,
    pub(crate) transactions: Vec<Transaction>,
    pub(crate) jump_gates: HashMap<WaypointSymbol, JumpGate>,
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

    pub fn check_refuel_facts(&self, ship_symbol: ShipSymbol, fuel_units: u32, from_cargo: bool) -> Result<RefuelTaskAnalysisSuccess, RefuelTaskAnalysisError> {
        if let Some(ship) = self.ships.get(&ship_symbol) {
            let number_fuel_barrels = (fuel_units as f64 / 100.0).ceil() as u32;

            if from_cargo {
                let maybe_inventory_entry = ship.cargo.inventory.iter().find(|inv| inv.symbol == TradeGoodSymbol::FUEL);

                match maybe_inventory_entry {
                    Some(inv) if inv.units >= number_fuel_barrels => {
                        let either_new_cargo = ship.cargo.with_units_removed(TradeGoodSymbol::FUEL, number_fuel_barrels);

                        match either_new_cargo {
                            Ok(new_cargo) => Ok(RefuelTaskAnalysisSuccess::CanRefuelFromCargo {
                                barrels: number_fuel_barrels,
                                fuel_units,
                                new_cargo,
                                empty_transaction: Transaction {
                                    waypoint_symbol: ship.nav.waypoint_symbol.clone(),
                                    ship_symbol,
                                    trade_symbol: TradeGoodSymbol::FUEL,
                                    transaction_type: TransactionType::Purchase,
                                    units: 0,
                                    price_per_unit: 0,
                                    total_price: 0,
                                    timestamp: Default::default(),
                                },
                            }),
                            Err(err) => Err(NotEnoughFuelInCargo { reason: err }),
                        }
                    }
                    _ => {
                        let inventory_fuel_barrels = maybe_inventory_entry.map(|inv| inv.units).unwrap_or_default();
                        Err(NotEnoughFuelInCargo {
                            reason: NotEnoughFuelInCargoError {
                                required: number_fuel_barrels,
                                current: inventory_fuel_barrels,
                            },
                        })
                    }
                }
            } else {
                let maybe_fuel_mtg = self
                    .marketplaces
                    .get(&ship.nav.waypoint_symbol)
                    .and_then(|mp| mp.trade_goods.clone().unwrap_or_default().iter().find(|mtg| mtg.symbol == TradeGoodSymbol::FUEL).cloned());
                match maybe_fuel_mtg {
                    None => Err(WaypointDoesntSellFuel {
                        waypoint_symbol: ship.nav.waypoint_symbol.clone(),
                    }),
                    Some(fuel_mtg) => {
                        let total_price = fuel_mtg.purchase_price as i64 * number_fuel_barrels as i64;
                        if total_price <= self.agent.credits {
                            Ok(RefuelTaskAnalysisSuccess::CanRefuelFromMarket {
                                barrels: number_fuel_barrels,
                                fuel_units,
                                transaction: Transaction {
                                    waypoint_symbol: ship.nav.waypoint_symbol.clone(),
                                    ship_symbol,
                                    trade_symbol: TradeGoodSymbol::FUEL,
                                    transaction_type: TransactionType::Purchase,
                                    units: number_fuel_barrels as i32,
                                    price_per_unit: fuel_mtg.purchase_price,
                                    total_price: total_price as i32,
                                    timestamp: Default::default(),
                                },
                            })
                        } else {
                            Err(NotEnoughCredits {
                                required: total_price,
                                current: self.agent.credits,
                            })
                        }
                    }
                }
            }
        } else {
            Err(ShipNotFound)
        }
    }

    pub fn book_transaction_and_adjust_agent_credits(&mut self, transaction: &Transaction) {
        let cash_amount = match transaction.transaction_type {
            TransactionType::Purchase => transaction.total_price * -1,
            TransactionType::Sell => transaction.total_price,
        };

        self.agent.credits += cash_amount as i64;
        self.transactions.push(transaction.clone())
    }

    pub fn adjust_ship_fuel(&mut self, ship_symbol: &ShipSymbol, fuel_units: u32) {
        if let Some(ship) = self.ships.get_mut(ship_symbol) {
            ship.fuel.current = (ship.fuel.current + fuel_units as i32).min(ship.fuel.capacity);
        }
    }
    pub fn set_ship_cargo(&mut self, ship_symbol: &ShipSymbol, new_cargo: Cargo) {
        if let Some(ship) = self.ships.get_mut(ship_symbol) {
            ship.cargo = new_cargo;
        }
    }
}

enum RefuelTaskAnalysisSuccess {
    CanRefuelFromMarket {
        barrels: u32,
        fuel_units: u32,
        transaction: Transaction,
    },
    CanRefuelFromCargo {
        barrels: u32,
        fuel_units: u32,
        new_cargo: Cargo,
        empty_transaction: Transaction,
    },
}

enum RefuelTaskAnalysisError {
    NotEnoughCredits { required: i64, current: i64 },
    WaypointDoesntSellFuel { waypoint_symbol: WaypointSymbol },
    NotEnoughFuelInCargo { reason: NotEnoughFuelInCargoError },
    ShipNotFound,
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
        let mut universe = self.universe.write().await;
        if let Some(ship) = universe.ships.get_mut(&ship_symbol) {
            let maybe_cannot_dock_reason = match ship.nav.status {
                NavStatus::InTransit => {
                    if Utc::now() < ship.nav.route.arrival {
                        Err(anyhow!("Ship is still in transit"))
                    } else {
                        Ok(())
                    }
                }
                NavStatus::InOrbit => Ok(()),
                NavStatus::Docked => Err(anyhow!("Ship is docked")),
            };

            match maybe_cannot_dock_reason {
                Ok(_) => {
                    ship.nav.status = NavStatus::Docked;
                    Ok(DockShipResponse {
                        data: NavOnlyResponse { nav: ship.nav.clone() },
                    })
                }
                Err(err) => Err(err),
            }
        } else {
            Err(anyhow!("Ship not found"))
        }
    }

    async fn set_flight_mode(&self, ship_symbol: ShipSymbol, mode: &FlightMode) -> anyhow::Result<SetFlightModeResponse> {
        let mut universe = self.universe.write().await;
        if let Some(ship) = universe.ships.get_mut(&ship_symbol) {
            let maybe_cant_set_flight_mode_reason = match ship.nav.status {
                NavStatus::InTransit => {
                    if Utc::now() < ship.nav.route.arrival {
                        Err(anyhow!("Ship is still in transit. This is possible now, but not implemented yet."))
                    } else {
                        Ok(())
                    }
                }
                NavStatus::InOrbit => Ok(()),
                NavStatus::Docked => Err(anyhow!("Ship is docked")),
            };
            match maybe_cant_set_flight_mode_reason {
                Ok(_) => {
                    ship.nav.flight_mode = mode.clone();
                    ship.nav.status = NavStatus::InOrbit;
                    Ok(SetFlightModeResponse {
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

    async fn navigate(&self, ship_symbol: ShipSymbol, to: &WaypointSymbol) -> anyhow::Result<NavigateShipResponse> {
        let (from_wp, to_wp) = {
            let read_universe = self.universe.read().await;
            let ship_location = read_universe.ships.get(&ship_symbol).ok_or(anyhow!("ship not found not found"))?.nav.waypoint_symbol.clone();
            let from_wp = read_universe.waypoints.get(&ship_location).ok_or(anyhow!("from_wp not found"))?;
            let to_wp = read_universe.waypoints.get(to).ok_or(anyhow!("to_wp not found"))?;
            (from_wp.clone(), to_wp.clone())
        };

        let mut universe = self.universe.write().await;
        if let Some(ship) = universe.ships.get_mut(&ship_symbol) {
            let distance = from_wp.distance_to(&to_wp);
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
                    ship.nav.system_symbol = to_wp.symbol.system_symbol();
                    ship.nav.waypoint_symbol = to_wp.symbol.clone();
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
        let refuel_task_result = {
            let guard = self.universe.read().await;
            let facts = guard.check_refuel_facts(ship_symbol.clone(), amount, from_cargo);
            facts
        };

        let mut universe = self.universe.write().await;

        match refuel_task_result {
            Err(err) => match err {
                NotEnoughCredits { required, current } => Err(anyhow!("Not enough credits to refuel. required: {required}; current: {current} ")),
                NotEnoughFuelInCargo {
                    reason: NotEnoughFuelInCargoError { required, current },
                } => Err(anyhow!("Not enough cargo units to refuel. required: {required}; current: {current} ")),
                WaypointDoesntSellFuel { waypoint_symbol } => Err(anyhow!("Waypoint: {} doesn't sell fuel", waypoint_symbol.0.clone())),
                ShipNotFound => Err(anyhow!("Ship not found")),
            },
            Ok(res) => {
                let transaction = match res {
                    RefuelTaskAnalysisSuccess::CanRefuelFromMarket {
                        barrels,
                        fuel_units,
                        transaction,
                    } => {
                        universe.book_transaction_and_adjust_agent_credits(&transaction);
                        universe.adjust_ship_fuel(&ship_symbol, fuel_units);
                        transaction
                    }
                    RefuelTaskAnalysisSuccess::CanRefuelFromCargo {
                        barrels,
                        fuel_units,
                        new_cargo,
                        empty_transaction,
                    } => {
                        universe.adjust_ship_fuel(&ship_symbol, fuel_units);
                        universe.set_ship_cargo(&ship_symbol, new_cargo);
                        empty_transaction
                    }
                };
                Ok(RefuelShipResponse {
                    data: RefuelShipResponseBody {
                        agent: universe.agent.clone(),
                        fuel: universe.ships.get(&ship_symbol).expect("Ship").fuel.clone(),
                        transaction,
                    },
                })
            }
        }
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
        let mut universe = self.universe.write().await;
        if let Some(ship) = universe.ships.get_mut(&ship_symbol) {
            match ship.nav.status {
                NavStatus::InTransit => {
                    if Utc::now() < ship.nav.route.arrival {
                        Err(anyhow!("Ship is still in transit"))
                    } else {
                        Err(anyhow!("Ship is already in orbit"))
                    }
                }
                NavStatus::InOrbit => Err(anyhow!("Ship is already in orbit")),
                NavStatus::Docked => {
                    ship.nav.status = NavStatus::InOrbit;
                    Ok(OrbitShipResponse {
                        data: NavOnlyResponse { nav: ship.nav.clone() },
                    })
                }
            }
        } else {
            Err(anyhow!("Ship not found"))
        }
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
        let guard = self.universe.read().await;
        match guard.jump_gates.get(&waypoint_symbol) {
            None => {
                anyhow::bail!("Marketplace not found")
            }
            Some(jg) => Ok(GetJumpGateResponse { data: jg.clone() }),
        }
    }

    async fn get_shipyard(&self, waypoint_symbol: WaypointSymbol) -> anyhow::Result<GetShipyardResponse> {
        let guard = self.universe.read().await;
        match guard.shipyards.get(&waypoint_symbol) {
            None => {
                anyhow::bail!("Marketplace not found")
            }
            Some(sy) => {
                let is_ship_present = guard.ships.iter().any(|(_, s)| s.nav.waypoint_symbol == waypoint_symbol);
                if is_ship_present {
                    Ok(GetShipyardResponse { data: sy.clone() })
                } else {
                    let mut reduced_shipyard_infos = sy.clone();
                    reduced_shipyard_infos.transactions = None;
                    reduced_shipyard_infos.ships = None;

                    Ok(GetShipyardResponse { data: reduced_shipyard_infos })
                }
            }
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
