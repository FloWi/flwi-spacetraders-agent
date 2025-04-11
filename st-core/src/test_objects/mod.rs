use chrono::Local;
use itertools::Itertools;

use st_domain::{
    Agent, AgentSymbol, Cargo, Construction, ConstructionMaterial, Cooldown, Crew, Engine, FlightMode, Frame, Fuel, FuelConsumed, MarketData, Nav,
    NavOnlyResponse, NavRouteWaypoint, NavStatus, Reactor, RefuelShipResponseBody, Registration, Requirements, Route, Ship, ShipFrameSymbol, ShipPriceInfo,
    ShipRegistrationRole, ShipSymbol, SystemSymbol, TradeGoodSymbol, Transaction, TransactionType, Waypoint, WaypointSymbol, WaypointTrait,
    WaypointTraitSymbol, WaypointType,
};

use tracing::{subscriber::set_global_default, Level};
use tracing_subscriber::{
    fmt::Layer,
    fmt::{format::FmtSpan, time::Uptime},
    layer::SubscriberExt,
    prelude::*,
    EnvFilter, Registry,
};

pub struct TestObjects;

impl TestObjects {
    pub(crate) fn ship_prices(p0: &[Waypoint]) -> ShipPriceInfo {
        ShipPriceInfo { price_infos: vec![] }
    }

    pub(crate) fn latest_market_data(waypoints: &[Waypoint]) -> Vec<MarketData> {
        vec![]
    }

    pub(crate) fn startup_construction(waypoint_symbol: &WaypointSymbol) -> Construction {
        Construction {
            symbol: waypoint_symbol.0.to_string(),
            materials: vec![
                ConstructionMaterial {
                    trade_symbol: TradeGoodSymbol::ADVANCED_CIRCUITRY.to_string(),
                    required: 400,
                    fulfilled: 0,
                },
                ConstructionMaterial {
                    trade_symbol: TradeGoodSymbol::FAB_MATS.to_string(),
                    required: 1600,
                    fulfilled: 0,
                },
            ],
            is_complete: false,
        }
    }
}

impl TestObjects {
    pub(crate) fn agent() -> Agent {
        Agent {
            account_id: Some("account_id".to_string()),
            symbol: AgentSymbol("FLWI".to_string()),
            headquarters: Self::system_symbol().with_waypoint_suffix("H53"),
            credits: 175_000,
            starting_faction: "COSMIC".to_string(),
            ship_count: 2,
        }
    }

    pub fn waypoint_symbol() -> WaypointSymbol {
        WaypointSymbol("X1-FOO-BAR".to_string())
    }

    pub fn system_symbol() -> SystemSymbol {
        Self::waypoint_symbol().system_symbol()
    }

    pub fn create_waypoint(waypoint_symbol: &WaypointSymbol, x: i64, y: i64, waypoint_traits: Vec<WaypointTraitSymbol>) -> Waypoint {
        Waypoint {
            symbol: waypoint_symbol.clone(),
            r#type: WaypointType::PLANET,
            system_symbol: waypoint_symbol.system_symbol(),
            x,
            y,
            orbitals: vec![],
            orbits: None,
            faction: None,
            traits: waypoint_traits
                .into_iter()
                .map(|wts| WaypointTrait {
                    symbol: wts.clone(),
                    name: format!("name: {}", wts.to_string()),
                    description: format!("description: {}", wts.to_string()),
                })
                .collect_vec(),
            modifiers: vec![],
            chart: None,
            is_under_construction: false,
        }
    }

    pub fn create_market_data(waypoint_symbol: &WaypointSymbol) -> MarketData {
        MarketData {
            symbol: waypoint_symbol.clone(),
            exports: vec![],
            imports: vec![],
            exchange: vec![],
            transactions: None,
            trade_goods: None,
        }
    }

    pub fn create_fuel(starting_fuel: u32, consumed: u32) -> Fuel {
        Fuel {
            current: (starting_fuel - consumed) as i32,
            capacity: 600,
            consumed: FuelConsumed {
                amount: consumed as i32,
                timestamp: Local::now().to_utc(),
            },
        }
    }

    pub fn create_refuel_ship_response_body(amount: u32) -> RefuelShipResponseBody {
        RefuelShipResponseBody {
            agent: Agent {
                account_id: None,
                symbol: AgentSymbol("".to_string()),
                headquarters: Self::waypoint_symbol(),
                credits: 42,
                starting_faction: "".to_string(),
                ship_count: 2,
            },
            fuel: Self::create_fuel(600, 0),
            transaction: Transaction {
                waypoint_symbol: Self::waypoint_symbol(),
                ship_symbol: ShipSymbol("".to_string()),
                trade_symbol: TradeGoodSymbol::FUEL,
                transaction_type: TransactionType::Purchase,
                units: amount as i32,
                price_per_unit: 42,
                total_price: 0,
                timestamp: Default::default(),
            },
        }
    }

    pub fn create_nav(
        mode: FlightMode,
        nav_status: NavStatus,
        origin_waypoint_symbol: &WaypointSymbol,
        destination_waypoint_symbol: &WaypointSymbol,
    ) -> NavOnlyResponse {
        NavOnlyResponse {
            nav: Nav {
                system_symbol: destination_waypoint_symbol.system_symbol(),
                waypoint_symbol: destination_waypoint_symbol.clone(),
                route: Route {
                    destination: NavRouteWaypoint {
                        symbol: destination_waypoint_symbol.clone(),
                        waypoint_type: WaypointType::PLANET,
                        system_symbol: destination_waypoint_symbol.system_symbol(),
                        x: 0,
                        y: 0,
                    },
                    origin: NavRouteWaypoint {
                        symbol: origin_waypoint_symbol.clone(),
                        waypoint_type: WaypointType::PLANET,
                        system_symbol: origin_waypoint_symbol.system_symbol(),
                        x: 0,
                        y: 0,
                    },
                    departure_time: Default::default(),
                    arrival: Default::default(),
                },
                status: nav_status,
                flight_mode: mode,
            },
        }
    }

