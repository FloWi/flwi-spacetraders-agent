use crate::behavior_tree::behavior_tree::{Actionable, Response};
use crate::behavior_tree::ship_behaviors::ShipAction;
use crate::pathfinder::pathfinder::TravelAction;
use crate::ship::ShipOperations;
use crate::st_model::NavStatus;
use anyhow::anyhow;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tracing::Span;

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

            ShipAction::IsCorrectFlightMode => Err(anyhow!("Failed")),
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
            ShipAction::CanSkipRefueling => {
                println!("TODO - calculate CanSkipRefueling");
                Ok(Response::Success)
            }

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
            ShipAction::Navigate => Err(anyhow!("Failed")),
            ShipAction::SetFlightMode => Err(anyhow!("Failed")),
            ShipAction::NavigateToWaypoint => Err(anyhow!("Failed")),
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
