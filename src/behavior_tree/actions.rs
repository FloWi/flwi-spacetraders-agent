use crate::behavior_tree::behavior_tree::{Actionable, Response};
use crate::behavior_tree::ship_behaviors::ShipAction;
use crate::pathfinder::pathfinder::TravelAction;
use crate::ship::ShipOperations;
use crate::st_model::NavStatus;
use anyhow::anyhow;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[async_trait]
impl Actionable for ShipAction {
    type ActionError = anyhow::Error;
    type ActionArgs = ();
    type ActionState = ShipOperations;

    async fn run(
        &self,
        args: &Self::ActionArgs,
        state: &mut Self::ActionState,
    ) -> Result<Response, Self::ActionError> {
        match self {
            ShipAction::HasActiveNavigationNode => {
                if state.current_action.is_some() {
                    Ok(Response::Success)
                } else {
                    Err(anyhow!("No active node"))
                }
            }

            ShipAction::HasTravelActionEntry => {
                let no_action_left = state.route.is_empty() && state.current_action.is_none();
                if no_action_left {
                    Ok(Response::Success)
                } else {
                    Ok(Response::Running)
                }
            }
            ShipAction::PopTravelAction => {
                state.pop_travel_action();
                Ok(Response::Success)
            }

            ShipAction::IsNavigationAction => match state.current_action {
                Some(TravelAction::Navigate { .. }) => Ok(Response::Success),
                _ => Err(anyhow!("Failed")),
            },

            ShipAction::IsRefuelAction => match state.current_action {
                Some(TravelAction::Refuel { .. }) => Ok(Response::Success),
                _ => Err(anyhow!("Failed")),
            },

            ShipAction::WaitForArrival => match state.nav.status {
                NavStatus::InTransit | NavStatus::Docked => Ok(Response::Success),
                NavStatus::InOrbit => {
                    let now: DateTime<Utc> = Utc::now();
                    let arrival_time: DateTime<Utc> = state.nav.route.arrival;
                    let is_still_travelling: bool = now < arrival_time;

                    if is_still_travelling {
                        Ok(Response::Running)
                    } else {
                        Ok(Response::Success)
                    }
                }
            },

            ShipAction::FixNavStatusIfNecessary => match state.nav.status {
                NavStatus::InTransit | NavStatus::Docked => Ok(Response::Success),
                NavStatus::InOrbit => {
                    let now: DateTime<Utc> = Utc::now();
                    let arrival_time: DateTime<Utc> = state.nav.route.arrival;
                    let is_still_travelling: bool = now < arrival_time;

                    if !is_still_travelling {
                        state.nav.status = NavStatus::InOrbit;
                    }
                    Ok(Response::Success)
                }
            },

            ShipAction::IsDocked => match state.nav.status {
                NavStatus::Docked => Ok(Response::Success),
                NavStatus::InOrbit | NavStatus::InTransit => Err(anyhow!("Failed")),
            },

            ShipAction::IsInOrbit => match state.nav.status {
                NavStatus::InOrbit => Ok(Response::Success),
                NavStatus::InTransit | NavStatus::Docked => Err(anyhow!("Failed")),
            },

            ShipAction::IsCorrectFlightMode => {
                if let Some(action) = &state.current_action {
                    match action {
                        TravelAction::Navigate { mode, .. } => {
                            let current_mode = &state.get_ship().nav.flight_mode;
                            if current_mode == mode {
                                Ok(Response::Success)
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
            ShipAction::MarkTravelActionAsCompleteIfPossible => match &state.current_action {
                None => Ok(Response::Success),
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
                        state.current_action = None;
                    }
                    Ok(Response::Success)
                }
            },
            ShipAction::CanSkipRefueling => match &state.current_action {
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
                    let maybe_navigate_action: Option<&TravelAction> = state.route.get(0);
                    let maybe_refuel_action: Option<&TravelAction> = state.route.get(1);

                    if let Some((
                        TravelAction::Navigate {
                            fuel_consumption, ..
                        },
                        TravelAction::Refuel { .. },
                    )) = maybe_navigate_action.zip(maybe_refuel_action)
                    {
                        let has_enough_fuel = state.fuel.current >= (*fuel_consumption as i32);
                        if has_enough_fuel {
                            Ok(Response::Success)
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

            ShipAction::Refuel => Err(anyhow!("Failed")),
            ShipAction::Dock => {
                let new_nav = state.dock().await?;
                state.set_nav(new_nav);
                Ok(Response::Success)
            }
            ShipAction::Orbit => {
                let new_nav = state.orbit().await?;
                state.set_nav(new_nav);
                Ok(Response::Success)
            }

            ShipAction::SetFlightMode => {
                if let Some(action) = &state.current_action {
                    match action {
                        TravelAction::Navigate { mode, .. } => {
                            let new_nav = state.set_flight_mode(mode).await?;
                            state.set_nav(new_nav);
                            Ok(Response::Success)
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
                if let Some(action) = &state.current_action {
                    match action {
                        TravelAction::Navigate { to, .. } => {
                            let new_nav = state.navigate(to).await?;
                            state.set_nav(new_nav.clone());
                            Ok(Response::Success)
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
                    "current action: {:?}\nqueue: {:?}",
                    state.current_action, state.route
                );
                Ok(Response::Success)
            }
        }
    }
}
