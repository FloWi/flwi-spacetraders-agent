use crate::behavior_tree::behavior_args::BehaviorArgs;
use crate::behavior_tree::behavior_tree::Response::Success;
use crate::behavior_tree::behavior_tree::{ActionEvent, Actionable, Response};
use crate::behavior_tree::ship_behaviors::ShipAction;
use crate::ship::ShipOperations;
use crate::st_client::StClientTrait;
use anyhow::Result;
use anyhow::{anyhow, Error};
use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, Utc};
use core::time::Duration;
use itertools::Itertools;
use st_domain::budgeting::credits::Credits;
use st_domain::budgeting::treasury_redesign::FinanceTicketDetails::{PurchaseTradeGoods, RefuelShip, SellTradeGoods};
use st_domain::budgeting::treasury_redesign::{FinanceTicket, FinanceTicketDetails};
use st_domain::cargo_transfer::{InternalTransferCargoRequest, InternalTransferCargoResponse, InternalTransferCargoToHaulerResult, TransferCargoError};
use st_domain::TradeGoodSymbol::MOUNT_GAS_SIPHON_I;
use st_domain::TransactionActionEvent::{PurchasedShip, PurchasedTradeGoods, SoldTradeGoods, SuppliedConstructionSite};
use st_domain::{
    get_exploration_tasks_for_waypoint, ExplorationTask, NavStatus, OperationExpenseEvent, RefuelShipResponse, RefuelShipResponseBody, ShipSymbol, Survey,
    TradeGoodSymbol, TravelAction,
};
use std::collections::HashSet;
use std::hint::unreachable_unchecked;
use std::ops::{Add, Not};
use std::sync::Arc;
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
                        event!(Level::DEBUG, "Waiting for arrival for: {duration:?}");
                        tokio::time::sleep(Duration::from_millis(u64::try_from(duration.num_milliseconds() + 1).unwrap_or(0))).await;
                        Ok(Success)
                    } else {
                        Ok(Success)
                    }
                }
            },
            ShipAction::WaitForCooldown => {
                let now: DateTime<Utc> = Utc::now();
                let cooldown_finished_time: DateTime<Utc> = state.cooldown.expiration.unwrap_or(Utc::now());

                let is_still_cooling_down: bool = now < cooldown_finished_time;
                event!(
                    Level::DEBUG,
                    "ShipAction::WaitForCooldown: Ship is still cooling down. now: {} cooldown_finished_time: {} is_still_cooling_down: {}",
                    now,
                    cooldown_finished_time,
                    is_still_cooling_down
                );

                if is_still_cooling_down {
                    let duration = cooldown_finished_time - now;
                    event!(Level::DEBUG, "Waiting for cooldown for: {duration:?}");
                    tokio::time::sleep(Duration::from_millis(u64::try_from(duration.num_milliseconds() + 1).unwrap_or(0))).await;
                    Ok(Success)
                } else {
                    Ok(Success)
                }
            }

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

                args.treasurer
                    .report_expense(
                        &state.my_fleet,
                        state.current_navigation_destination.clone(),
                        state.maybe_trades.clone().unwrap_or_default(),
                        TradeGoodSymbol::FUEL,
                        response.data.transaction.units as u32,
                        Credits::from(response.data.transaction.price_per_unit),
                    )
                    .await?;

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

            ShipAction::HasUncompletedTrade => match &state.maybe_trades {
                None => Err(anyhow!("No trade assigned")),
                Some(trades) => {
                    if trades.is_empty() {
                        Err(anyhow!("Trades Vector is empty"))
                    } else {
                        Ok(Success)
                    }
                }
            },

            ShipAction::IsAtDestination => {
                if let Some(current) = &state.current_navigation_destination {
                    if &state.nav.waypoint_symbol == current && state.is_stationary() {
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

                if state.symbol.0.ends_with("-1").not() {
                    let foo = 1 + 41; // just for the breakpoint
                }

                let exploration_tasks = if is_uncharted {
                    args.get_exploration_tasks_waypoint(&state.nav.waypoint_symbol)
                        .await?
                } else {
                    exploration_tasks
                };

                for task in exploration_tasks.iter() {
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

                event!(
                    Level::INFO,
                    message = "CollectWaypointInfos",
                    waypoint = state.nav.waypoint_symbol.0.clone(),
                    exploration_tasks_for_this_waypoint = exploration_tasks.iter().map(|t| t.to_string()).join(", "),
                    num_exploration_tasks_left = state.explore_location_queue.len() as i64,
                );

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
            ShipAction::SetNextTradeStopAsDestination => match state.maybe_trades.clone() {
                None => Err(anyhow!("No next trade waypoint found - state.maybe_trade is None")),
                Some(trades) if trades.is_empty() => Err(anyhow!("No next trade waypoint found - state.maybe_trade has empty Vec<FinanceTicket>")),
                Some(trades) => {
                    // we can't execute all trades immediately. (e.g. can't sell _before_ you purchased the goods)

                    let executable_trades = trades
                        .iter()
                        .filter(|t| match t.details.clone() {
                            SellTradeGoods(details) => match details.maybe_matching_purchase_ticket {
                                None => true,
                                Some(related_purchase_ticket) => !trades
                                    .iter()
                                    .any(|t| t.ticket_id == related_purchase_ticket),
                            },
                            FinanceTicketDetails::SupplyConstructionSite(details) => match details.maybe_matching_purchase_ticket {
                                None => true,
                                Some(related_purchase_ticket) => !trades
                                    .iter()
                                    .any(|t| t.ticket_id == related_purchase_ticket),
                            },
                            PurchaseTradeGoods(_) => true,
                            FinanceTicketDetails::PurchaseShip(_) => true,
                            RefuelShip(_) => true,
                        })
                        .collect_vec();

                    let waypoints = executable_trades
                        .into_iter()
                        .map(|t| t.details.get_waypoint())
                        .collect_vec();

                    let maybe_closest_waypoint = args
                        .get_closest_waypoint(&state.nav.waypoint_symbol, &waypoints)
                        .await?;
                    if let Some(closest_waypoint) = maybe_closest_waypoint {
                        state.current_navigation_destination = Some(closest_waypoint);
                        Ok(Success)
                    } else {
                        Err(anyhow!("Unable to set navigation destination. maybe_closest_waypoint is None"))
                    }
                }
            },

            ShipAction::PerformTradeActionAndMarkAsCompleted => {
                if state.nav.status != NavStatus::Docked {
                    println!("Hello, breakpoint. Ship should be docked by now");
                }
                if let Some(finance_tickets) = &state.maybe_trades.clone() {
                    let mut completed_tickets: HashSet<FinanceTicket> = HashSet::new();

                    let current_location = &state.nav.waypoint_symbol.clone();
                    for finance_ticket in finance_tickets
                        .iter()
                        .filter(|t| &t.details.get_waypoint() == current_location)
                    {
                        match &finance_ticket.details {
                            PurchaseTradeGoods(details) => {
                                let response = state
                                    .purchase_trade_good(details.quantity, details.trade_good.clone())
                                    .await?;

                                args.mark_purchase_as_completed(finance_ticket.clone(), &response)
                                    .await?;

                                action_completed_tx
                                    .send(ActionEvent::TransactionCompleted(
                                        state.clone(),
                                        PurchasedTradeGoods {
                                            ticket_id: finance_ticket.ticket_id,
                                            ticket_details: details.clone(),
                                            response,
                                        },
                                        finance_ticket.clone(),
                                    ))
                                    .await?;
                            }
                            SellTradeGoods(details) => {
                                let response = state
                                    .sell_trade_good(details.quantity, details.trade_good.clone())
                                    .await?;

                                args.mark_sell_as_completed(finance_ticket.clone(), &response)
                                    .await?;

                                action_completed_tx
                                    .send(ActionEvent::TransactionCompleted(
                                        state.clone(),
                                        SoldTradeGoods {
                                            ticket_id: finance_ticket.ticket_id,
                                            ticket_details: details.clone(),
                                            response,
                                        },
                                        finance_ticket.clone(),
                                    ))
                                    .await?;
                            }
                            FinanceTicketDetails::PurchaseShip(details) => {
                                let response = state
                                    .purchase_ship(&details.ship_type, &details.waypoint_symbol)
                                    .await?;

                                args.mark_ship_purchase_as_completed(finance_ticket.clone(), &response)
                                    .await?;

                                action_completed_tx
                                    .send(ActionEvent::TransactionCompleted(
                                        state.clone(),
                                        PurchasedShip {
                                            ticket_id: finance_ticket.ticket_id,
                                            ticket_details: details.clone(),
                                            response,
                                        },
                                        finance_ticket.clone(),
                                    ))
                                    .await?;
                            }
                            RefuelShip(_details) => {}
                            FinanceTicketDetails::SupplyConstructionSite(details) => {
                                let response = state
                                    .supply_construction_site(details.quantity, &details.trade_good, &details.waypoint_symbol)
                                    .await?;

                                args.mark_construction_delivery_as_completed(finance_ticket.clone(), &response)
                                    .await?;
                                args.blackboard
                                    .update_construction_site(&response.data.construction)
                                    .await?;

                                action_completed_tx
                                    .send(ActionEvent::TransactionCompleted(
                                        state.clone(),
                                        SuppliedConstructionSite {
                                            ticket_id: finance_ticket.ticket_id,
                                            ticket_details: details.clone(),
                                            response,
                                        },
                                        finance_ticket.clone(),
                                    ))
                                    .await?;
                            }
                        }
                        completed_tickets.insert(finance_ticket.clone());
                        let still_open_tickets = finance_tickets
                            .iter()
                            .filter(|t| completed_tickets.contains(t).not())
                            .cloned()
                            .collect_vec();
                        state.set_trade_tickets(still_open_tickets)
                    }

                    Ok(Success)
                } else {
                    Ok(Success)
                }
            }

            ShipAction::HasNextTradeWaypoint => match state.maybe_trades.clone() {
                //FIXME: allow setting multiple trading stops (e.g. 1st purchase, 2nd sell)
                None => Err(anyhow!("No next trade waypoint found - state.maybe_trade is None")),
                Some(_) => Ok(Success),
            },
            ShipAction::SleepUntilNextObservationTimeOrShipPurchaseTicketHasBeenAssigned => match state.maybe_next_observation_time {
                None => Ok(Success),
                Some(next_time) => loop {
                    if state.maybe_trades.is_some() || Utc::now() > next_time {
                        break Ok(Success);
                    } else {
                        tokio::time::sleep(sleep_duration).await;
                    }
                },
            },
            ShipAction::HasShipPurchaseTicketForWaypoint => {
                let current_location = state.current_location();

                if let Some(trades) = state.maybe_trades.clone() {
                    let has_ship_ticket_at_current_waypoint = trades.iter().any(|trade| match &trade.details {
                        PurchaseTradeGoods(_) => false,
                        SellTradeGoods(_) => false,
                        RefuelShip(_) => false,
                        FinanceTicketDetails::SupplyConstructionSite(_) => false,
                        FinanceTicketDetails::PurchaseShip(details) => {
                            let shipyard_wp = details.waypoint_symbol.clone();
                            shipyard_wp == current_location
                        }
                    });
                    if has_ship_ticket_at_current_waypoint {
                        Ok(Success)
                    } else {
                        Err(anyhow!("No matching ship purchase ticket found"))
                    }
                } else {
                    Err(anyhow!("No trading ticket"))
                }
            }
            ShipAction::RegisterProbeForPermanentObservation => {
                // we don't need to send a specialized message
                Ok(Success)
            }
            ShipAction::CheckForShipPurchaseTicket => {
                if state.has_trade().not() {
                    let tickets = args.treasurer.get_active_tickets().await?;
                    if let Some((_, ship_purchase_ticket)) = tickets.into_iter().find(|(_, t)| match t.details {
                        PurchaseTradeGoods(_) => false,
                        SellTradeGoods(_) => false,
                        FinanceTicketDetails::SupplyConstructionSite(_) => false,
                        RefuelShip(_) => false,
                        FinanceTicketDetails::PurchaseShip(_) => t.ship_symbol == state.symbol,
                    }) {
                        state.set_trade_tickets(vec![ship_purchase_ticket]);
                    }
                }

                Ok(Success)
            }
            ShipAction::SiphonResources => {
                state.siphon_resources().await?;

                Ok(Success)
            }
            ShipAction::JettisonInvaluableCarboHydrates => {
                if let Some(cfg) = state.maybe_siphoning_config.clone() {
                    let _responses = state
                        .jettison_everything_not_on_list(cfg.demanded_goods)
                        .await?;
                }

                Ok(Success)
            }
            ShipAction::IsAtSiphoningSite => {
                if let Some(cfg) = state.maybe_siphoning_config.clone() {
                    if state.current_location() == cfg.siphoning_waypoint && state.nav.status == NavStatus::InOrbit {
                        Ok(Success)
                    } else {
                        Err(anyhow!("No siphoning config found"))
                    }
                } else {
                    Err(anyhow!("No siphoning config found"))
                }
            }
            ShipAction::SetSiphoningSiteAsDestination => {
                if let Some(cfg) = state.maybe_siphoning_config.clone() {
                    state.set_destination(cfg.siphoning_waypoint);
                    Ok(Success)
                } else {
                    Err(anyhow!("No siphoning config found"))
                }
            }
            ShipAction::HasCargoSpaceForSiphoning => {
                let cargo_space_left = (state.cargo.capacity - state.cargo.units) as u32;
                let yield_size = state.get_yield_size_for_siphoning();

                if yield_size < cargo_space_left {
                    Ok(Success)
                } else {
                    Err(anyhow!(
                        "Cargo space too small for siphoning. Yield size: {}, cargo space left: {}",
                        yield_size,
                        cargo_space_left
                    ))
                }
            }
            ShipAction::CreateSellTicketsForAllCargoItems => {
                if let Some(delivery_locations) = state.get_delivery_locations() {
                    let (cargo_items_with_delivery_location, cargo_items_without_delivery_location): (Vec<_>, Vec<_>) =
                        state.cargo.inventory.iter().partition_map(|inv| {
                            if let Some(delivery_location) = delivery_locations.get(&inv.symbol) {
                                itertools::Either::Left((inv.clone(), delivery_location.clone()))
                            } else {
                                itertools::Either::Right(inv.clone())
                            }
                        });

                    if cargo_items_without_delivery_location.len() > 0 {
                        let items = cargo_items_without_delivery_location
                            .iter()
                            .map(|inv| inv.symbol.clone().to_string())
                            .join(", ");
                        Err(anyhow!(
                            "No delivery location found for {} of the {} cargo items: {}",
                            cargo_items_without_delivery_location.len(),
                            state.cargo.inventory.len(),
                            items
                        ))
                    } else {
                        let mut sell_tickets = vec![];
                        for (item, delivery_route) in cargo_items_with_delivery_location.into_iter() {
                            let delivery_location = delivery_route.delivery_location.clone();
                            let batches = calc_batches_based_on_trade_volume(item.units, delivery_route.delivery_market_entry.trade_volume as u32);
                            for batch in batches {
                                let ticket = args
                                    .treasurer
                                    .create_sell_trade_goods_ticket(
                                        &state.my_fleet,
                                        item.symbol.clone(),
                                        delivery_location.clone(),
                                        state.symbol.clone(),
                                        batch,
                                        Credits::default(),
                                        None,
                                    )
                                    .await?;
                                sell_tickets.push(ticket);
                            }
                        }

                        state.set_trade_tickets(sell_tickets);
                        Ok(Success)
                    }
                } else {
                    Err(anyhow!("No delivery_locations found"))
                }
            }
            ShipAction::HasCargoSpaceForMining => {
                let cargo_space_left = (state.cargo.capacity - state.cargo.units) as u32;
                let yield_size = state.get_yield_size_for_mining();

                if yield_size < cargo_space_left {
                    Ok(Success)
                } else {
                    Err(anyhow!(
                        "Cargo space too small for mining. Yield size: {}, cargo space left: {}",
                        yield_size,
                        cargo_space_left
                    ))
                }
            }
            ShipAction::JettisonInvaluableMinerals => {
                if let Some(cfg) = state.maybe_mining_ops_config.clone() {
                    let _responses = state
                        .jettison_everything_not_on_list(cfg.demanded_goods)
                        .await?;
                }

                Ok(Success)
            }
            ShipAction::ExtractResources => {
                if let Some(cfg) = state.maybe_mining_ops_config.clone() {
                    loop {
                        let maybe_survey: Option<Survey> = args
                            .blackboard
                            .get_best_survey_for_current_demand(&cfg)
                            .await?;

                        match state.extract_resources(maybe_survey.clone()).await {
                            Ok(result) => {
                                state.cargo = result.data.cargo.clone();
                                state.cooldown = result.data.cooldown.clone();
                                break Ok(Success);
                            }
                            Err(e) => {
                                if e.to_string().contains("ShipSurveyExhaustedError") {
                                    if let Some(survey) = maybe_survey {
                                        args.blackboard.mark_survey_as_exhausted(&survey).await?;
                                    }
                                } else {
                                    break Err(e);
                                }
                            }
                        }
                    }
                } else {
                    Err(anyhow!("No mining config found"))
                }
            }
            ShipAction::Survey => {
                let survey_response = state.perform_survey().await;
                match survey_response {
                    Ok(survey_response) => {
                        args.blackboard
                            .save_survey_response(survey_response)
                            .await?;

                        Ok(Success)
                    }

                    Err(e) => Err(anyhow!(e)),
                }
            }
            ShipAction::IsSurveyCapable => {
                if state.is_surveyor() {
                    Ok(Success)
                } else {
                    Err(anyhow!("Ship is not a surveyor"))
                }
            }
            ShipAction::IsSurveyNecessary => {
                if args
                    .blackboard
                    .is_survey_necessary(state.get_mining_site())
                    .await?
                {
                    Ok(Success)
                } else {
                    Err(anyhow!("No survey needed"))
                }
            }
            ShipAction::SetMiningSiteAsDestination => {
                if let Some(mining_site_wps) = state.get_mining_site() {
                    state.set_destination(mining_site_wps);
                    Ok(Success)
                } else {
                    Err(anyhow!("Mining site not found"))
                }
            }
            ShipAction::IsAtMiningSite => {
                if state.is_at_mining_waypoint() {
                    Ok(Success)
                } else {
                    Err(anyhow!("Not at mining site yet"))
                }
            }
            ShipAction::AttemptCargoTransfer => {
                let cargo_transfer_result = args
                    .transfer_cargo_manager
                    .try_to_transfer_cargo_until_available_space(state.symbol.clone(), state.nav.waypoint_symbol.clone(), state.cargo.clone(), |args| {
                        wrap_transfer_cargo_request(Arc::clone(&state.client), args)
                    })
                    .await;

                match cargo_transfer_result {
                    Ok(res) => match res {
                        InternalTransferCargoToHaulerResult::NoMatchingShipFound => {}
                        InternalTransferCargoToHaulerResult::Success {
                            updated_miner_cargo,
                            transfer_tasks,
                        } => {
                            state.cargo = updated_miner_cargo;
                            println!(
                                "Ship {} transferred cargo to haulers in {} transfers: {:?}",
                                state.symbol,
                                transfer_tasks.len(),
                                transfer_tasks
                            );
                        }
                    },
                    Err(err) => {
                        eprintln!("err: {err:?}");
                    }
                }

                Ok(Success)
            }
            ShipAction::AnnounceHaulerReadyForPickup => {
                let hauler_wait_result = args
                    .transfer_cargo_manager
                    .register_hauler_for_pickup_and_wait_until_full(state.nav.waypoint_symbol.clone(), state.symbol.clone(), state.cargo.clone())
                    .await;

                match hauler_wait_result {
                    Ok(res) => {
                        state.cargo = res.cargo.clone();
                        event!(
                            Level::INFO,
                            message = "Hauler successfully received cargo",
                            num_transfers = res.transfers.len(),
                            total_wait_time = format!("{}s", res.total_wait_time().num_seconds())
                        );

                        Ok(Success)
                    }
                    Err(err) => Err(err),
                }
            }
            ShipAction::IsHaulerFilledEnoughForDelivery => {
                let fill_ratio = state.cargo.units as f64 / state.cargo.capacity as f64;
                if fill_ratio > 0.8 {
                    Ok(Success)
                } else {
                    Err(anyhow!(
                        "Ship cargo is {} out of {} --> {}% <= 80%",
                        state.cargo.units,
                        state.cargo.capacity,
                        (fill_ratio * 100.0).round() as u32
                    ))
                }
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
                anyhow::bail!("Action failed {}", err)
            }
        };

        result
    }
}

fn calc_batches_based_on_trade_volume(number_items: u32, trade_volume: u32) -> Vec<u32> {
    let result = if number_items <= trade_volume {
        vec![number_items]
    } else {
        let mut batches = vec![];
        let num_batches = number_items / trade_volume;
        for i in 1..=num_batches {
            if i < num_batches {
                batches.push(trade_volume);
            } else {
                let rest = number_items - batches.iter().sum::<u32>();
                batches.push(rest);
            }
        }
        batches
    };

    let total = result.iter().sum::<u32>();
    assert_eq!(total, number_items);

    result
}

async fn wrap_transfer_cargo_request(
    client: Arc<dyn StClientTrait>,
    internal_args: InternalTransferCargoRequest,
) -> Result<InternalTransferCargoResponse, TransferCargoError> {
    let result = client
        .transfer_cargo(
            internal_args.sending_ship.clone(),
            internal_args.receiving_ship.clone(),
            internal_args.trade_good_symbol.clone(),
            internal_args.units,
        )
        .await;
    match result {
        Ok(server_response) => Ok(InternalTransferCargoResponse {
            receiving_ship: internal_args.receiving_ship,
            trade_good_symbol: internal_args.trade_good_symbol.clone(),
            units: internal_args.units,
            sending_ship_cargo: server_response.data.cargo,
            receiving_ship_cargo: server_response.data.target_cargo,
        }),
        Err(err) => Err(TransferCargoError::ServerError(err)),
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
        DockShipResponse, FleetId, FlightMode, GetMarketResponse, NavAndFuelResponse, NavStatus, NavigateShipResponse, SetFlightModeResponse, ShipSymbol,
        TravelAction, WaypointSymbol, WaypointTraitSymbol,
    };

    use crate::behavior_tree::behavior_tree::Response::Success;
    use crate::fleet::ship_runner::ship_behavior_runner;
    use crate::st_client::MockStClientTrait;
    use anyhow::anyhow;
    use itertools::Itertools;
    use std::sync::Arc;
    use tokio::sync::mpsc::{Receiver, Sender};

    use crate::test_objects::TestObjects;
    use crate::transfer_cargo_manager::TransferCargoManager;
    use st_domain::blackboard_ops::MockBlackboardOps;
    use st_domain::budgeting::test_sync_ledger::create_test_ledger_setup;
    use st_domain::budgeting::treasury_redesign::ThreadSafeTreasurer;
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

        let mut ship_ops = ShipOperations::new(ship, Arc::new(mock_client), FleetId(42));
        let result = ship_ops.dock().await;
        assert!(result.is_ok());
    }

    #[test(tokio::test)]
    async fn test_dock_if_necessary_behavior_on_docked_ship() {
        let mut mock_client = MockStClientTrait::new();
        let (test_archiver, task_sender) = create_test_ledger_setup().await;

        let args = BehaviorArgs {
            blackboard: Arc::new(MockBlackboardOps::new()),
            treasurer: ThreadSafeTreasurer::new(0.into(), task_sender.clone()).await,
            transfer_cargo_manager: Arc::new(TransferCargoManager::new()),
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

        let mut ship_ops = ShipOperations::new(ship, Arc::new(mock_client), FleetId(42));
        let (result, ship_states, action_events) = test_run_ship_behavior(&mut ship_ops, Duration::from_millis(1), args, ship_behavior)
            .await
            .unwrap();

        assert_eq!(result, Success);
    }

    #[test(tokio::test)]
    async fn test_dock_if_necessary_behavior_on_orbiting_ship() {
        let mut mock_client = MockStClientTrait::new();
        let (test_archiver, task_sender) = create_test_ledger_setup().await;

        let args = BehaviorArgs {
            blackboard: Arc::new(MockBlackboardOps::new()),
            treasurer: ThreadSafeTreasurer::new(0.into(), task_sender.clone()).await,
            transfer_cargo_manager: Arc::new(TransferCargoManager::new()),
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

        let mut ship_ops = ShipOperations::new(ship, Arc::new(mock_client), FleetId(42));

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
            .with(always())
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
        let (test_archiver, task_sender) = create_test_ledger_setup().await;

        let mut ship_ops = ShipOperations::new(ship, Arc::new(mock_client), FleetId(42));
        let args = BehaviorArgs {
            blackboard: Arc::new(mock_test_blackboard),
            treasurer: ThreadSafeTreasurer::new(0.into(), task_sender.clone()).await,
            transfer_cargo_manager: Arc::new(TransferCargoManager::new()),
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

        let mut ship_ops = ShipOperations::new(ship, Arc::new(mock_client), FleetId(42));
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

        let mut ship_ops = ShipOperations::new(ship, Arc::new(mock_client), FleetId(42));
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
