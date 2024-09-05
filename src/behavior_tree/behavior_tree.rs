use bonsai_bt::{Action, Behavior, Select, Sequence, While};

#[derive(Clone, Debug)]
pub enum ShipAction {
    HasTravelActionEntry,
    WaitForArrival,
    Dock,
    Orbit,
    Navigate,
    Refuel,
    PopTravelAction,
    IsNavigationAction,
    FixNavStatusIfNecessary,
    IsInOrbit,
    IsCorrectFlightMode,
    SetFlightMode,
    NavigateToWaypoint,
    IsDocked,
    IsRefuelAction,
    MarkTravelActionAsCompleteIfPossible,
    CanSkipRefueling,
}

pub struct Behaviors {
    pub travel_behavior: Behavior<ShipAction>,
    pub adjust_flight_mode_if_necessary: Behavior<ShipAction>,
    pub orbit_if_necessary: Behavior<ShipAction>,
    pub wait_for_arrival_bt: Behavior<ShipAction>,
    pub dock_if_necessary: Behavior<ShipAction>,
    pub refuel_behavior: Behavior<ShipAction>,
    pub travel_action_behavior: Behavior<ShipAction>,
}

pub fn ship_navigation_behaviors() -> Behaviors {
    /*
    /// Runs behaviors one by one until all succeeded.
    ///
    /// The sequence fails if a behavior fails.
    /// The sequence succeeds if all the behavior succeeds.
    /// Can be thought of as a short-circuited logical AND gate.
    Sequence(Vec<Behavior<A>>),


    /// Runs behaviors one by one until a behavior succeeds.
    ///
    /// If a behavior fails it will try the next one.
    /// Fails if the last behavior fails.
    /// Can be thought of as a short-circuited logical OR gate.
    Select(Vec<Behavior<A>>),
     */

    let wait_for_arrival_bt = Select(vec![
        Action(ShipAction::WaitForArrival),
        Action(ShipAction::FixNavStatusIfNecessary),
        Action(ShipAction::MarkTravelActionAsCompleteIfPossible),
    ]);

    let orbit_if_necessary = Select(vec![
        Action(ShipAction::IsInOrbit),
        Action(ShipAction::Orbit),
    ]);

    let dock_if_necessary = Select(vec![Action(ShipAction::IsDocked), Action(ShipAction::Dock)]);

    let adjust_flight_mode_if_necessary = Select(vec![
        Action(ShipAction::IsCorrectFlightMode),
        Action(ShipAction::SetFlightMode),
    ]);

    let navigate_behavior = Sequence(vec![
        Action(ShipAction::IsNavigationAction),
        wait_for_arrival_bt.clone(),
        orbit_if_necessary.clone(),
        adjust_flight_mode_if_necessary.clone(),
        Action(ShipAction::NavigateToWaypoint),
    ]);

    let refuel_behavior = Sequence(vec![
        Action(ShipAction::IsRefuelAction),
        wait_for_arrival_bt.clone(),
        Select(vec![
            Action(ShipAction::CanSkipRefueling),
            Sequence(vec![dock_if_necessary.clone(), Action(ShipAction::Refuel)]),
        ]),
        Action(ShipAction::MarkTravelActionAsCompleteIfPossible),
    ]);

    let travel_action_behavior = Select(vec![navigate_behavior, refuel_behavior.clone()]);

    let travel_behavior = While(
        Box::new(Action(ShipAction::HasTravelActionEntry)),
        vec![
            Action(ShipAction::PopTravelAction),
            travel_action_behavior.clone(),
        ],
    );

    Behaviors {
        wait_for_arrival_bt,
        orbit_if_necessary,
        dock_if_necessary,
        adjust_flight_mode_if_necessary,
        refuel_behavior,
        travel_behavior,
        travel_action_behavior,
    }
}
