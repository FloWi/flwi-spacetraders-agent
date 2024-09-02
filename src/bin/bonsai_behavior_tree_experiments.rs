use std::collections::HashMap;

use crate::ShipActions::*;
use bonsai_bt::{Action, Event, Sequence, Success, UpdateArgs, Wait, BT};
/*
Sequences

dockRefuelObserveOrbit
eventualJumpSystem
executeNextNavigationTask
isArrivedAtDestination
navigateToDestinationSeq
navigateToWaypointSeq


Selections

computePath
jumpSystemNavigationTask
navigateToDestinationSel
navigateToWaypoint
orbitShipNavigationTask
refuelNavigationTask

Conditions
failIfIsJumpSystemNavigationTask
failIfIsOrbitTask
failIfIsRefuelNavigationTask
failIfIsWaypointNavigationTask
hasPath
isAtDestination
waitForArrival
waitForCoolDown

 */

#[derive(Clone, Debug, Copy)]
pub enum ShipActions {
    CollectWaypointInfo,
    ComputePath,
    Dock,
    JumpSystem,
    NavigateToWaypoint,
    FixNavStatusAfterArrival,
    Orbit,
    Refuel,
    RemoveCurrentWaypoint,
    IsAtDestination,
    IsInTransit,
}

struct ShipId(String);

struct Ship {
    id: String,
}

impl Ship {
    pub fn collect_waypoint_info(&self) {
        println!("CollectWaypointInfo called");
    }
    pub fn compute_path(&self) {
        println!("ComputePath called");
    }
    pub fn dock(&self) {
        println!("Dock called");
    }
    pub fn jump_system(&self) {
        println!("JumpSystem called");
    }
    pub fn navigate_to_waypoint(&self) {
        println!("NavigateToWaypoint called");
    }
    pub fn fix_nav_status_after_arrival(&self) {
        println!("FixNavStatusAfterArrival called");
    }
    pub fn orbit(&self) {
        println!("Orbit called");
    }
    pub fn refuel(&self) {
        println!("Refuel called");
    }
    pub fn remove_current_waypoint(&self) {
        println!("RemoveCurrentWaypoint called");
    }
    pub fn is_at_destination(&self) {
        println!("IsAtDestination called");
    }
    pub fn is_in_transit(&self) {
        println!("IsInTransit called");
    }
}

// A test state machine that can increment and decrement.
fn tick(mut ship: Ship, dt: f64, bt: &mut BT<ShipActions, HashMap<String, i32>>) {
    let e: Event = UpdateArgs { dt }.into();

    let bb = bt.get_blackboard();

    let (_s, _t) = bt.tick(&e, &mut |args, _| match *args.action {
        CollectWaypointInfo => {
            ship.collect_waypoint_info();
            (Success, args.dt)
        }
        ComputePath => {
            ship.compute_path();
            (Success, args.dt)
        }
        Dock => {
            ship.dock();
            (Success, args.dt)
        }
        JumpSystem => {
            ship.jump_system();
            (Success, args.dt)
        }
        NavigateToWaypoint => {
            ship.navigate_to_waypoint();
            (Success, args.dt)
        }
        FixNavStatusAfterArrival => {
            ship.fix_nav_status_after_arrival();
            (Success, args.dt)
        }
        Orbit => {
            ship.orbit();
            (Success, args.dt)
        }
        Refuel => {
            ship.refuel();
            (Success, args.dt)
        }
        RemoveCurrentWaypoint => {
            ship.remove_current_waypoint();
            (Success, args.dt)
        }
        IsAtDestination => {
            ship.is_at_destination();
            (Success, args.dt)
        }
        IsInTransit => {
            ship.is_in_transit();
            (Success, args.dt)
        }
    });
}

fn main() {
    let a: i32 = 0;
    let seq = Sequence(vec![Wait(1.0)]);

    let h: HashMap<String, i32> = HashMap::new();
    let mut bt = BT::new(seq, h);
}
