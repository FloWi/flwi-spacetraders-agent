use crate::behavior_tree::behavior_args::{BehaviorArgs, ExplorationTask};
use crate::behavior_tree::behavior_tree::Response::Success;
use crate::behavior_tree::behavior_tree::{Actionable, Response};
use crate::behavior_tree::ship_behaviors::ShipAction;
use crate::pathfinder::pathfinder::TravelAction;
use crate::ship::ShipOperations;
use crate::st_model::{
    NavRouteWaypoint, NavStatus, RefuelShipResponse, RefuelShipResponseBody, ShipSymbol,
    WaypointType,
};
use anyhow::anyhow;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[async_trait]
impl Actionable for ShipAction {
    type ActionError = anyhow::Error;
    type ActionArgs = BehaviorArgs;
    type ActionState = ShipOperations;

    async fn run(
        &self,
        args: &Self::ActionArgs,
        state: &mut Self::ActionState,
    ) -> Result<Response, Self::ActionError> {
        match self {
            ShipAction::HasActiveTravelAction => {
                if state.current_travel_action.is_some() {
                    Ok(Success)
                } else {
                    Err(anyhow!("No active travel_action"))
                }
            }

            ShipAction::HasTravelActionEntry => {
                let no_action_left =
                    state.travel_action_queue.is_empty() && state.current_travel_action.is_none();
                if no_action_left {
                    Ok(Success)
                } else {
                    Ok(Response::Running)
                }
            }
            ShipAction::PopTravelAction => {
                state.pop_travel_action();
                Ok(Success)
            }

            ShipAction::IsNavigationAction => match state.current_travel_action {
                Some(TravelAction::Navigate { .. }) => Ok(Success),
                _ => Err(anyhow!("Failed")),
            },

            ShipAction::IsRefuelAction => match state.current_travel_action {
                Some(TravelAction::Refuel { .. }) => Ok(Success),
                _ => Err(anyhow!("Failed")),
            },

            ShipAction::WaitForArrival => match state.nav.status {
                NavStatus::Docked => Ok(Success),
                NavStatus::InTransit => Ok(Response::Running),
                NavStatus::InOrbit => {
                    let now: DateTime<Utc> = Utc::now();
                    let arrival_time: DateTime<Utc> = state.nav.route.arrival;
                    let is_still_travelling: bool = now < arrival_time;

                    if is_still_travelling {
                        Ok(Response::Running)
                    } else {
                        Ok(Success)
                    }
                }
            },

            ShipAction::FixNavStatusIfNecessary => match state.nav.status {
                NavStatus::InTransit | NavStatus::Docked => Ok(Success),
                NavStatus::InOrbit => {
                    let now: DateTime<Utc> = Utc::now();
                    let arrival_time: DateTime<Utc> = state.nav.route.arrival;
                    let is_still_travelling: bool = now < arrival_time;

                    if !is_still_travelling {
                        state.nav.status = NavStatus::InOrbit;
                    }
                    Ok(Success)
                }
            },

            ShipAction::IsDocked => match state.nav.status {
                NavStatus::Docked => Ok(Success),
                NavStatus::InTransit => Ok(Response::Running),
                NavStatus::InOrbit => Err(anyhow!("Failed")),
            },

            ShipAction::IsInOrbit => match state.nav.status {
                NavStatus::InOrbit => Ok(Success),
                NavStatus::InTransit => Ok(Response::Running),
                NavStatus::Docked => Err(anyhow!("Failed")),
            },

            ShipAction::IsCorrectFlightMode => {
                if let Some(action) = &state.current_travel_action {
                    match action {
                        TravelAction::Navigate { mode, .. } => {
                            let current_mode = &state.get_ship().nav.flight_mode;
                            if current_mode == mode {
                                Ok(Success)
                            } else {
                                Err(anyhow!(
                                    "Failed - current mode {} != wanted mode {}",
                                    current_mode,
                                    mode
                                ))
                            }
                        }
                        TravelAction::Refuel { .. } => {
                            Err(anyhow!("Failed - no travel mode on refuel action"))
                        }
                    }
                } else {
                    Err(anyhow!("Failed - no current action"))
                }
            }
            ShipAction::MarkTravelActionAsCompleteIfPossible => {
                match &state.current_travel_action {
                    None => Ok(Success),
                    Some(action) => {
                        let is_done = match action {
                            TravelAction::Navigate { to, .. } => {
                                state.nav.waypoint_symbol == *to
                                    && state.nav.status != NavStatus::InTransit
                            }
                            TravelAction::Refuel { at, .. } => {
                                state.nav.waypoint_symbol == *at
                                    && state.nav.status != NavStatus::InTransit
                                    && state.fuel.current == state.fuel.capacity
                            }
                        };

                        if is_done {
                            state.current_travel_action = None;
                        }
                        Ok(Success)
                    }
                }
            }
            ShipAction::CanSkipRefueling => match &state.current_travel_action {
                None => Err(anyhow!(
                    "Called CanSkipRefueling, but current action is None",
                )),
                Some(TravelAction::Navigate { .. }) => Err(anyhow!(
                    "Called CanSkipRefueling, but current action is Navigate",
                )),
                Some(TravelAction::Refuel { at, .. }) => {
                    // we can skip refueling, if
                    // - queued_action #1 is: go_to_waypoint X
                    // - queued_action #2 is: refuel_at_waypoint X
                    // - we have enough fuel to reach X in desired flight mode without refueling
                    let maybe_navigate_action: Option<&TravelAction> =
                        state.travel_action_queue.get(0);
                    let maybe_refuel_action: Option<&TravelAction> =
                        state.travel_action_queue.get(1);

                    if let Some((
                        TravelAction::Navigate {
                            fuel_consumption, ..
                        },
                        TravelAction::Refuel { .. },
                    )) = maybe_navigate_action.zip(maybe_refuel_action)
                    {
                        let has_enough_fuel = state.fuel.current >= (*fuel_consumption as i32);
                        if has_enough_fuel {
                            Ok(Success)
                        } else {
                            Err(anyhow!(
                                "Called CanSkipRefueling, but not enough fuel to reach destination",
                            ))
                        }
                    } else {
                        Err(anyhow!(
                            "Called CanSkipRefueling, but can't refuel at next station. maybe_navigate_action: {:?}; maybe_refuel_action: {:?}",
                            maybe_navigate_action,
                            maybe_refuel_action
                        ))
                    }
                }
            },

            ShipAction::Refuel => {
                let RefuelShipResponse {
                    data: RefuelShipResponseBody { fuel: new_fuel, .. },
                } = state.refuel(false).await?;
                state.set_fuel(new_fuel);
                Ok(Success)
            }

            ShipAction::Dock => {
                let new_nav = state.dock().await?;
                state.set_nav(new_nav);
                Ok(Success)
            }

            ShipAction::Orbit => {
                let new_nav = state.orbit().await?;
                state.set_nav(new_nav);
                Ok(Success)
            }

            ShipAction::SetFlightMode => {
                if let Some(action) = &state.current_travel_action {
                    match action {
                        TravelAction::Navigate { mode, .. } => {
                            let new_nav = state.set_flight_mode(mode).await?;
                            state.set_nav(new_nav);
                            Ok(Success)
                        }
                        TravelAction::Refuel { .. } => {
                            Err(anyhow!("Failed - no travel mode on refuel action"))
                        }
                    }
                } else {
                    Err(anyhow!("Failed - no current action"))
                }
            }
            ShipAction::NavigateToWaypoint => {
                if let Some(action) = &state.current_travel_action {
                    match action {
                        TravelAction::Navigate { to, .. } => {
                            let new_nav = state.navigate(to).await?;
                            state.set_nav(new_nav.clone());
                            Ok(Success)
                        }
                        TravelAction::Refuel { .. } => Err(anyhow!(
                            "Failed - can't navigate - current action is refuel action"
                        )),
                    }
                } else {
                    Err(anyhow!("Failed - no current action"))
                }
            }
            ShipAction::PrintTravelActions => {
                println!(
                    "current travel action: {:?}\ntravel_action queue: {:?}",
                    state.current_travel_action, state.travel_action_queue
                );
                Ok(Success)
            }
            ShipAction::HasExploreLocationEntry => {
                let no_explore_location_left = state.explore_location_queue.is_empty()
                    && state.current_explore_location.is_none();
                if no_explore_location_left {
                    Err(anyhow!("no_explore_location_left"))
                } else {
                    Ok(Success)
                }
            }
            ShipAction::PopExploreLocationAsDestination => {
                state.pop_explore_location();
                Ok(Success)
            }
            ShipAction::HasActiveExploreLocationEntry => {
                if state.current_explore_location.is_some() {
                    Ok(Success)
                } else {
                    Err(anyhow!("No active explore_destination"))
                }
            }
            ShipAction::PrintExploreLocations => {
                println!(
                    "current explore location: {:?}\nexplore_location_queue: {:?}",
                    state.current_explore_location, state.explore_location_queue
                );
                Ok(Success)
            }
            ShipAction::HasDestination => {
                if state.current_navigation_destination.is_some() {
                    Ok(Success)
                } else {
                    Err(anyhow!("No active navigation_destination"))
                }
            }
            ShipAction::SetExploreLocationAsDestination => {
                state.current_navigation_destination = state.current_explore_location.clone();
                Ok(Success)
            }

            ShipAction::IsAtDestination => {
                if let Some(current) = &state.current_navigation_destination {
                    if &state.nav.waypoint_symbol == current {
                        Ok(Success)
                    } else {
                        Err(anyhow!("Not at destination"))
                    }
                } else {
                    Err(anyhow!("No active navigation_destination"))
                }
            }
            ShipAction::HasRouteToDestination => {
                if let Some((current_destination, last_travel_action)) = state
                    .current_navigation_destination
                    .clone()
                    .zip(state.last_travel_action())
                {
                    if current_destination == *last_travel_action.waypoint_and_time().0 {
                        Ok(Success)
                    } else {
                        Err(anyhow!("Last entry of travel_actions {:?} doesn't match the current destination {:?}", last_travel_action, current_destination))
                    }
                } else {
                    Err(anyhow!(
                        "No active navigation_destination or no last_travel_action"
                    ))
                }
            }
            ShipAction::ComputePathToDestination => {
                let from = state.nav.waypoint_symbol.clone();
                let to = state.current_navigation_destination.clone().unwrap();
                let path: Vec<TravelAction> = args.compute_path(from, to, state.get_ship()).await?;

                state.set_route(path);
                Ok(Success)
            }
            ShipAction::CollectWaypointInfos => {
                let exploration_tasks = args
                    .get_exploration_tasks_for_current_waypoint(state.nav.waypoint_symbol.clone())
                    .await
                    .map_err(|_| anyhow!("inserting waypoint failed"))?;

                if exploration_tasks.contains(&ExplorationTask::CreateChart) {
                    let charted_waypoint = state.chart_waypoint().await?;
                    args.insert_waypoint(&charted_waypoint.waypoint)
                        .await
                        .map_err(|_| anyhow!("inserting waypoint failed"))?;
                }

                let exploration_tasks = args
                    .get_exploration_tasks_for_current_waypoint(state.nav.waypoint_symbol.clone())
                    .await?;

                for task in exploration_tasks {
                    match task {
                        ExplorationTask::CreateChart => {
                            return Err(anyhow!("Waypoint should have been charted by now"))
                        }
                        ExplorationTask::GetMarket => {
                            let market = state.get_market().await?;
                            args.insert_market(market).await?;
                        }
                        ExplorationTask::GetJumpGate => {
                            let jump_gate = state.get_jump_gate().await?;
                            args.insert_jump_gate(jump_gate).await?;
                        }
                        ExplorationTask::GetShipyard => {
                            let shipyard = state.get_shipyard().await?;
                            args.insert_shipyard(shipyard).await?;
                        }
                    }
                }
                Ok(Success)
            }
            ShipAction::MarkExploreLocationAsComplete => {
                state.current_explore_location = None;
                Ok(Success)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::behavior_tree::actions::TestObjects;
    use crate::behavior_tree::behavior_tree::Actionable;
    use crate::behavior_tree::behavior_tree::Response;
    use crate::behavior_tree::ship_behaviors::ship_navigation_behaviors;
    use crate::pagination::{PaginatedResponse, PaginationInput};
    use crate::ship::ShipOperations;
    use crate::st_client::{Data, StClientTrait};
    use crate::st_model::{
        AgentInfoResponse, AgentSymbol, DockShipResponse, FlightMode, GetConstructionResponse,
        GetMarketResponse, ListAgentsResponse, NavResponse, NavStatus, NavigateShipResponse,
        OrbitShipResponse, PatchShipNavResponse, RefuelShipResponse, RegistrationRequest,
        RegistrationResponse, Ship, StStatusResponse, SystemSymbol, SystemsPageData, Waypoint,
        WaypointSymbol,
    };
    use async_trait::async_trait;
    use mockall::mock;
    use mockall::predicate::*;
    use std::sync::Arc;

    mock! {
            #[derive(Debug)]
            pub StClient {}

            #[async_trait]
            impl StClientTrait for StClient { async fn register(&self, registration_request: RegistrationRequest) -> anyhow::Result<Data<RegistrationResponse>> {}

        async fn get_public_agent(&self, agent_symbol: &AgentSymbol) -> anyhow::Result<AgentInfoResponse> {}

        async fn get_agent(&self) -> anyhow::Result<AgentInfoResponse> {}

        async fn get_construction_site(&self, waypoint_symbol: &WaypointSymbol) -> anyhow::Result<GetConstructionResponse> {}

        async fn dock_ship(&self, ship_symbol: String) -> anyhow::Result<DockShipResponse> {}

        async fn set_flight_mode(&self, ship_symbol: String, mode: &FlightMode) -> anyhow::Result<PatchShipNavResponse> {}

        async fn navigate(&self, ship_symbol: String, to: &WaypointSymbol) -> anyhow::Result<NavigateShipResponse> {}

    async fn refuel(&self, ship_symbol: String, amount: u32, from_cargo: bool) -> anyhow::Result<RefuelShipResponse> {}

        async fn orbit_ship(&self, ship_symbol: String) -> anyhow::Result<OrbitShipResponse> {}

        async fn list_ships(&self, pagination_input: PaginationInput) -> anyhow::Result<PaginatedResponse<Ship>> {}

        async fn list_waypoints_of_system_page(&self, system_symbol: &SystemSymbol, pagination_input: PaginationInput) -> anyhow::Result<PaginatedResponse<Waypoint >> {}

        async fn list_systems_page(&self, pagination_input: PaginationInput) -> anyhow::Result<PaginatedResponse<SystemsPageData>> {}

        async fn get_marketplace(&self, waypoint_symbol: WaypointSymbol) -> anyhow::Result<GetMarketResponse> {}

        async fn list_agents_page(&self, pagination_input: PaginationInput) -> anyhow::Result<ListAgentsResponse> {}

        async fn get_status(&self) -> anyhow::Result<StStatusResponse> {}
            }
        }

    #[tokio::test]
    async fn test_experiment_with_mockall() {
        let mut mock_client = MockStClient::new();

        mock_client
            .expect_dock_ship()
            .with(eq("FLWI-1".to_string()))
            .return_once(move |_| {
                Ok(DockShipResponse {
                    data: NavResponse {
                        nav: TestObjects::create_nav(),
                    },
                })
            });

        let ship = TestObjects::test_ship();

        let mut ship_ops = ShipOperations::new(ship, Arc::new(mock_client));
        let result = ship_ops.dock().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dock_if_necessary_behavior_on_docked_ship() {
        let mut mock_client = MockStClient::new();

        let mocked_client = mock_client
            .expect_dock_ship()
            .with(eq("FLWI-1".to_string()))
            .returning(move |_| {
                Ok(DockShipResponse {
                    data: NavResponse {
                        nav: TestObjects::create_nav(),
                    },
                })
            });

        // if ship is docked

        let mut ship = TestObjects::test_ship();
        ship.nav.status = NavStatus::Docked;

        let behaviors = ship_navigation_behaviors();
        let ship_behavior = behaviors.dock_if_necessary;

        mocked_client.never();

        let mut ship_ops = ShipOperations::new(ship, Arc::new(mock_client));
        let result = ship_behavior.run(&(), &mut ship_ops).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dock_if_necessary_behavior_on_orbiting_ship() {
        let mut mock_client = MockStClient::new();

        let mocked_client = mock_client
            .expect_dock_ship()
            .with(eq("FLWI-1".to_string()))
            .returning(move |_| {
                Ok(DockShipResponse {
                    data: NavResponse {
                        nav: TestObjects::create_nav(),
                    },
                })
            });

        // if ship is docked

        let mut ship = TestObjects::test_ship();
        ship.nav.status = NavStatus::InOrbit;

        let behaviors = ship_navigation_behaviors();
        let ship_behavior = behaviors.dock_if_necessary;

        mocked_client.times(1);

        let mut ship_ops = ShipOperations::new(ship, Arc::new(mock_client));
        let result = ship_behavior.run(&(), &mut ship_ops).await.unwrap();
        assert_eq!(result, Response::Success);
    }
}

struct TestObjects;
use crate::st_model::{
    Cargo, Cooldown, Crew, Engine, FlightMode, Frame, Fuel, FuelConsumed, Nav, Reactor,
    Registration, Requirements, Route, Ship, SystemSymbol, Waypoint, WaypointSymbol,
};

impl TestObjects {
    pub fn create_nav() -> Nav {
        Nav {
            system_symbol: SystemSymbol("X1-FOO".to_string()),
            waypoint_symbol: WaypointSymbol("X1-FOO-BAR".to_string()),
            route: Route {
                destination: NavRouteWaypoint {
                    symbol: WaypointSymbol("X1-FOO-BAR".to_string()),
                    waypoint_type: WaypointType::PLANET,
                    system_symbol: SystemSymbol("X1-FOO".to_string()),
                    x: 0,
                    y: 0,
                },
                origin: NavRouteWaypoint {
                    symbol: WaypointSymbol("X1-FOO-BAR".to_string()),
                    waypoint_type: WaypointType::PLANET,
                    system_symbol: SystemSymbol("X1-FOO".to_string()),
                    x: 0,
                    y: 0,
                },
                departure_time: Default::default(),
                arrival: Default::default(),
            },
            status: NavStatus::InTransit,
            flight_mode: FlightMode::Drift,
        }
    }

    pub fn test_ship() -> Ship {
        Ship {
            symbol: ShipSymbol("FLWI-1".to_string()),
            registration: Registration {
                name: "FLWI".to_string(),
                faction_symbol: "GALACTIC".to_string(),
                role: "".to_string(),
            },
            nav: Self::create_nav(),
            crew: Crew {
                current: 0,
                required: 0,
                capacity: 0,
                rotation: "".to_string(),
                morale: 0,
                wages: 0,
            },
            frame: Frame {
                symbol: "".to_string(),
                name: "".to_string(),
                description: "".to_string(),
                condition: 0.0,
                integrity: 0.0,
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
                condition: 0.0,
                integrity: 0.0,
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
                condition: 0.0,
                integrity: 0.0,
                speed: 0,
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
                current: 0,
                capacity: 0,
                consumed: FuelConsumed {
                    amount: 0,
                    timestamp: Default::default(),
                },
            },
        }
    }
}
