use crate::st_model::{distance_to, FlightMode, Fuel, LabelledCoordinate, Ship, TradeGoodSymbol};
use crate::st_model::{MarketData, Waypoint, WaypointSymbol};
use futures::StreamExt;
use itertools::Itertools;
use pathfinding::prelude::astar;
use serde::{Deserialize, Serialize};
use strum_macros::Display;

impl MarketData {
    fn all_trade_goods(&self) -> Vec<TradeGoodSymbol> {
        self.imports
            .iter()
            .chain(self.exports.iter())
            .chain(self.exchange.iter())
            .map(|tg| tg.symbol.clone())
            .collect()
    }
}

pub fn compute_path(
    from: WaypointSymbol,
    to: WaypointSymbol,
    waypoints_of_system: Vec<Waypoint>,
    market_entries_of_system: Vec<MarketData>,
    engine_speed: u32,
    current_fuel: u32,
    fuel_capacity: u32,
) -> Option<Vec<TravelAction>> {
    let waypoints: Vec<PathfindingWaypoint> = waypoints_of_system
        .iter()
        .map(|wps| {
            let is_refueling_station = market_entries_of_system.iter().any(|me| {
                me.symbol == wps.symbol && me.all_trade_goods().contains(&TradeGoodSymbol::FUEL)
            });

            PathfindingWaypoint {
                label: wps.symbol.clone(),
                x: wps.x as i32,
                y: wps.y as i32,
                is_refueling_station,
            }
        })
        .collect();

    let start_idx = waypoints.iter().position(|wp| wp.label == from)?;
    let goal_idx = waypoints.iter().position(|wp| wp.label == to)?;

    let start = State {
        waypoint_idx: start_idx,
        fuel: current_fuel,
    };

    let distance_map: Vec<Vec<u32>> = waypoints
        .iter()
        .map(|from| {
            let to_map: Vec<u32> = waypoints
                .iter()
                .map(|to| {
                    let distance = from.distance_to(to);
                    distance
                })
                .collect();
            to_map
        })
        .collect();

    let problem = Problem {
        goal_idx,
        waypoints: waypoints.clone(),
        distance_map,
        fuel_capacity,
        refuel_time: 2,
        engine_speed,
        allowed_flight_modes: vec![FlightMode::Burn, FlightMode::Cruise, FlightMode::Drift],
    };

    let result = astar(
        &start,
        |s| problem.successors(s, &problem.waypoints),
        |s| problem.heuristic(s),
        |s| s.waypoint_idx == problem.goal_idx,
    );

    result.map(|(path, _cost)| compute_travel_actions(&problem, &path))
}

#[derive(Clone, Debug, Serialize, Deserialize, Hash, PartialEq, Eq)]
struct PathfindingWaypoint {
    pub label: WaypointSymbol,
    pub x: i32,
    pub y: i32,
    pub is_refueling_station: bool,
}

impl PathfindingWaypoint {
    pub(crate) fn distance_to(&self, to: &PathfindingWaypoint) -> u32 {
        distance_to(self.x as i64, self.y as i64, to.x as i64, to.y as i64)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct State {
    waypoint_idx: usize,
    fuel: u32,
}

impl State {
    fn waypoint<'a>(&self, waypoints: &'a Vec<PathfindingWaypoint>) -> &'a PathfindingWaypoint {
        waypoints.get(self.waypoint_idx).unwrap()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Hash, PartialEq, Eq, Display)]
pub enum TravelAction {
    Navigate {
        from: WaypointSymbol,
        to: WaypointSymbol,
        distance: u32,
        travel_time: u32,
        fuel_consumption: u32,
        mode: FlightMode,
        total_time: u32,
    },
    Refuel {
        at: WaypointSymbol,
        total_time: u32,
    },
}

impl TravelAction {
    fn total_time(&self) -> u32 {
        match self {
            TravelAction::Navigate { total_time, .. } => *total_time,
            TravelAction::Refuel { total_time, .. } => *total_time,
        }
    }

    pub fn waypoint_and_time(&self) -> (&WaypointSymbol, &u32) {
        match self {
            TravelAction::Navigate { to, total_time, .. } => (to, total_time),
            TravelAction::Refuel { at, total_time } => (at, total_time),
        }
    }
}

fn determine_travel_mode(problem: &Problem, fuel_consumed: u32, distance: u32) -> FlightMode {
    problem
        .allowed_flight_modes
        .iter()
        .find(|fm| {
            let consumption = fm.calculate_fuel_consumption(distance);
            fuel_consumed == consumption
        })
        .unwrap()
        .clone()
}

