use crate::behavior_tree::behavior_args::{BehaviorArgs, BlackboardOps};
use crate::behavior_tree::behavior_tree::Response::Success;
use crate::behavior_tree::behavior_tree::{ActionEvent, Actionable, Response};
use crate::behavior_tree::ship_behaviors::ShipAction;
use crate::exploration::exploration::ExplorationTask;
use crate::pathfinder::pathfinder::TravelAction;
use crate::ship::ShipOperations;
use anyhow::anyhow;
use async_trait::async_trait;
use chrono::{DateTime, Local, TimeDelta, Utc};
use core::time::Duration;
use itertools::Itertools;
use st_domain::TransactionActionEvent::{PurchasedTradeGoods, ShipPurchased, SoldTradeGoods, SuppliedConstructionSite};
use st_domain::{
    Agent, AgentSymbol, Cargo, Cooldown, Crew, Engine, FlightMode, Frame, Fuel, FuelConsumed, MarketData, Nav, NavRouteWaypoint, NavStatus, Reactor,
    RefuelShipResponse, RefuelShipResponseBody, Registration, Requirements, Route, Ship, ShipFrameSymbol, ShipRegistrationRole, ShipSymbol, TradeGoodSymbol,
    TradeTicket, Transaction, TransactionType, Waypoint, WaypointSymbol, WaypointType,
};
use std::ops::{Add, Not};
use tokio::sync::mpsc::Sender;

#[async_trait]
impl Actionable for ShipAction {
    type ActionError = anyhow::Error;
    type ActionArgs = BehaviorArgs;
    type ActionState = ShipOperations;