    pub fn test_ship(current_fuel: u32) -> Ship {
        Ship {
            symbol: ShipSymbol("FLWI-1".to_string()),
            registration: Registration {
                name: "FLWI".to_string(),
                faction_symbol: "GALACTIC".to_string(),
                role: ShipRegistrationRole::Command,
            },
            nav: Self::create_nav(FlightMode::Drift, NavStatus::InOrbit, &Self::waypoint_symbol(), &Self::waypoint_symbol()).nav,
            crew: Crew {
                current: 0,
                required: 0,
                capacity: 0,
                rotation: "".to_string(),
                morale: 0,
                wages: 0,
            },
            frame: Frame {
                symbol: ShipFrameSymbol::FRAME_DRONE,
                name: "".to_string(),
                description: "".to_string(),
                condition: 0.0.into(),
                integrity: 0.0.into(),
                module_slots: 0,
                mounting_points: 0,
                fuel_capacity: 0,
                requirements: Requirements {
                    power: None,
                    crew: None,
                    slots: None,
                },
            },
            reactor: Reactor {
                symbol: "".to_string(),
                name: "".to_string(),
                description: "".to_string(),
                condition: 0.0.into(),
                integrity: 0.0.into(),
                power_output: 0,
                requirements: Requirements {
                    power: None,
                    crew: None,
                    slots: None,
                },
            },
            engine: Engine {
                symbol: "".to_string(),
                name: "".to_string(),
                description: "".to_string(),
                condition: 0.0.into(),
                integrity: 0.0.into(),
                speed: 30,
                requirements: Requirements {
                    power: None,
                    crew: None,
                    slots: None,
                },
            },
            cooldown: Cooldown {
                ship_symbol: "".to_string(),
                total_seconds: 0,
                remaining_seconds: 0,
                expiration: None,
            },
            modules: vec![],
            mounts: vec![],
            cargo: Cargo {
                capacity: 0,
                units: 0,
                inventory: vec![],
            },
            fuel: Fuel {
                current: current_fuel as i32,
                capacity: 600,
                consumed: FuelConsumed {
                    amount: 0,
                    timestamp: Default::default(),
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::behavior_tree::behavior_args::BehaviorArgs;
    use crate::behavior_tree::behavior_tree::{ActionEvent, Behavior, Response};
    use crate::behavior_tree::ship_behaviors::ShipAction;
    use crate::ship::ShipOperations;

    use core::time::Duration;

    use crate::fleet::ship_runner::ship_behavior_runner;
    use tokio::sync::mpsc::{Receiver, Sender};

    use test_log::test;

    async fn test_run_ship_behavior(
        ship_ops: &mut ShipOperations,
        sleep_duration: Duration,
        args: BehaviorArgs,
        behavior: Behavior<ShipAction>,
    ) -> anyhow::Result<(Response, Vec<ShipOperations>, Vec<ActionEvent>)> {
        let (ship_updated_tx, mut ship_updated_rx): (Sender<ShipOperations>, Receiver<ShipOperations>) = tokio::sync::mpsc::channel(32);
        let (ship_action_completed_tx, mut ship_action_completed_rx): (Sender<ActionEvent>, Receiver<ActionEvent>) = tokio::sync::mpsc::channel(32);

        // Create channels for collectors to send results back
        let (state_result_tx, state_result_rx) = tokio::sync::oneshot::channel();
        let (action_result_tx, action_result_rx) = tokio::sync::oneshot::channel();

        tokio::spawn(async move {
            let mut received_messages = Vec::new();
            while let Some(msg) = ship_updated_rx.recv().await {
                received_messages.push(msg);
            }
            let _ = state_result_tx.send(received_messages);
        });

        tokio::spawn(async move {
            let mut received_messages = Vec::new();
            while let Some(msg) = ship_action_completed_rx.recv().await {
                received_messages.push(msg);
            }
            let _ = action_result_tx.send(received_messages);
        });

        // Run the behavior
        let result = ship_behavior_runner(ship_ops, sleep_duration, &args, behavior, &ship_updated_tx, &ship_action_completed_tx).await;

        // Close the channels to signal collection is complete
        drop(ship_updated_tx);
        drop(ship_action_completed_tx);

        // Wait for the collectors to finish and get their results
        let received_action_state_messages = state_result_rx.await.map_err(|_| anyhow::anyhow!("Failed to receive action state messages"))?;
        let received_action_completed_messages = action_result_rx.await.map_err(|_| anyhow::anyhow!("Failed to receive action completed messages"))?;

        Ok((result?, received_action_state_messages, received_action_completed_messages))
    }
}

pub fn setup_test_tracing() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("trace"));

    // Format with timestamps, all span events, and include parent spans
    let fmt_layer = Layer::new()
        .with_timer(Uptime::default())
        .with_span_events(FmtSpan::FULL) // This will show new, enter, exit, close events
        .with_thread_ids(true)
        .with_target(true);

    // Register the subscriber
    let subscriber = Registry::default().with(env_filter).with(fmt_layer);

    // Set as global default
    set_global_default(subscriber).expect("Failed to set tracing subscriber");
}
