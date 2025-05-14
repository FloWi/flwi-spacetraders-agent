use crate::behavior_tree::behavior_args::BehaviorArgs;
use crate::behavior_tree::behavior_tree::Response::Success;
use crate::behavior_tree::behavior_tree::{ActionEvent, Actionable, Response};
use crate::behavior_tree::ship_behaviors::ShipAction;
use crate::ship::ShipOperations;
use anyhow::Result;
use anyhow::{anyhow, Error};
use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, Utc};
use core::time::Duration;
use itertools::Itertools;
use st_domain::budgeting::budgeting::{
    PurchaseShipTransactionGoal, PurchaseTradeGoodsTransactionGoal, SellTradeGoodsTransactionGoal, TicketStatus, TicketType, TransactionGoal, TransactionTicket,
};
use st_domain::TransactionActionEvent::{PurchasedShip, PurchasedTradeGoods, SoldTradeGoods, SuppliedConstructionSite};
use st_domain::{
    get_exploration_tasks_for_waypoint, ExplorationTask, NavStatus, OperationExpenseEvent, RefuelShipResponse, RefuelShipResponseBody, TradeGoodSymbol,
    TradeTicket, TravelAction, WaypointSymbol,
};
use std::ops::{Add, Not};
use tokio::sync::mpsc::Sender;
use tracing::event;
use tracing_core::Level;

#[async_trait]
impl Actionable for ShipAction {
    type ActionError = Error;
    type ActionArgs = BehaviorArgs;
    type ActionState = ShipOperations;