struct Problem {
    goal_idx: usize,
    waypoints: Vec<PathfindingWaypoint>,
    fuel_capacity: u32,
    refuel_time: u32,
    engine_speed: u32,
    allowed_flight_modes: Vec<FlightMode>,
    distance_map: Vec<Vec<u32>>,
}

impl Problem {
    fn successors(&self, state: &State, waypoints: &Vec<PathfindingWaypoint>) -> Vec<(State, u32)> {
        let mut result = Vec::new();

        let current_waypoint = state.waypoint(waypoints);

        for (waypoint_idx, distance) in self
            .distance_map
            .get(state.waypoint_idx)
            .unwrap()
            .iter()
            .enumerate()
        {
            let waypoint = self.waypoints.get(waypoint_idx).unwrap();
            // We have waypoints at the same location. If they don't give us an advantage, we skip them
            let is_same_location =
                current_waypoint.x == waypoint.x && current_waypoint.y == waypoint.y;
            let is_better_location =
                !current_waypoint.is_refueling_station && waypoint.is_refueling_station;
            let can_improve_condition = if is_same_location {
                is_better_location
            } else {
                true
            };

            if waypoint_idx != state.waypoint_idx && can_improve_condition {
                for mode in self.allowed_flight_modes.iter() {
                    let fuel_consumption = mode.calculate_fuel_consumption(*distance);
                    let time = mode.calculate_time(*distance, self.engine_speed);

                    if current_waypoint.is_refueling_station {
                        let can_reach = self.fuel_capacity >= fuel_consumption;
                        if can_reach {
                            result.push((
                                State {
                                    waypoint_idx,
                                    fuel: self.fuel_capacity - fuel_consumption,
                                },
                                self.refuel_time + time,
                            ))
                        }
                    } else {
                        let can_reach = state.fuel >= fuel_consumption;
                        if can_reach {
                            result.push((
                                State {
                                    waypoint_idx,
                                    fuel: state.fuel - fuel_consumption,
                                },
                                time,
                            ))
                        }
                    }
                }
            }
        }

        result
    }

    fn heuristic(&self, state: &State) -> u32 {
        let distance = self
            .distance_map
            .get(state.waypoint_idx)
            .unwrap()
            .get(self.goal_idx)
            .unwrap();
        FlightMode::Burn.calculate_time(*distance, self.engine_speed)

        // *self.heuristic_values.get(state.waypoint_idx).unwrap()
    }
}

fn compute_travel_actions(problem: &Problem, path: &Vec<State>) -> Vec<TravelAction> {
    path.iter()
        .tuple_windows()
        .enumerate()
        .fold(Vec::new(), |acc, (idx, (from, to))| {
            let from_waypoint = from.waypoint(&problem.waypoints);
            let to_waypoint = to.waypoint(&problem.waypoints);
            let current_time = acc
                .last()
                .map_or(0, |action: &TravelAction| action.total_time());

            let mut new_actions = Vec::new();

            // Initial refuel action if starting at a refueling station
            if idx == 0 && from_waypoint.is_refueling_station {
                new_actions.push(TravelAction::Refuel {
                    at: from_waypoint.label.clone(),
                    total_time: current_time + problem.refuel_time,
                });
            }

            // Navigation action
            let distance = from_waypoint.distance_to(&to_waypoint);
            let fuel_consumed = if from_waypoint.is_refueling_station {
                problem.fuel_capacity - to.fuel
            } else {
                from.fuel - to.fuel
            };
            let mode = determine_travel_mode(&problem, fuel_consumed, distance);
            let travel_time = mode.calculate_time(distance, problem.engine_speed);

            new_actions.push(TravelAction::Navigate {
                from: from_waypoint.label.clone(),
                to: to_waypoint.label.clone(),
                distance,
                travel_time,
                fuel_consumption: fuel_consumed,
                mode,
                total_time: new_actions
                    .last()
                    .map_or(current_time + travel_time, |action: &TravelAction| {
                        action.total_time() + travel_time
                    }),
            });

            // Refuel action if ending at a refueling station
            if to_waypoint.is_refueling_station {
                new_actions.push(TravelAction::Refuel {
                    at: to_waypoint.label.clone(),
                    total_time: new_actions.last().map_or(
                        current_time + problem.refuel_time,
                        |action: &TravelAction| action.total_time() + problem.refuel_time,
                    ),
                });
            }

            // Combine the accumulated actions with the new actions
            acc.into_iter().chain(new_actions).collect()
        })
}
