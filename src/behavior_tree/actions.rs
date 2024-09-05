use crate::behavior_tree::behavior_tree::ShipAction;
use crate::pathfinder::pathfinder::TravelAction;
use crate::ship::ShipOperations;
use crate::st_model::NavStatus;
use bonsai_bt::{Event, Status, Timer, UpdateArgs, BT, RUNNING};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::mpsc::channel;
use tracing::{event, Level};

pub async fn ship_tick(
    timer: &mut Timer,
    bt: &mut BT<ShipAction, HashMap<String, String>>,
    state: &mut ShipOperations,
) -> (Status, f64) {
    // timer since bt was first invoked
    let _t = timer.duration_since_start();

    // have bt advance dt seconds into the future
    let dt = timer.get_dt();

    // proceed to next iteration in event loop
    let e: Event = UpdateArgs { dt }.into();

    bt.tick(&e, &mut |args: bonsai_bt::ActionArgs<Event, ShipAction>,
                      blackboard| {
        let result = match *args.action {
            ShipAction::HasTravelActionEntry => {
                let no_action_left = state.route.is_empty() && state.current_action.is_none();
                if no_action_left {
                    (Status::Success, args.dt)
                } else {
                    RUNNING
                }
            }
            ShipAction::PopTravelAction => {
                state.pop_travel_action();
                (Status::Success, args.dt)
            }

            ShipAction::IsNavigationAction => match state.current_action {
                Some(TravelAction::Navigate { .. }) => (Status::Success, args.dt),
                _ => (Status::Failure, args.dt),
            },

            ShipAction::IsRefuelAction => match state.current_action {
                Some(TravelAction::Refuel { .. }) => (Status::Success, args.dt),
                _ => (Status::Failure, args.dt),
            },

            ShipAction::WaitForArrival => match state.nav.status {
                NavStatus::InTransit | NavStatus::Docked => (Status::Success, args.dt),
                NavStatus::InOrbit => {
                    let now: DateTime<Utc> = Utc::now();
                    let arrival_time: DateTime<Utc> = state.nav.route.arrival;
                    let is_still_travelling: bool = now < arrival_time;

                    if is_still_travelling {
                        RUNNING
                    } else {
                        (Status::Success, args.dt)
                    }
                }
            },

            ShipAction::FixNavStatusIfNecessary => match state.nav.status {
                NavStatus::InTransit | NavStatus::Docked => (Status::Success, args.dt),
                NavStatus::InOrbit => {
                    let now: DateTime<Utc> = Utc::now();
                    let arrival_time: DateTime<Utc> = state.nav.route.arrival;
                    let is_still_travelling: bool = now < arrival_time;

                    if !is_still_travelling {
                        state.nav.status = NavStatus::InOrbit;
                    }
                    (Status::Success, args.dt)
                }
            },

            ShipAction::IsDocked => match state.nav.status {
                NavStatus::Docked => (Status::Success, args.dt),
                NavStatus::InOrbit | NavStatus::InTransit => (Status::Failure, args.dt),
            },

            ShipAction::IsInOrbit => match state.nav.status {
                NavStatus::InOrbit => (Status::Success, args.dt),
                NavStatus::InTransit | NavStatus::Docked => (Status::Failure, args.dt),
            },

            ShipAction::IsCorrectFlightMode => (Status::Failure, args.dt),
            ShipAction::MarkTravelActionAsCompleteIfPossible => match &state.current_action {
                None => (Status::Success, args.dt),

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
                    (Status::Success, args.dt)
                }
            },
            ShipAction::CanSkipRefueling => {
                println!("TODO - calculate CanSkipRefueling");
                (Status::Success, args.dt)
            }

            ShipAction::Refuel => (Status::Failure, args.dt),
            ShipAction::Dock => (Status::Failure, args.dt),
            ShipAction::Orbit => {
                // wild hack talking to a sync::mpsc channel from an async action
                let (tx, rx) = channel();
                let mut state_clone = state.clone();
                let action = async move {
                    println!("Calling orbit endpoint");
                    let new_nav = state_clone.orbit().await;
                    println!("Called orbit endpoint successfully");
                    tx.send(new_nav).unwrap();
                };

                println!("spawning orbit task");
                tokio::spawn(action);
                println!("done spawning orbit task. Waiting for callback");
                let new_nav = rx.recv().unwrap().unwrap();
                println!("Got callback");
                state.set_nav(new_nav);

                (Status::Failure, args.dt)
            }
            ShipAction::Navigate => (Status::Failure, args.dt),
            ShipAction::SetFlightMode => (Status::Failure, args.dt),
            ShipAction::NavigateToWaypoint => (Status::Failure, args.dt),
        };

        event!(
            Level::INFO,
            "Executed {:?} - result {:?}",
            args.action,
            result
        );
        result
    })
}