    async fn run(
        &self,
        args: &Self::ActionArgs,
        state: &mut Self::ActionState,
        _: Duration,
        state_changed_tx: &Sender<Self::ActionState>,
        action_completed_tx: &Sender<ActionEvent>,
    ) -> Result<Response, Self::ActionError> {
        let result = match self {
            ShipAction::HasTravelActionEntry => {
                let no_action_left = state.travel_action_queue.is_empty();
                if no_action_left {
                    Err(anyhow!("No action left"))
                } else {
                    Ok(Response::Success)
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
                    println!("ShipAction::WaitForArrival: Ship is {:?}", state.nav.status);
                    Ok(Success)
                }
                NavStatus::InTransit => {
                    let now: DateTime<Utc> = Utc::now();
                    let arrival_time: DateTime<Utc> = state.nav.route.arrival;
                    let is_still_travelling: bool = now < arrival_time;
                    println!(
                        "ShipAction::WaitForArrival: Ship is InTransit. now: {} arrival_time: {} is_still_travelling: {}",
                        now, arrival_time, is_still_travelling
                    );

                    if is_still_travelling {
                        Ok(Response::Running)
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
                if let Some(action) = &state.current_travel_action() {
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
            ShipAction::MarkTravelActionAsCompleteIfPossible => match &state.current_travel_action() {
                None => Ok(Success),
                Some(action) => {
                    let is_done = match action {
                        TravelAction::Navigate { to, .. } => state.nav.waypoint_symbol == *to && state.nav.status != NavStatus::InTransit,
                        TravelAction::Refuel { at, .. } => {
                            state.nav.waypoint_symbol == *at && state.nav.status != NavStatus::InTransit && state.fuel.current == state.fuel.capacity
                        }
                    };

                    if is_done {
                        state.pop_travel_action();
                    }
                    Ok(Success)
                }
            },
            ShipAction::CanSkipRefueling => match &state.current_travel_action() {
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
                if let Some(action) = &state.current_travel_action() {
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
                if let Some(action) = &state.current_travel_action() {
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
                println!("travel_action queue: {:?}", state.travel_action_queue);
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
                println!("explore_location_queue: {:?}", state.explore_location_queue);
                Ok(Success)
            }
            ShipAction::PrintDestination => {
                println!("current_navigation_destination: {:?}", state.current_navigation_destination);
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
                if let Some((current_destination, last_travel_action)) = state.current_navigation_destination.clone().zip(state.last_travel_action()) {
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

                println!("successfully computed route from {:?} to {:?}: {:?}", from, to, &path);
                state.set_route(path);
                Ok(Success)
            }
            ShipAction::CollectWaypointInfos => {
                let exploration_tasks = args
                    .get_exploration_tasks_for_current_waypoint(state.nav.waypoint_symbol.clone())
                    .await
                    .map_err(|_| anyhow!("inserting waypoint failed"))?;

                let is_uncharted = exploration_tasks.contains(&ExplorationTask::CreateChart);
                if is_uncharted {
                    let charted_waypoint = state.chart_waypoint().await?;
                    args.insert_waypoint(&charted_waypoint.waypoint).await.map_err(|_| anyhow!("inserting waypoint failed"))?;
                }

                let exploration_tasks = if is_uncharted {
                    args.get_exploration_tasks_for_current_waypoint(state.nav.waypoint_symbol.clone()).await?
                } else {
                    exploration_tasks
                };

                println!("CollectWaypointInfos - exploration_tasks: {:?}", exploration_tasks);

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
                        Ok(Response::Running)
                    }
                }
            },
            ShipAction::SetNextTradeStopAsDestination => match state.maybe_trade.clone() {
                None => Err(anyhow!("No next trade waypoint found - state.maybe_trade is None")),
                Some(trade) => match trade {
                    TradeTicket::TradeCargo {
                        purchase_completion_status,
                        sale_completion_status,
                        ..
                    } => {
                        let candidates = purchase_completion_status
                            .iter()
                            .filter_map(|(ticket, is_completed)| is_completed.not().then_some(ticket.waypoint_symbol.clone()))
                            .chain(sale_completion_status.iter().filter_map(|(ticket, is_completed)| {
                                let is_in_cargo =
                                    state.cargo.inventory.iter().any(|inventory| inventory.symbol == ticket.trade_good && inventory.units >= ticket.quantity);
                                (is_completed.not() && is_in_cargo).then_some(ticket.waypoint_symbol.clone())
                            }))
                            .unique()
                            .collect_vec();
                        let maybe_best_wps: Option<WaypointSymbol> = args.get_closest_waypoint(&state.nav.waypoint_symbol, &candidates).await?;
                        match maybe_best_wps {
                            None => Err(anyhow!("No next trade waypoint found - maybe_best_waypoint is None")),
                            Some(best_wps) => {
                                println!(
                                    r#"ShipAction::SetNextTradeStopAsDestination:
                                purchase_completion_status: {}
                                sale_completion_status: {}
                                candidates: {}
                                maybe_best_wps: {}
                                "#,
                                    serde_json::to_string(&purchase_completion_status)?,
                                    serde_json::to_string(&sale_completion_status)?,
                                    serde_json::to_string(&candidates)?,
                                    best_wps.0,
                                );
                                state.set_destination(best_wps);
                                Ok(Success)
                            }
                        }
                    }
                    TradeTicket::DeliverConstructionMaterials {
                        ticket_id,
                        purchase_completion_status,
                        delivery_status,
                    } => {
                        let candidates = purchase_completion_status
                            .iter()
                            .filter_map(|(ticket, is_completed)| is_completed.not().then_some(ticket.waypoint_symbol.clone()))
                            .collect_vec();

                        let maybe_best_wps: Option<WaypointSymbol> = args.get_closest_waypoint(&state.nav.waypoint_symbol, &candidates).await?;
                        match maybe_best_wps {
                            None => Err(anyhow!("No next trade waypoint found - maybe_best_waypoint is None")),
                            Some(best_wps) => {
                                state.set_destination(best_wps);
                                Ok(Success)
                            }
                        }
                    }
                    TradeTicket::PurchaseShipTicket { ticket_id, details } => {
                        state.set_destination(details.waypoint_symbol);
                        Ok(Success)
                    }
                },
            },
            ShipAction::PerformTradeActionAndMarkAsCompleted => {
                if let Some(trade) = &state.maybe_trade.clone() {
                    match trade {
                        TradeTicket::TradeCargo {
                            ticket_id,
                            purchase_completion_status,
                            sale_completion_status,
                            ..
                        } => {
                            let current_location = state.current_location();

                            let purchases = purchase_completion_status
                                .iter()
                                .filter(|(ticket, is_completed)| is_completed.not() && ticket.waypoint_symbol == current_location);

                            let sales =
                                sale_completion_status.iter().filter(|(ticket, is_completed)| is_completed.not() && ticket.waypoint_symbol == current_location);

                            for (purchase, _) in purchases {
                                let result = state.purchase_trade_good(purchase).await?;
                                state.mark_transaction_as_complete(&purchase.id);
                                action_completed_tx
                                    .send(ActionEvent::TransactionCompleted(
                                        state.clone(),
                                        PurchasedTradeGoods(purchase.clone(), result),
                                        state.maybe_trade.clone().unwrap(),
                                    ))
                                    .await?;
                            }

                            for (sale, _) in sales {
                                let result = state.sell_trade_good(sale).await?;
                                state.mark_transaction_as_complete(&sale.id);
                                action_completed_tx
                                    .send(ActionEvent::TransactionCompleted(
                                        state.clone(),
                                        SoldTradeGoods(sale.clone(), result),
                                        state.maybe_trade.clone().unwrap(),
                                    ))
                                    .await?;
                            }
                        }
                        TradeTicket::DeliverConstructionMaterials {
                            ticket_id,
                            purchase_completion_status,
                            delivery_status: delivery_completion_status,
                        } => {
                            let current_location = state.current_location();

                            let purchases = purchase_completion_status
                                .iter()
                                .filter(|(ticket, is_completed)| is_completed.not() && ticket.waypoint_symbol == current_location);

                            let deliveries = delivery_completion_status
                                .iter()
                                .filter(|(ticket, is_completed)| is_completed.not() && ticket.construction_site_waypoint_symbol == current_location);

                            for (purchase, _) in purchases {
                                let result = state.purchase_trade_good(purchase).await?;
                                state.mark_transaction_as_complete(&purchase.id);
                                action_completed_tx
                                    .send(ActionEvent::TransactionCompleted(
                                        state.clone(),
                                        PurchasedTradeGoods(purchase.clone(), result),
                                        state.maybe_trade.clone().unwrap(),
                                    ))
                                    .await?;
                                // args.report_purchase(ticket_id, &purchase.id, &result).await?;
                            }

                            for (delivery, _) in deliveries {
                                let result = state.supply_construction_site(delivery).await?;
                                state.mark_transaction_as_complete(&delivery.id);
                                action_completed_tx
                                    .send(ActionEvent::TransactionCompleted(
                                        state.clone(),
                                        SuppliedConstructionSite(delivery.clone(), result),
                                        state.maybe_trade.clone().unwrap(),
                                    ))
                                    .await?;
                            }
                        }
                        TradeTicket::PurchaseShipTicket { ticket_id, details } => {
                            let result = state.purchase_ship(details).await?;
                            state.mark_transaction_as_complete(&details.id);
                            action_completed_tx
                                .send(ActionEvent::TransactionCompleted(
                                    state.clone(),
                                    ShipPurchased(details.clone(), result),
                                    state.maybe_trade.clone().unwrap(),
                                ))
                                .await?;
                        }
                    }
                    Ok(Success)
                } else {
                    Ok(Success)
                }
            }

            ShipAction::HasNextTradeWaypoint => {
                match state.maybe_trade.clone() {
                    None => Err(anyhow!("No next trade waypoint found - state.maybe_trade is None")),
                    Some(trade) => {
                        match trade {
                            TradeTicket::TradeCargo {
                                purchase_completion_status,
                                sale_completion_status,
                                ..
                            } => {
                                if purchase_completion_status.iter().any(|(_, is_complete)| !is_complete)
                                    || sale_completion_status.iter().any(|(_, is_complete)| !is_complete)
                                {
                                    Ok(Success)
                                } else {
                                    Err(anyhow!("No next trade waypoint found - All transactions marked as completed"))
                                }
                            }
                            TradeTicket::DeliverConstructionMaterials {
                                purchase_completion_status, ..
                            } => {
                                if purchase_completion_status.iter().any(|(_, is_complete)| !is_complete) {
                                    Ok(Success)
                                } else {
                                    Err(anyhow!("No next trade waypoint found - All transactions marked as completed"))
                                }
                            }
                            TradeTicket::PurchaseShipTicket { .. } => {
                                // one-off action
                                Ok(Success)
                            }
                        }
                    }
                }
            }
        };

        let capacity = action_completed_tx.capacity();
        println!("Sending ActionEvent::ShipActionCompleted to action_completed_tx - capacity: {capacity}");

        match result {
            Ok(_res) => {
                action_completed_tx.send(ActionEvent::ShipActionCompleted(Ok((state.clone(), self.clone())))).await?;
            }
            Err(err) => {
                action_completed_tx.send(anyhow::bail!("Action failed {}", err)).await?;
            }
        };

        result
    }
}

struct TestObjects;

impl TestObjects {
    pub fn create_waypoint(waypoint_symbol: &WaypointSymbol, x: i64, y: i64) -> Waypoint {
        Waypoint {
            symbol: waypoint_symbol.clone(),
            r#type: WaypointType::PLANET,
            system_symbol: waypoint_symbol.system_symbol(),
            x,
            y,
            orbitals: vec![],
            orbits: None,
            faction: None,
            traits: vec![],
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
                headquarters: WaypointSymbol("".to_string()),
                credits: 42,
                starting_faction: "".to_string(),
                ship_count: 2,
            },
            fuel: Self::create_fuel(600, 0),
            transaction: Transaction {
                waypoint_symbol: WaypointSymbol("".to_string()),
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

    pub fn create_nav(mode: FlightMode, nav_status: NavStatus, origin_waypoint_symbol: &WaypointSymbol, destination_waypoint_symbol: &WaypointSymbol) -> Nav {
        Nav {
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
            nav: Self::create_nav(
                FlightMode::Drift,
                NavStatus::InTransit,
                &WaypointSymbol("X1-FOO-BAR".to_string()),
                &WaypointSymbol("X1-FOO-BAR".to_string()),
            ),
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
    use crate::behavior_tree::actions::TestObjects;
    use crate::behavior_tree::behavior_args::{BehaviorArgs, BlackboardOps, ExplorationTask};
    use crate::behavior_tree::behavior_tree::Actionable;
    use crate::behavior_tree::behavior_tree::Response;
    use crate::behavior_tree::ship_behaviors::ship_behaviors;
    use crate::pagination::{PaginatedResponse, PaginationInput};
    use crate::pathfinder::pathfinder::TravelAction;
    use crate::ship::ShipOperations;
    use async_trait::async_trait;

    use core::time::Duration;
    use mockall::mock;
    use mockall::predicate::*;

    use st_domain::{
        AgentResponse, AgentSymbol, CreateChartResponse, Data, DockShipResponse, FlightMode, GetConstructionResponse, GetJumpGateResponse, GetMarketResponse,
        GetShipyardResponse, JumpGate, ListAgentsResponse, MarketData, NavAndFuelResponse, NavOnlyResponse, NavStatus, NavigateShipResponse, OrbitShipResponse,
        PatchShipNavResponse, RefuelShipResponse, RegistrationRequest, RegistrationResponse, Ship, ShipSymbol, Shipyard, StStatusResponse, SystemSymbol,
        SystemsPageData, Waypoint, WaypointSymbol,
    };

    use crate::st_client::StClientTrait;
    use std::sync::Arc;
    use tracing_test::traced_test;

    mock! {
        #[derive(Debug)]
        pub TestBlackboard {}

        #[async_trait]
        impl BlackboardOps for TestBlackboard {
            async fn compute_path(&self, from: WaypointSymbol, to: WaypointSymbol, engine_speed: u32, current_fuel: u32, fuel_capacity: u32) -> anyhow::Result<Vec<TravelAction>> {}

            async fn get_exploration_tasks_for_current_waypoint(&self, current_location: WaypointSymbol) -> anyhow::Result<Vec<ExplorationTask>> {}

            async fn insert_waypoint(&self, waypoint: &Waypoint) -> anyhow::Result<()> {}

            async fn insert_market(&self, market_data: MarketData) -> anyhow::Result<()> {}

            async fn insert_jump_gate(&self, jump_gate: JumpGate) -> anyhow::Result<()> {}

            async fn insert_shipyard(&self, shipyard: Shipyard) -> anyhow::Result<()> {}
        }
    }

    mock! {
        #[derive(Debug)]
        pub StClient {}

        #[async_trait]
        impl StClientTrait for StClient {
            async fn register(&self, registration_request: RegistrationRequest) -> anyhow::Result<Data<RegistrationResponse>> {}

            async fn get_public_agent(&self, agent_symbol: &AgentSymbol) -> anyhow::Result<AgentResponse> {}

            async fn get_agent(&self) -> anyhow::Result<AgentResponse> {}

            async fn get_construction_site(&self, waypoint_symbol: &WaypointSymbol) -> anyhow::Result<GetConstructionResponse> {}

            async fn dock_ship(&self, ship_symbol: ShipSymbol) -> anyhow::Result<DockShipResponse> {}

            async fn set_flight_mode(&self, ship_symbol: ShipSymbol, mode: &FlightMode) -> anyhow::Result<NavigateShipResponse> {}

            async fn navigate(&self, ship_symbol: ShipSymbol, to: &WaypointSymbol) -> anyhow::Result<NavigateShipResponse> {}

            async fn refuel(&self, ship_symbol: ShipSymbol, amount: u32, from_cargo: bool) -> anyhow::Result<RefuelShipResponse> {}

            async fn orbit_ship(&self, ship_symbol: ShipSymbol) -> anyhow::Result<OrbitShipResponse> {}

            async fn list_ships(&self, pagination_input: PaginationInput) -> anyhow::Result<PaginatedResponse<Ship>> {}

            async fn list_waypoints_of_system_page(&self, system_symbol: &SystemSymbol, pagination_input: PaginationInput) -> anyhow::Result<PaginatedResponse<Waypoint>> {}

            async fn list_systems_page(&self, pagination_input: PaginationInput) -> anyhow::Result<PaginatedResponse<SystemsPageData>> {}

            async fn get_system(&self, system_symbol: &SystemSymbol) -> anyhow::Result<SystemsPageData> {}

            async fn get_marketplace(&self, waypoint_symbol: WaypointSymbol) -> anyhow::Result<GetMarketResponse> {}

            async fn get_jump_gate(&self, waypoint_symbol: WaypointSymbol) -> anyhow::Result<GetJumpGateResponse> {}

            async fn get_shipyard(&self, waypoint_symbol: WaypointSymbol) -> anyhow::Result<GetShipyardResponse> {}

            async fn create_chart(&self, ship_symbol: ShipSymbol) -> anyhow::Result<CreateChartResponse> {}

            async fn list_agents_page(&self, pagination_input: PaginationInput) -> anyhow::Result<ListAgentsResponse> {}

            async fn get_status(&self) -> anyhow::Result<StStatusResponse> {}

        }
    }

    #[tokio::test]
    async fn test_experiment_with_mockall() {
        let mut mock_client = MockStClient::new();

        mock_client.expect_dock_ship().with(eq(ShipSymbol("FLWI-1".to_string()))).return_once(move |_| {
            Ok(DockShipResponse {
                data: NavOnlyResponse {
                    nav: TestObjects::create_nav(
                        FlightMode::Drift,
                        NavStatus::InTransit,
                        &WaypointSymbol("X1-FOO-BAR".to_string()),
                        &WaypointSymbol("X1-FOO-BAR".to_string()),
                    ),
                },
            })
        });

        let ship = TestObjects::test_ship(500);

        let mut ship_ops = ShipOperations::new(ship, Arc::new(mock_client));
        let result = ship_ops.dock().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dock_if_necessary_behavior_on_docked_ship() {
        let mut mock_client = MockStClient::new();

        let args = BehaviorArgs {
            blackboard: Arc::new(MockTestBlackboard::new()),
        };

        let mocked_client = mock_client.expect_dock_ship().with(eq(ShipSymbol("FLWI-1".to_string()))).returning(move |_| {
            Ok(DockShipResponse {
                data: NavOnlyResponse {
                    nav: TestObjects::create_nav(
                        FlightMode::Drift,
                        NavStatus::InTransit,
                        &WaypointSymbol("X1-FOO-BAR".to_string()),
                        &WaypointSymbol("X1-FOO-BAR".to_string()),
                    ),
                },
            })
        });

        // if ship is docked

        let mut ship = TestObjects::test_ship(500);
        ship.nav.status = NavStatus::Docked;

        let behaviors = ship_behaviors();
        let ship_behavior = behaviors.dock_if_necessary;

        mocked_client.never();

        let mut ship_ops = ShipOperations::new(ship, Arc::new(mock_client));
        let result = ship_behavior.run(&args, &mut ship_ops, Duration::from_millis(1)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dock_if_necessary_behavior_on_orbiting_ship() {
        let mut mock_client = MockStClient::new();

        let args = BehaviorArgs {
            blackboard: Arc::new(MockTestBlackboard::new()),
        };

        let mocked_client = mock_client.expect_dock_ship().with(eq(ShipSymbol("FLWI-1".to_string()))).returning(move |_| {
            Ok(DockShipResponse {
                data: NavOnlyResponse {
                    nav: TestObjects::create_nav(
                        FlightMode::Drift,
                        NavStatus::InTransit,
                        &WaypointSymbol("X1-FOO-BAR".to_string()),
                        &WaypointSymbol("X1-FOO-BAR".to_string()),
                    ),
                },
            })
        });

        // if ship is docked

        let mut ship = TestObjects::test_ship(500);
        ship.nav.status = NavStatus::InOrbit;

        let behaviors = ship_behaviors();
        let ship_behavior = behaviors.dock_if_necessary;

        mocked_client.times(1);

        let mut ship_ops = ShipOperations::new(ship, Arc::new(mock_client));
        let result = ship_behavior.run(&args, &mut ship_ops, Duration::from_millis(1)).await.unwrap();
        assert_eq!(result, Response::Success);
    }

    // Helper function to create a WaypointSymbol
    fn wp(s: &str) -> Arc<WaypointSymbol> {
        Arc::new(WaypointSymbol(s.to_string()))
    }

    #[tokio::test]
    #[traced_test]
    async fn test_explorer_behavior_with_two_waypoints() {
        let mut mock_client = MockStClient::new();
        let mut mock_test_blackboard = MockTestBlackboard::new();

        let current_fuel: u32 = 500;
        let mut ship = TestObjects::test_ship(current_fuel);
        ship.nav.status = NavStatus::InOrbit;

        let waypoint_a1 = wp("X1-FOO-A1");
        let waypoint_a2 = wp("X1-FOO-A2");
        let waypoint_bar = wp("X1-FOO-BAR");

        mock_test_blackboard
            .expect_get_exploration_tasks_for_current_waypoint()
            .withf(|wp| wp.0.contains("X1-FOO-A"))
            .returning(|_| Ok(vec![ExplorationTask::GetMarket]));

        let explorer_waypoints = vec![
            TestObjects::create_waypoint(&waypoint_a1, 100, 0),
            TestObjects::create_waypoint(&waypoint_a2, 200, 0),
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

        mock_test_blackboard
            .expect_compute_path()
            .with(eq((*waypoint_a1).clone()), eq((*waypoint_a2).clone()), eq(30), eq(300), eq(600))
            .returning(move |_, _, _, _, _| Ok(second_hop_actions.clone()));

        mock_test_blackboard.expect_insert_market().with(mockall::predicate::always()).times(2).returning(|_| Ok(()));

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
                        nav: return_nav,
                        fuel: TestObjects::create_fuel(current_fuel, 200),
                    },
                })
            });

        let waypoint_bar_clone = Arc::clone(&waypoint_bar);
        mock_client.expect_set_flight_mode().with(eq(ShipSymbol("FLWI-1".to_string())), eq(FlightMode::Burn)).times(1).returning(move |_, _| {
            Ok(PatchShipNavResponse {
                data: TestObjects::create_nav(FlightMode::Burn, NavStatus::InTransit, &waypoint_bar_clone, &waypoint_bar_clone),
            })
        });

        let waypoint_a1_clone = Arc::clone(&waypoint_a1);
        let waypoint_a2_clone = Arc::clone(&waypoint_a2);
        mock_client.expect_get_marketplace().withf(|wp| wp.0.contains("X1-FOO-A")).times(2).returning(move |wp| {
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

        println!("{}", ship_behavior.to_mermaid());

        let mut ship_ops = ShipOperations::new(ship, Arc::new(mock_client));
        let args = BehaviorArgs {
            blackboard: Arc::new(mock_test_blackboard),
        };

        ship_ops.set_explore_locations(explorer_waypoints);
        let result = ship_behavior.run(&args, &mut ship_ops, Duration::from_millis(1)).await.unwrap();

        assert_eq!(result, Response::Success);
        assert_eq!(ship_ops.nav.waypoint_symbol, *waypoint_a2);
        assert_eq!(ship_ops.travel_action_queue.len(), 0);
        assert_eq!(ship_ops.explore_location_queue.len(), 0);
    }

    #[tokio::test]
    #[traced_test]
    async fn test_navigate_to_destination_behavior() {
        let mut mock_client = MockStClient::new();

        let mut mock_test_blackboard = MockTestBlackboard::new();

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
        let mut mock_client = MockStClient::new();

        let mut mock_test_blackboard = MockTestBlackboard::new();

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
                data: NavOnlyResponse {
                    nav: TestObjects::create_nav(
                        FlightMode::Drift,
                        NavStatus::Docked,
                        &WaypointSymbol("X1-FOO-BAR".to_string()),
                        &WaypointSymbol("X1-FOO-BAR".to_string()),
                    ),
                },
            })
        });

        // 1st waypoint: Orbit after refueling
        mock_client.expect_orbit_ship().with(eq(ShipSymbol("FLWI-1".to_string()))).once().in_sequence(&mut seq).return_once(move |_| {
            Ok(OrbitShipResponse {
                data: NavOnlyResponse {
                    nav: TestObjects::create_nav(
                        FlightMode::Drift,
                        NavStatus::InOrbit,
                        &WaypointSymbol("X1-FOO-BAR".to_string()),
                        &WaypointSymbol("X1-FOO-BAR".to_string()),
                    ),
                },
            })
        });

        // 2nd waypoint: Dock for refueling
        mock_client.expect_dock_ship().with(eq(ShipSymbol("FLWI-1".to_string()))).once().in_sequence(&mut seq).returning(move |_| {
            Ok(DockShipResponse {
                data: NavOnlyResponse {
                    nav: TestObjects::create_nav(
                        FlightMode::Burn,
                        NavStatus::Docked,
                        &WaypointSymbol("X1-FOO-A1".to_string()),
                        &WaypointSymbol("X1-FOO-A1".to_string()),
                    ),
                },
            })
        });

        // 2nd waypoint: Orbit after refueling
        mock_client.expect_orbit_ship().with(eq(ShipSymbol("FLWI-1".to_string()))).once().in_sequence(&mut seq).return_once(move |_| {
            Ok(OrbitShipResponse {
                data: NavOnlyResponse {
                    nav: TestObjects::create_nav(
                        FlightMode::Burn,
                        NavStatus::InOrbit,
                        &WaypointSymbol("X1-FOO-A1".to_string()),
                        &WaypointSymbol("X1-FOO-A1".to_string()),
                    ),
                },
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
}