    async fn run(
        &self,
        args: &Self::ActionArgs,
        state: &mut Self::ActionState,
        sleep_duration: Duration,
        state_changed_tx: &Sender<Self::ActionState>,
        action_completed_tx: &Sender<ActionEvent>,
    ) -> Result<Response, Self::ActionError> {
        let result = match self {
            ShipAction::HasTravelActionEntry => {
                let no_action_left = state.travel_action_queue.is_empty();
                if no_action_left {
                    Err(anyhow!("No action left"))
                } else {
                    Ok(Success)
                }
            }
            ShipAction::PopTravelAction => {
                if state.travel_action_queue.is_empty() {
                    Err(anyhow!("PopTravelAction called, but queue is empty"))
                } else {
                    state.pop_travel_action();
                    Ok(Success)
                }
            }

            ShipAction::IsNavigationAction => match state.current_travel_action() {
                Some(TravelAction::Navigate { .. }) => Ok(Success),
                _ => Err(anyhow!("Failed")),
            },

            ShipAction::IsRefuelAction => match state.current_travel_action() {
                Some(TravelAction::Refuel { .. }) => Ok(Success),
                _ => Err(anyhow!("Failed")),
            },

            ShipAction::WaitForArrival => match state.nav.status {
                NavStatus::Docked | NavStatus::InOrbit => {
                    event!(Level::DEBUG, "ShipAction::WaitForArrival: Ship is {:?}", state.nav.status);
                    Ok(Success)
                }
                NavStatus::InTransit => {
                    let now: DateTime<Utc> = Utc::now();
                    let arrival_time: DateTime<Utc> = state.nav.route.arrival;

                    let is_still_travelling: bool = now < arrival_time;
                    event!(
                        Level::DEBUG,
                        "ShipAction::WaitForArrival: Ship is InTransit. now: {} arrival_time: {} is_still_travelling: {}",
                        now,
                        arrival_time,
                        is_still_travelling
                    );

                    if is_still_travelling {
                        let duration = arrival_time - now;
                        event!(Level::DEBUG, "WaitForArrival: {duration:?}");
                        tokio::time::sleep(Duration::from_millis(u64::try_from(duration.num_milliseconds()).unwrap_or(0))).await;
                        Ok(Response::Success)
                    } else {
                        Ok(Success)
                    }
                }
            },

            ShipAction::FixNavStatusIfNecessary => match state.nav.status {
                NavStatus::InOrbit | NavStatus::Docked => Ok(Success),
                NavStatus::InTransit => {
                    let now: DateTime<Utc> = Utc::now();
                    let arrival_time: DateTime<Utc> = state.nav.route.arrival;
                    let is_still_travelling: bool = now < arrival_time;

                    if !is_still_travelling {
                        state.nav.status = NavStatus::InOrbit;
                    } else {
                        event!(
                            Level::DEBUG,
                            "FixNavStatusIfNecessary: ship is InTransit, but arrival_time {:?} hasn't been reached yet",
                            arrival_time
                        );
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
                if let Some(action) = state.current_travel_action() {
                    match action {
                        TravelAction::Navigate { mode, .. } => {
                            let current_mode = &state.get_ship().nav.flight_mode;
                            if current_mode == mode {
                                Ok(Success)
                            } else {
                                Err(anyhow!("Failed - current mode {} != wanted mode {}", current_mode, mode))
                            }
                        }
                        TravelAction::Refuel { .. } => Err(anyhow!("Failed - no travel mode on refuel action")),
                    }
                } else {
                    Err(anyhow!("Failed - no current action"))
                }
            }
            ShipAction::MarkTravelActionAsCompleteIfPossible => match state.current_travel_action() {
                None => Ok(Success),
                Some(action) => {
                    let is_done = match action {
                        TravelAction::Navigate { to, .. } => {
                            let is_arrived = state.nav.waypoint_symbol == *to && state.nav.status != NavStatus::InTransit;
                            if !is_arrived {
                                event!(Level::DEBUG, "MarkTravelActionAsCompleteIfPossible: ship has not arrived yet");
                            }
                            is_arrived
                        }
                        TravelAction::Refuel { at, .. } => {
                            let has_refueled =
                                state.nav.waypoint_symbol == *at && state.nav.status != NavStatus::InTransit && state.fuel.current == state.fuel.capacity;
                            if !has_refueled {
                                event!(Level::DEBUG, "MarkTravelActionAsCompleteIfPossible: ship has not refueled yet");
                            }

                            has_refueled
                        }
                    };

                    if is_done {
                        state.pop_travel_action();
                    }
                    Ok(Success)
                }
            },
            ShipAction::CanSkipRefueling => match state.current_travel_action() {
                None => Err(anyhow!("Called CanSkipRefueling, but current action is None",)),
                Some(TravelAction::Navigate { .. }) => Err(anyhow!("Called CanSkipRefueling, but current action is Navigate",)),
                Some(TravelAction::Refuel { at, .. }) => {
                    // we can skip refueling, if
                    // - queued_action #1 is: go_to_waypoint X
                    // - queued_action #2 is: refuel_at_waypoint X
                    // - we have enough fuel to reach X in desired flight mode without refueling
                    let maybe_navigate_action: Option<&TravelAction> = state.travel_action_queue.front();
                    let maybe_refuel_action: Option<&TravelAction> = state.travel_action_queue.get(1);

                    if let Some((TravelAction::Navigate { fuel_consumption, .. }, TravelAction::Refuel { .. })) = maybe_navigate_action.zip(maybe_refuel_action)
                    {
                        let has_enough_fuel = state.fuel.current >= (*fuel_consumption as i32);
                        if has_enough_fuel {
                            Ok(Success)
                        } else {
                            Err(anyhow!("Called CanSkipRefueling, but not enough fuel to reach destination",))
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
            ShipAction::SkipRefueling => match state.current_travel_action() {
                Some(TravelAction::Refuel { .. }) => {
                    state.pop_travel_action();
                    Ok(Success)
                }
                _ => Err(anyhow!(
                    "Called SkipRefueling, but current_travel_action is {:?}",
                    state.current_travel_action()
                )),
            },

            ShipAction::Refuel => {
                let ref response @ RefuelShipResponse {
                    data: RefuelShipResponseBody { fuel: ref new_fuel, .. },
                } = state.refuel(false).await?;
                state.set_fuel(new_fuel.clone());
                action_completed_tx
                    .send(ActionEvent::Expense(
                        state.clone(),
                        OperationExpenseEvent::RefueledShip { response: response.clone() },
                    ))
                    .await?;

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
                if let Some(action) = state.current_travel_action() {
                    match action {
                        TravelAction::Navigate { mode, .. } => {
                            let response = state.set_flight_mode(mode).await?;
                            state.set_nav(response.nav);
                            state.set_fuel(response.fuel);
                            Ok(Success)
                        }
                        TravelAction::Refuel { .. } => Err(anyhow!("Failed - no travel mode on refuel action")),
                    }
                } else {
                    Err(anyhow!("Failed - no current action"))
                }
            }
            ShipAction::NavigateToWaypoint => {
                if let Some(action) = state.current_travel_action() {
                    match action {
                        TravelAction::Navigate { to, .. } => {
                            let nav_response = state.navigate(to).await?;
                            state.set_nav(nav_response.nav.clone());
                            state.set_fuel(nav_response.fuel.clone());
                            Ok(Success)
                        }
                        TravelAction::Refuel { .. } => Err(anyhow!("Failed - can't navigate - current action is refuel action")),
                    }
                } else {
                    Err(anyhow!("Failed - no current action"))
                }
            }
            ShipAction::PrintTravelActions => {
                event!(Level::DEBUG, "travel_action queue: {:?}", state.travel_action_queue);
                Ok(Success)
            }
            ShipAction::HasExploreLocationEntry => {
                let no_explore_location_left = state.explore_location_queue.is_empty();
                if no_explore_location_left {
                    Err(anyhow!("no_explore_location_left"))
                } else {
                    Ok(Success)
                }
            }
            ShipAction::PopExploreLocationAsDestination => {
                state.pop_explore_location_as_destination();
                Ok(Success)
            }

            ShipAction::PrintExploreLocations => {
                event!(Level::DEBUG, "explore_location_queue: {:?}", state.explore_location_queue);
                Ok(Success)
            }
            ShipAction::PrintDestination => {
                event!(Level::DEBUG, "current_navigation_destination: {:?}", state.current_navigation_destination);
                Ok(Success)
            }

            ShipAction::RemoveDestination => {
                state.current_navigation_destination = None;
                Ok(Success)
            }

            ShipAction::HasDestination => {
                if state.current_navigation_destination.is_some() {
                    Ok(Success)
                } else {
                    Err(anyhow!("No active navigation_destination"))
                }
            }

            ShipAction::HasUncompletedTrade => match &state.maybe_trade {
                None => Err(anyhow!("No trade assigned")),
                Some(trade) => {
                    if trade.completed_at.is_some() {
                        Err(anyhow!("Trade is complete"))
                    } else {
                        Ok(Success)
                    }
                }
            },

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

            ShipAction::IsAtObservationWaypoint => {
                if let Some(current) = &state.permanent_observation_location {
                    if &state.nav.waypoint_symbol == current {
                        Ok(Success)
                    } else {
                        Err(anyhow!("Not at observation location"))
                    }
                } else {
                    Err(anyhow!("No active permanent_observation_location"))
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
                        Err(anyhow!(
                            "Last entry of travel_actions {:?} doesn't match the current destination {:?}",
                            last_travel_action,
                            current_destination
                        ))
                    }
                } else {
                    Err(anyhow!("No active navigation_destination or no last_travel_action"))
                }
            }
            ShipAction::ComputePathToDestination => {
                let from = state.nav.waypoint_symbol.clone();
                let to = state.current_navigation_destination.clone().unwrap();
                let ship = state.get_ship();
                let path: Vec<TravelAction> = args
                    .compute_path(
                        from.clone(),
                        to.clone(),
                        ship.engine.speed as u32,
                        ship.fuel.current as u32,
                        ship.fuel.capacity as u32,
                    )
                    .await?;

                event!(Level::DEBUG, "successfully computed route from {:?} to {:?}: {:?}", from, to, &path);
                state.set_route(path);
                Ok(Success)
            }
            ShipAction::CollectWaypointInfos => {
                let exploration_tasks = args
                    .get_waypoint(&state.nav.waypoint_symbol.clone())
                    .await
                    .map(|wp| get_exploration_tasks_for_waypoint(&wp))
                    .map_err(|_| anyhow!("inserting waypoint failed"))?;

                let is_uncharted = exploration_tasks.contains(&ExplorationTask::CreateChart);
                if is_uncharted {
                    let charted_waypoint = state.chart_waypoint().await?;
                    args.insert_waypoint(&charted_waypoint.waypoint)
                        .await
                        .map_err(|_| anyhow!("inserting waypoint failed"))?;
                }

                let exploration_tasks = if is_uncharted {
                    args.get_exploration_tasks_waypoint(&state.nav.waypoint_symbol)
                        .await?
                } else {
                    exploration_tasks
                };

                event!(Level::DEBUG, "CollectWaypointInfos - exploration_tasks: {:?}", exploration_tasks);

                for task in exploration_tasks {
                    match task {
                        ExplorationTask::CreateChart => return Err(anyhow!("Waypoint should have been charted by now")),
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

            ShipAction::HasPermanentExploreLocationEntry => match state.permanent_observation_location {
                None => Err(anyhow!("No permanent_observation_location")),
                Some(_) => Ok(Success),
            },
            ShipAction::SetPermanentExploreLocationAsDestination => match state.permanent_observation_location.clone() {
                None => Err(anyhow!("No permanent_observation_location")),
                Some(waypoint) => {
                    state.set_destination(waypoint);
                    Ok(Success)
                }
            },
            ShipAction::SetNextObservationTime => {
                let now = Utc::now();
                state.set_next_observation_time(now.add(TimeDelta::minutes(10)));
                Ok(Success)
            }
            ShipAction::IsLateEnoughForWaypointObservation => match state.maybe_next_observation_time {
                None => Ok(Success),
                Some(next_time) => {
                    if next_time < Utc::now() {
                        Ok(Success)
                    } else {
                        Err(anyhow!("Not enough time has passed"))
                    }
                }
            },
            ShipAction::SetNextTradeStopAsDestination => match state.maybe_trade.clone() {
                None => Err(anyhow!("No next trade waypoint found - state.maybe_trade is None")),
                Some(trade) => {
                    let incomplete_goals: Vec<TransactionGoal> = trade.get_incomplete_goals();
                    // get all purchases
                    // get all sales (for which we already executed the purchase)

                    let candidate_waypoints = incomplete_goals
                        .iter()
                        .filter_map(|g| match g {
                            TransactionGoal::PurchaseTradeGoods(PurchaseTradeGoodsTransactionGoal { source_waypoint, .. }) => Some(source_waypoint.clone()),
                            TransactionGoal::SellTradeGoods(SellTradeGoodsTransactionGoal {
                                good,
                                target_quantity,
                                destination_waypoint,
                                ..
                            }) => {
                                let is_in_cargo = state
                                    .cargo
                                    .inventory
                                    .iter()
                                    .any(|inventory| &inventory.symbol == good && &inventory.units >= target_quantity);
                                is_in_cargo.then_some(destination_waypoint.clone())
                            }
                            TransactionGoal::PurchaseShip(PurchaseShipTransactionGoal { shipyard_waypoint, .. }) => Some(shipyard_waypoint.clone()),
                        })
                        .collect_vec();

                    let maybe_best_wps: Option<WaypointSymbol> = args
                        .get_closest_waypoint(&state.nav.waypoint_symbol, &candidate_waypoints)
                        .await?;

                    match maybe_best_wps {
                        None => Err(anyhow!("No next trade waypoint found - maybe_best_waypoint is None")),
                        Some(best_wps) => {
                            event!(
                                Level::DEBUG,
                                r#"ShipAction::SetNextTradeStopAsDestination:
                                trade: {}
                                best_wps: {}
                                "#,
                                serde_json::to_string(&trade)?,
                                best_wps.0,
                            );
                            state.set_destination(best_wps);
                            Ok(Success)
                        }
                    }
                }
            },

            ShipAction::PerformTradeActionAndMarkAsCompleted => {
                if let Some(trade) = &state.maybe_trade.clone() {
                    //trade.goals

                    let current_location = &state.nav.waypoint_symbol.clone();

                    for g in trade
                        .get_incomplete_goals()
                        .into_iter()
                        .filter(|g| g.get_waypoint() == current_location)
                    {
                        // println!("processing incomplete goal {g:?}");

                        match g {
                            TransactionGoal::PurchaseTradeGoods(purchase_transaction_goal) => {
                                let PurchaseTradeGoodsTransactionGoal { good, target_quantity, .. } = &purchase_transaction_goal;
                                let response = state
                                    .purchase_trade_good(*target_quantity, good.clone())
                                    .await?;

                                let updated_trade = args
                                    .mark_purchase_as_completed(trade.id, &purchase_transaction_goal, &response)
                                    .await?;
                                state.maybe_trade = updated_trade
                                    .is_complete()
                                    .not()
                                    .then_some(updated_trade.clone());

                                action_completed_tx
                                    .send(ActionEvent::TransactionCompleted(
                                        state.clone(),
                                        PurchasedTradeGoods {
                                            ticket_details: purchase_transaction_goal,
                                            response,
                                        },
                                        updated_trade,
                                    ))
                                    .await?;
                            }
                            TransactionGoal::SellTradeGoods(sell_transaction_goal) => {
                                let SellTradeGoodsTransactionGoal {
                                    id,
                                    good,
                                    target_quantity,
                                    sold_quantity,
                                    estimated_price_per_unit: estimated_price,
                                    min_acceptable_price_per_unit: min_acceptable_price,
                                    destination_waypoint,
                                } = &sell_transaction_goal;

                                let response = state
                                    .sell_trade_good(*target_quantity, good.clone())
                                    .await?;

                                let updated_trade = args
                                    .mark_sell_as_completed(trade.id, &sell_transaction_goal, &response)
                                    .await?;
                                state.maybe_trade = updated_trade
                                    .is_complete()
                                    .not()
                                    .then_some(updated_trade.clone());

                                action_completed_tx
                                    .send(ActionEvent::TransactionCompleted(
                                        state.clone(),
                                        SoldTradeGoods {
                                            ticket_details: sell_transaction_goal,
                                            response,
                                        },
                                        updated_trade,
                                    ))
                                    .await?;
                            }
                            TransactionGoal::PurchaseShip(ship_purchase_goal) => {
                                let PurchaseShipTransactionGoal {
                                    id,
                                    ship_type,
                                    estimated_cost,
                                    has_been_purchased,
                                    beneficiary_fleet,
                                    shipyard_waypoint,
                                } = &ship_purchase_goal;

                                let response = state.purchase_ship(ship_type, shipyard_waypoint).await?;

                                let updated_trade = args
                                    .mark_ship_purchase_as_completed(trade.id, &ship_purchase_goal, &response)
                                    .await?;
                                state.maybe_trade = updated_trade
                                    .is_complete()
                                    .not()
                                    .then_some(updated_trade.clone());

                                action_completed_tx
                                    .send(ActionEvent::TransactionCompleted(
                                        state.clone(),
                                        PurchasedShip {
                                            ticket_details: ship_purchase_goal,
                                            response,
                                        },
                                        updated_trade,
                                    ))
                                    .await?;
                            }
                        }
                    }
                    Ok(Success)
                } else {
                    Ok(Success)
                }
            }

            ShipAction::HasNextTradeWaypoint => match state.maybe_trade.clone() {
                None => Err(anyhow!("No next trade waypoint found - state.maybe_trade is None")),
                Some(trade) => {
                    if trade.get_incomplete_goals().is_empty() {
                        Err(anyhow!("No incomplete goals found"))
                    } else {
                        Ok(Success)
                    }
                }
            },
            ShipAction::SleepUntilNextObservationTimeOrShipPurchaseTicketHasBeenAssigned => match state.maybe_next_observation_time {
                None => Ok(Success),
                Some(next_time) => loop {
                    if state.maybe_trade.is_some() || Utc::now() > next_time {
                        break Ok(Success);
                    } else {
                        tokio::time::sleep(sleep_duration).await;
                    }
                },
            },
            ShipAction::WaitForAllocatedBudgetToBeAvailable => {
                let current_location = state.current_location();

                let mut sleeping_turns = 0;
                let sleep_duration = Duration::from_millis(20).max(sleep_duration);
                loop {
                    let is_funded = if let Some(trade) = &state.maybe_trade.clone() {
                        match trade.status {
                            TicketStatus::Planned => false,
                            TicketStatus::Funded => true,
                            TicketStatus::InProgress => true,
                            TicketStatus::Completed => true,
                            TicketStatus::Failed { .. } => false,
                            TicketStatus::Cancelled { .. } => false,
                        }
                    } else {
                        false
                    };

                    if is_funded {
                        break;
                    }

                    sleeping_turns += 1;

                    if sleeping_turns < 20 {
                        event!(
                            Level::DEBUG,
                            message = format!("WaitForAllocatedBudgetToBeAvailable: sleeping for {sleep_duration:?}"),
                            is_funded,
                        )
                    } else {
                        event!(
                            Level::WARN,
                            message =
                                format!("WaitForAllocatedBudgetToBeAvailable: Slept for {sleeping_turns} turns already.  sleeping for {sleep_duration:?}"),
                            is_funded,
                        )
                    }

                    tokio::time::sleep(sleep_duration).await;
                }
                Ok(Success)
            }
            ShipAction::HasShipPurchaseTicketForWaypoint => {
                let current_location = state.current_location();

                let result = if let Some(trade) = &state.maybe_trade.clone() {
                    match trade.ticket_type {
                        TicketType::Trading => Err(anyhow!("TicketType == Trading")),
                        TicketType::DeliverConstructionMaterial => Err(anyhow!("TicketType == DeliverConstructionMaterial")),
                        TicketType::Exploration => Err(anyhow!("TicketType == Exploration")),
                        TicketType::ShipPurchase => {
                            let maybe_shipyard_wp = trade.goals.iter().find_map(|tg| match tg {
                                TransactionGoal::PurchaseShip(PurchaseShipTransactionGoal { shipyard_waypoint, .. }) => Some(shipyard_waypoint.clone()),
                                TransactionGoal::PurchaseTradeGoods(PurchaseTradeGoodsTransactionGoal { .. }) => None,
                                TransactionGoal::SellTradeGoods(SellTradeGoodsTransactionGoal { .. }) => None,
                            });

                            match maybe_shipyard_wp {
                                None => Err(anyhow!("No goal of type ShipPurchase found")),
                                Some(shipyard_wp) => {
                                    if shipyard_wp == current_location {
                                        Ok(Success)
                                    } else {
                                        Err(anyhow!("Ship purchase ticket for different location"))
                                    }
                                }
                            }
                        }
                    }
                } else {
                    Err(anyhow!("No trading ticket"))
                };
                result
            }
        };

        let capacity = action_completed_tx.capacity();
        event!(
            Level::DEBUG,
            "Sending ActionEvent::ShipActionCompleted to action_completed_tx - capacity: {capacity}"
        );

        match result {
            Ok(_res) => {
                action_completed_tx
                    .send(ActionEvent::ShipActionCompleted(state.clone(), self.clone(), Ok(())))
                    .await?;
            }
            Err(err) => {
                action_completed_tx
                    .send(anyhow::bail!("Action failed {}", err))
                    .await?;
            }
        };

        result
    }
}

#[cfg(test)]
mod tests {
    use crate::behavior_tree::behavior_args::BehaviorArgs;
    use crate::behavior_tree::behavior_tree::{ActionEvent, Behavior, Response};
    use crate::behavior_tree::ship_behaviors::{ship_behaviors, ShipAction};
    use crate::ship::ShipOperations;

    use core::time::Duration;
    use mockall::predicate::*;
    use std::collections::HashMap;

    use st_domain::{
        DockShipResponse, FlightMode, GetMarketResponse, NavAndFuelResponse, NavStatus, NavigateShipResponse, SetFlightModeResponse, ShipSymbol, TravelAction,
        WaypointSymbol, WaypointTraitSymbol,
    };

    use crate::behavior_tree::behavior_tree::Response::Success;
    use crate::fleet::ship_runner::ship_behavior_runner;
    use crate::st_client::MockStClientTrait;
    use anyhow::anyhow;
    use itertools::Itertools;
    use std::sync::Arc;
    use tokio::sync::mpsc::{Receiver, Sender};

    use crate::test_objects::TestObjects;
    use st_domain::blackboard_ops::MockBlackboardOps;
    use st_domain::budgeting::treasurer::InMemoryTreasurer;
    use test_log::test;
    use tokio::sync::Mutex;

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
        let result = ship_behavior_runner(
            ship_ops,
            sleep_duration,
            &args,
            behavior,
            ship_updated_tx.clone(),
            ship_action_completed_tx.clone(),
        )
        .await;

        // Close the channels to signal collection is complete
        drop(ship_updated_tx);
        drop(ship_action_completed_tx);

        // Wait for the collectors to finish and get their results
        let received_action_state_messages = state_result_rx
            .await
            .map_err(|_| anyhow::anyhow!("Failed to receive action state messages"))?;
        let received_action_completed_messages = action_result_rx
            .await
            .map_err(|_| anyhow::anyhow!("Failed to receive action completed messages"))?;

        Ok((result?, received_action_state_messages, received_action_completed_messages))
    }

    #[test(tokio::test)]
    async fn test_experiment_with_mockall() {
        let mut mock_client = MockStClientTrait::new();

        mock_client
            .expect_dock_ship()
            .with(eq(ShipSymbol("FLWI-1".to_string())))
            .return_once(move |_| {
                Ok(DockShipResponse {
                    data: TestObjects::create_nav(
                        FlightMode::Drift,
                        NavStatus::InTransit,
                        &WaypointSymbol("X1-FOO-BAR".to_string()),
                        &WaypointSymbol("X1-FOO-BAR".to_string()),
                    ),
                })
            });

        let ship = TestObjects::test_ship(500);

        let mut ship_ops = ShipOperations::new(ship, Arc::new(mock_client));
        let result = ship_ops.dock().await;
        assert!(result.is_ok());
    }

    #[test(tokio::test)]
    async fn test_dock_if_necessary_behavior_on_docked_ship() {
        let mut mock_client = MockStClientTrait::new();

        let args = BehaviorArgs {
            blackboard: Arc::new(MockBlackboardOps::new()),
            treasurer: Arc::new(Mutex::new(InMemoryTreasurer::new(0.into()))),
        };

        let mocked_client = mock_client
            .expect_dock_ship()
            .with(eq(ShipSymbol("FLWI-1".to_string())))
            .returning(move |_| {
                Ok(DockShipResponse {
                    data: TestObjects::create_nav(
                        FlightMode::Drift,
                        NavStatus::InTransit,
                        &WaypointSymbol("X1-FOO-BAR".to_string()),
                        &WaypointSymbol("X1-FOO-BAR".to_string()),
                    ),
                })
            });

        // if ship is docked

        let mut ship = TestObjects::test_ship(500);
        ship.nav.status = NavStatus::Docked;

        let behaviors = ship_behaviors();
        let ship_behavior: Behavior<ShipAction> = behaviors.dock_if_necessary;

        mocked_client.never();

        let mut ship_ops = ShipOperations::new(ship, Arc::new(mock_client));
        let (result, ship_states, action_events) = test_run_ship_behavior(&mut ship_ops, Duration::from_millis(1), args, ship_behavior)
            .await
            .unwrap();

        assert_eq!(result, Success);
    }

    #[test(tokio::test)]
    async fn test_dock_if_necessary_behavior_on_orbiting_ship() {
        let mut mock_client = MockStClientTrait::new();

        let args = BehaviorArgs {
            blackboard: Arc::new(MockBlackboardOps::new()),
            treasurer: Arc::new(Mutex::new(InMemoryTreasurer::new(0.into()))),
        };

        let mocked_client = mock_client
            .expect_dock_ship()
            .with(eq(ShipSymbol("FLWI-1".to_string())))
            .returning(move |_| {
                Ok(DockShipResponse {
                    data: TestObjects::create_nav(
                        FlightMode::Drift,
                        NavStatus::InTransit,
                        &WaypointSymbol("X1-FOO-BAR".to_string()),
                        &WaypointSymbol("X1-FOO-BAR".to_string()),
                    ),
                })
            });

        // if ship is docked

        let mut ship = TestObjects::test_ship(500);
        ship.nav.status = NavStatus::InOrbit;

        let behaviors = ship_behaviors();
        let ship_behavior = behaviors.dock_if_necessary;

        mocked_client.times(1);

        let mut ship_ops = ShipOperations::new(ship, Arc::new(mock_client));

        let (result, ship_states, action_events) = test_run_ship_behavior(&mut ship_ops, Duration::from_millis(1), args, ship_behavior)
            .await
            .unwrap();

        assert_eq!(result, Success);
        assert_eq!(1, ship_states.len());
        assert_eq!(2, action_events.len());

        matches!(action_events.first(), Some(ActionEvent::ShipActionCompleted(_, _, Ok(_))));
        matches!(action_events.get(1), Some(ActionEvent::BehaviorCompleted(_, _, Ok(_))));
    }

    // Helper function to create a WaypointSymbol
    fn wp(s: &str) -> Arc<WaypointSymbol> {
        Arc::new(WaypointSymbol(s.to_string()))
    }

    #[test(tokio::test)]
    async fn test_explorer_behavior_with_two_waypoints() {
        let mut mock_client = MockStClientTrait::new();
        let mut mock_test_blackboard = MockBlackboardOps::new();

        let current_fuel: u32 = 500;
        let mut ship = TestObjects::test_ship(current_fuel);
        ship.nav.status = NavStatus::InOrbit;

        let waypoint_a1 = wp("X1-FOO-A1");
        let waypoint_a2 = wp("X1-FOO-A2");
        let waypoint_bar = wp("X1-FOO-BAR");
        //
        // mock_test_blackboard
        //     .expect_get_exploration_tasks_for_current_waypoint()
        //     .withf(|wp| wp.0.contains("X1-FOO-A"))
        //     .returning(|_| Ok(vec![ExplorationTask::GetMarket]));

        let explorer_waypoints = vec![
            TestObjects::create_waypoint(&waypoint_a1, 100, 0, vec![WaypointTraitSymbol::MARKETPLACE]),
            TestObjects::create_waypoint(&waypoint_a2, 200, 0, vec![WaypointTraitSymbol::MARKETPLACE]),
        ];

        let first_hop_actions = vec![TravelAction::Navigate {
            from: (*waypoint_bar).clone(),
            to: (*waypoint_a1).clone(),
            distance: 100,
            travel_time: 57,
            fuel_consumption: 200,
            mode: FlightMode::Burn,
            total_time: 57,
        }];

        let second_hop_actions = vec![TravelAction::Navigate {
            from: (*waypoint_a1).clone(),
            to: (*waypoint_a2).clone(),
            distance: 100,
            travel_time: 57,
            fuel_consumption: 200,
            mode: FlightMode::Burn,
            total_time: 57,
        }];

        mock_test_blackboard
            .expect_compute_path()
            .with(eq((*waypoint_bar).clone()), eq((*waypoint_a1).clone()), eq(30), eq(current_fuel), eq(600))
            .returning(move |_, _, _, _, _| Ok(first_hop_actions.clone()));

        let waypoint_map = explorer_waypoints
            .iter()
            .map(|wp| (wp.symbol.clone(), wp.clone()))
            .collect::<HashMap<_, _>>();

        mock_test_blackboard
            .expect_get_waypoint()
            .returning(move |wps| {
                waypoint_map
                    .get(wps)
                    .cloned()
                    .ok_or(anyhow!("Waypoint not expected"))
            });

        mock_test_blackboard
            .expect_compute_path()
            .with(eq((*waypoint_a1).clone()), eq((*waypoint_a2).clone()), eq(30), eq(300), eq(600))
            .returning(move |_, _, _, _, _| Ok(second_hop_actions.clone()));

        mock_test_blackboard
            .expect_insert_market()
            .with(mockall::predicate::always())
            .times(2)
            .returning(|_| Ok(()));

        let waypoint_a1_clone = Arc::clone(&waypoint_a1);
        let waypoint_a2_clone = Arc::clone(&waypoint_a2);
        let waypoint_bar_clone = Arc::clone(&waypoint_bar);

        mock_client
            .expect_navigate()
            .withf(|ship_symbol, to| ship_symbol == &ShipSymbol("FLWI-1".to_string()) && to.0.contains("X1-FOO-A"))
            .times(2)
            .returning(move |_, to| {
                let return_nav = if to.0.ends_with("A1") {
                    TestObjects::create_nav(FlightMode::Burn, NavStatus::InTransit, &waypoint_bar_clone, &waypoint_a1_clone)
                } else {
                    TestObjects::create_nav(FlightMode::Burn, NavStatus::InTransit, &waypoint_a1_clone, &waypoint_a2_clone)
                };
                Ok(NavigateShipResponse {
                    data: NavAndFuelResponse {
                        nav: return_nav.nav,
                        fuel: TestObjects::create_fuel(current_fuel, 200),
                    },
                })
            });

        let waypoint_bar_clone = Arc::clone(&waypoint_bar);
        mock_client
            .expect_set_flight_mode()
            .with(eq(ShipSymbol("FLWI-1".to_string())), eq(FlightMode::Burn))
            .times(1)
            .returning(move |_, _| {
                Ok(SetFlightModeResponse {
                    data: NavAndFuelResponse {
                        nav: TestObjects::create_nav(FlightMode::Burn, NavStatus::InTransit, &waypoint_bar_clone, &waypoint_bar_clone).nav,
                        fuel: TestObjects::create_fuel(current_fuel, 200),
                    },
                })
            });

        let waypoint_a1_clone = Arc::clone(&waypoint_a1);
        let waypoint_a2_clone = Arc::clone(&waypoint_a2);
        mock_client
            .expect_get_marketplace()
            .withf(|wp| wp.0.contains("X1-FOO-A"))
            .times(2)
            .returning(move |wp| {
                let market_data = if wp.0.ends_with("A1") {
                    TestObjects::create_market_data(&waypoint_a1_clone)
                } else {
                    TestObjects::create_market_data(&waypoint_a2_clone)
                };

                Ok(GetMarketResponse { data: market_data })
            });

        let behaviors = ship_behaviors();
        let mut ship_behavior = behaviors.explorer_behavior;
        ship_behavior.update_indices();

        //println!("{}", ship_behavior.to_mermaid());

        let mut ship_ops = ShipOperations::new(ship, Arc::new(mock_client));
        let args = BehaviorArgs {
            blackboard: Arc::new(mock_test_blackboard),
            treasurer: Arc::new(Mutex::new(InMemoryTreasurer::new(0.into()))),
        };

        let explorer_waypoint_symbols = explorer_waypoints
            .iter()
            .map(|wp| wp.symbol.clone())
            .collect_vec();

        ship_ops.set_explore_locations(explorer_waypoint_symbols);

        let (result, ship_states, action_events) = test_run_ship_behavior(&mut ship_ops, Duration::from_millis(1), args, ship_behavior)
            .await
            .unwrap();

        assert_eq!(result, Success);
        assert_eq!(ship_ops.nav.waypoint_symbol, *waypoint_a2);
        assert_eq!(ship_ops.travel_action_queue.len(), 0);
        assert_eq!(ship_ops.explore_location_queue.len(), 0);
    }

    /*

    #[tokio::test]
    #[traced_test]
    async fn test_navigate_to_destination_behavior() {
        let mut mock_client = MockStClientTrait::new();

        let mut mock_test_blackboard = MockBlackboardOps::new();

        let mut ship = TestObjects::test_ship(500);
        ship.nav.status = NavStatus::InOrbit;

        let first_hop_actions: Vec<TravelAction> = vec![TravelAction::Navigate {
            from: WaypointSymbol("X1-FOO-BAR".to_string()),
            to: WaypointSymbol("X1-FOO-A1".to_string()),
            distance: 100,
            travel_time: 57,
            fuel_consumption: 200,
            mode: FlightMode::Burn,
            total_time: 57,
        }];

        mock_test_blackboard
            .expect_compute_path()
            .with(
                eq(WaypointSymbol("X1-FOO-BAR".to_string())),
                eq(WaypointSymbol("X1-FOO-A1".to_string())),
                eq(30),
                eq(500),
                eq(600),
            )
            .returning(move |_, _, _, _, _| Ok(first_hop_actions.clone()));

        mock_client.expect_navigate().with(eq(ShipSymbol("FLWI-1".to_string())), eq(WaypointSymbol("X1-FOO-A1".to_string()))).times(1).returning(
            move |_, _| {
                Ok(NavigateShipResponse {
                    data: NavAndFuelResponse {
                        nav: TestObjects::create_nav(
                            FlightMode::Burn,
                            NavStatus::InTransit,
                            &WaypointSymbol("X1-FOO-BAR".to_string()),
                            &WaypointSymbol("X1-FOO-A1".to_string()),
                        ),
                        fuel: TestObjects::create_fuel(500, 200),
                    },
                })
            },
        );

        mock_client.expect_set_flight_mode().with(eq(ShipSymbol("FLWI-1".to_string())), eq(FlightMode::Burn)).times(1).returning(move |_, _| {
            Ok(PatchShipNavResponse {
                data: TestObjects::create_nav(
                    FlightMode::Burn,
                    NavStatus::InTransit,
                    &WaypointSymbol("X1-FOO-BAR".to_string()),
                    &WaypointSymbol("X1-FOO-BAR".to_string()),
                ),
            })
        });

        let behaviors = ship_behaviors();
        let mut ship_behavior = behaviors.navigate_to_destination;

        ship_behavior.update_indices();

        println!("{}", ship_behavior.to_mermaid());

        let mut ship_ops = ShipOperations::new(ship, Arc::new(mock_client));
        let args = BehaviorArgs {
            blackboard: Arc::new(mock_test_blackboard),
        };

        ship_ops.set_destination(WaypointSymbol("X1-FOO-A1".to_string()));
        let result = ship_behavior.run(&args, &mut ship_ops, Duration::from_millis(1)).await.unwrap();
        assert_eq!(result, Response::Success);
        assert_eq!(ship_ops.nav.waypoint_symbol, WaypointSymbol("X1-FOO-A1".to_string()));
        assert_eq!(ship_ops.nav.status, NavStatus::InOrbit);
        assert_eq!(ship_ops.travel_action_queue.len(), 0);
        assert_eq!(ship_ops.current_navigation_destination, None);
    }

    #[tokio::test]
    #[traced_test]
    async fn test_navigate_to_destination_behavior_with_refuel() {
        // let result = timeout(Duration::from_secs(3), async {
        let mut mock_client = MockStClientTrait::new();

        let mut mock_test_blackboard = MockBlackboardOps::new();

        let mut ship = TestObjects::test_ship(100);
        ship.nav.status = NavStatus::InOrbit;

        let first_hop_actions: Vec<TravelAction> = vec![
            TravelAction::Refuel {
                at: WaypointSymbol("X1-FOO-BAR".to_string()),
                total_time: 2,
            },
            TravelAction::Navigate {
                from: WaypointSymbol("X1-FOO-BAR".to_string()),
                to: WaypointSymbol("X1-FOO-A1".to_string()),
                distance: 100,
                travel_time: 57,
                fuel_consumption: 200,
                mode: FlightMode::Burn,
                total_time: 59,
            },
            TravelAction::Refuel {
                at: WaypointSymbol("X1-FOO-A1".to_string()),
                total_time: 2,
            },
        ];

        let mut seq = mockall::Sequence::new();

        // 1st waypoint: Dock for refueling
        mock_client.expect_dock_ship().with(eq(ShipSymbol("FLWI-1".to_string()))).once().in_sequence(&mut seq).returning(move |_| {
            Ok(DockShipResponse {
                data: TestObjects::create_nav(
                    FlightMode::Drift,
                    NavStatus::Docked,
                    &WaypointSymbol("X1-FOO-BAR".to_string()),
                    &WaypointSymbol("X1-FOO-BAR".to_string()),
                ),
            })
        });

        // 1st waypoint: Orbit after refueling
        mock_client.expect_orbit_ship().with(eq(ShipSymbol("FLWI-1".to_string()))).once().in_sequence(&mut seq).return_once(move |_| {
            Ok(OrbitShipResponse {
                data: TestObjects::create_nav(
                    FlightMode::Drift,
                    NavStatus::InOrbit,
                    &WaypointSymbol("X1-FOO-BAR".to_string()),
                    &WaypointSymbol("X1-FOO-BAR".to_string()),
                ),
            })
        });

        // 2nd waypoint: Dock for refueling
        mock_client.expect_dock_ship().with(eq(ShipSymbol("FLWI-1".to_string()))).once().in_sequence(&mut seq).returning(move |_| {
            Ok(DockShipResponse {
                data: TestObjects::create_nav(
                    FlightMode::Burn,
                    NavStatus::Docked,
                    &WaypointSymbol("X1-FOO-A1".to_string()),
                    &WaypointSymbol("X1-FOO-A1".to_string()),
                ),
            })
        });

        // 2nd waypoint: Orbit after refueling
        mock_client.expect_orbit_ship().with(eq(ShipSymbol("FLWI-1".to_string()))).once().in_sequence(&mut seq).return_once(move |_| {
            Ok(OrbitShipResponse {
                data: TestObjects::create_nav(
                    FlightMode::Burn,
                    NavStatus::InOrbit,
                    &WaypointSymbol("X1-FOO-A1".to_string()),
                    &WaypointSymbol("X1-FOO-A1".to_string()),
                ),
            })
        });

        mock_client.expect_refuel().with(eq(ShipSymbol("FLWI-1".to_string())), eq(500), eq(false)).returning(move |_, _, _| {
            Ok(RefuelShipResponse {
                data: TestObjects::create_refuel_ship_response_body(500),
            })
        });

        mock_client.expect_refuel().with(eq(ShipSymbol("FLWI-1".to_string())), eq(200), eq(false)).returning(move |_, _, _| {
            Ok(RefuelShipResponse {
                data: TestObjects::create_refuel_ship_response_body(200),
            })
        });

        mock_test_blackboard
            .expect_compute_path()
            .with(
                eq(WaypointSymbol("X1-FOO-BAR".to_string())),
                eq(WaypointSymbol("X1-FOO-A1".to_string())),
                eq(30),
                eq(100),
                eq(600),
            )
            .returning(move |_, _, _, _, _| Ok(first_hop_actions.clone()));

        mock_client.expect_navigate().with(eq(ShipSymbol("FLWI-1".to_string())), eq(WaypointSymbol("X1-FOO-A1".to_string()))).times(1).returning(
            move |_, _| {
                Ok(NavigateShipResponse {
                    data: NavAndFuelResponse {
                        nav: TestObjects::create_nav(
                            FlightMode::Burn,
                            NavStatus::InTransit,
                            &WaypointSymbol("X1-FOO-BAR".to_string()),
                            &WaypointSymbol("X1-FOO-A1".to_string()),
                        ),
                        fuel: TestObjects::create_fuel(600, 200),
                    },
                })
            },
        );

        mock_client.expect_set_flight_mode().with(eq(ShipSymbol("FLWI-1".to_string())), eq(FlightMode::Burn)).times(1).returning(move |_, _| {
            Ok(PatchShipNavResponse {
                data: TestObjects::create_nav(
                    FlightMode::Burn,
                    NavStatus::InTransit,
                    &WaypointSymbol("X1-FOO-BAR".to_string()),
                    &WaypointSymbol("X1-FOO-BAR".to_string()),
                ),
            })
        });

        let behaviors = ship_behaviors();
        let mut ship_behavior = behaviors.navigate_to_destination;

        ship_behavior.update_indices();

        println!("{}", ship_behavior.to_mermaid());

        let mut ship_ops = ShipOperations::new(ship, Arc::new(mock_client));
        let args = BehaviorArgs {
            blackboard: Arc::new(mock_test_blackboard),
        };

        ship_ops.set_destination(WaypointSymbol("X1-FOO-A1".to_string()));
        let result = ship_behavior.run(&args, &mut ship_ops, Duration::from_millis(1)).await.unwrap();
        assert_eq!(result, Response::Success);
        assert_eq!(ship_ops.nav.waypoint_symbol, WaypointSymbol("X1-FOO-A1".to_string()));
        assert_eq!(ship_ops.nav.status, NavStatus::InOrbit);
        assert_eq!(ship_ops.travel_action_queue.len(), 0);
        assert_eq!(ship_ops.current_navigation_destination, None);
        // })
        // .await;
        //
        // assert!(result.is_ok(), "test-timed out");
    }

     */
}
