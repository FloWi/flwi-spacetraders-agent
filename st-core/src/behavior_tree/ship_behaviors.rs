use crate::behavior_tree::behavior_tree::Behavior::*;
use crate::behavior_tree::behavior_tree::*;
use serde::Serialize;
use std::collections::HashMap;
use std::fmt::Display;
use strum_macros::Display;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Display, Hash)]
pub enum ShipAction {
    HasTravelActionEntry,
    WaitForArrival,
    Dock,
    Orbit,
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
    PrintTravelActions,
    HasExploreLocationEntry,
    PopExploreLocationAsDestination,
    PrintExploreLocations,
    HasDestination,
    IsAtDestination,
    HasRouteToDestination,
    ComputePathToDestination,
    CollectWaypointInfos,
    RemoveDestination,
    SkipRefueling,
    PrintDestination,
}

pub struct Behaviors {
    pub navigate_to_destination: Behavior<ShipAction>,
    pub adjust_flight_mode_if_necessary: Behavior<ShipAction>,
    pub orbit_if_necessary: Behavior<ShipAction>,
    pub wait_for_arrival_bt: Behavior<ShipAction>,
    pub dock_if_necessary: Behavior<ShipAction>,
    pub refuel_behavior: Behavior<ShipAction>,
    pub travel_action_behavior: Behavior<ShipAction>,
    pub explorer_behavior: Behavior<ShipAction>,
}

impl Behaviors {
    pub fn to_labelled_sub_behaviors(&self) -> HashMap<String, Behavior<ShipAction>> {
        let mut all: [(String, Behavior<ShipAction>); 7] = [
            (
                "travel_behavior".to_string(),
                self.navigate_to_destination.clone(),
            ),
            (
                "adjust_flight_mode_if_necessary".to_string(),
                self.adjust_flight_mode_if_necessary.clone(),
            ),
            (
                "orbit_if_necessary".to_string(),
                self.orbit_if_necessary.clone(),
            ),
            (
                "wait_for_arrival_bt".to_string(),
                self.wait_for_arrival_bt.clone(),
            ),
            (
                "dock_if_necessary".to_string(),
                self.dock_if_necessary.clone(),
            ),
            ("refuel_behavior".to_string(), self.refuel_behavior.clone()),
            (
                "travel_action_behavior".to_string(),
                self.travel_action_behavior.clone(),
            ),
        ];

        for (_, b) in all.iter_mut() {
            b.update_indices();
        }

        HashMap::from(all)
    }
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

    let mut wait_for_arrival_bt = Behavior::new_sequence(vec![
        Behavior::new_action(ShipAction::WaitForArrival),
        Behavior::new_action(ShipAction::FixNavStatusIfNecessary),
        Behavior::new_action(ShipAction::MarkTravelActionAsCompleteIfPossible),
        Behavior::new_action(ShipAction::PrintTravelActions),
    ]);

    let mut orbit_if_necessary = Behavior::new_select(vec![
        Behavior::new_action(ShipAction::IsInOrbit),
        Behavior::new_action(ShipAction::Orbit),
    ]);

    let mut dock_if_necessary = Behavior::new_select(vec![
        Behavior::new_action(ShipAction::IsDocked),
        Behavior::new_action(ShipAction::Dock),
    ]);

    let mut adjust_flight_mode_if_necessary = Behavior::new_select(vec![
        Behavior::new_action(ShipAction::IsCorrectFlightMode),
        Behavior::new_action(ShipAction::SetFlightMode),
    ]);

    let execute_navigate_travel_action = Behavior::new_sequence(vec![
        Behavior::new_action(ShipAction::IsNavigationAction),
        wait_for_arrival_bt.clone(),
        orbit_if_necessary.clone(),
        adjust_flight_mode_if_necessary.clone(),
        Behavior::new_action(ShipAction::NavigateToWaypoint),
        Behavior::new_action(ShipAction::WaitForArrival),
    ]);

    let mut execute_refuel_travel_action = Behavior::new_sequence(vec![
        Behavior::new_action(ShipAction::IsRefuelAction),
        wait_for_arrival_bt.clone(),
        Behavior::new_select(vec![
            Behavior::new_action(ShipAction::CanSkipRefueling),
            Behavior::new_sequence(vec![
                dock_if_necessary.clone(),
                Behavior::new_action(ShipAction::Refuel),
                orbit_if_necessary.clone(),
            ]),
        ]),
    ]);

    let mut travel_action_behavior = Behavior::new_select(vec![
        execute_navigate_travel_action,
        execute_refuel_travel_action.clone(),
    ]);

    let while_condition_travel_action = Behavior::new_sequence(vec![
        wait_for_arrival_bt.clone(),
        Behavior::new_action(ShipAction::HasTravelActionEntry),
    ]);

    let mut follow_travel_actions = Behavior::new_while(
        while_condition_travel_action,
        Behavior::new_sequence(vec![
            wait_for_arrival_bt.clone(),
            Behavior::new_select(vec![travel_action_behavior.clone()]),
        ]),
    );

    let mut navigate_to_destination = Behavior::new_sequence(vec![
        Behavior::new_action(ShipAction::HasDestination),
        wait_for_arrival_bt.clone(),
        Behavior::new_select(vec![
            Behavior::new_action(ShipAction::HasRouteToDestination),
            Behavior::new_action(ShipAction::ComputePathToDestination),
            Behavior::new_action(ShipAction::PrintTravelActions),
        ]),
        follow_travel_actions.clone(),
        Behavior::new_action(ShipAction::RemoveDestination),
    ]);

    let prime_explorer_destination_with_first_explorer_location = Behavior::new_select(vec![
        Behavior::new_invert(Behavior::new_action(ShipAction::PrintExploreLocations)),
        Behavior::new_action(ShipAction::HasDestination),
        Behavior::new_action(ShipAction::PopExploreLocationAsDestination),
    ]);

    let while_condition_explorer = Behavior::new_select(vec![
        Behavior::new_action(ShipAction::HasDestination),
        Behavior::new_action(ShipAction::HasExploreLocationEntry),
    ]);

    let process_explorer_queue_until_empty = Behavior::new_while(
        while_condition_explorer,
        Behavior::new_sequence(vec![
            Behavior::new_action(ShipAction::PrintDestination),
            Behavior::new_action(ShipAction::PrintExploreLocations),
            wait_for_arrival_bt.clone(),
            navigate_to_destination.clone(),
            Behavior::new_action(ShipAction::CollectWaypointInfos),
            Behavior::new_action(ShipAction::PopExploreLocationAsDestination),
        ]),
    );
    let mut explorer_behavior = Behavior::new_sequence(vec![
        prime_explorer_destination_with_first_explorer_location,
        process_explorer_queue_until_empty,
    ]);

    Behaviors {
        wait_for_arrival_bt: wait_for_arrival_bt.update_indices().clone(),
        orbit_if_necessary: orbit_if_necessary.update_indices().clone(),
        dock_if_necessary: dock_if_necessary.update_indices().clone(),
        adjust_flight_mode_if_necessary: adjust_flight_mode_if_necessary.update_indices().clone(),
        refuel_behavior: execute_refuel_travel_action.update_indices().clone(),
        navigate_to_destination: navigate_to_destination.update_indices().clone(),
        travel_action_behavior: travel_action_behavior.update_indices().clone(),
        explorer_behavior: explorer_behavior.update_indices().clone(),
    }
}

#[cfg(test)]
mod tests {
    use crate::behavior_tree::behavior_tree::Behavior;
    use crate::behavior_tree::ship_behaviors::ship_navigation_behaviors;

    #[tokio::test]
    async fn generate_mermaid_chart() {
        let behaviors = ship_navigation_behaviors();

        let mut behavior = behaviors.explorer_behavior;
        behavior.update_indices();

        println!("{}", behavior.to_mermaid())
    }

    #[tokio::test]
    async fn generate_mermaid_chart_2() {
        let repeated_action = Behavior::new_action("Repeated Action".to_string());
        let mut tree = Behavior::new_select(vec![
            repeated_action.clone(),
            Behavior::new_sequence(vec![
                repeated_action.clone(),
                Behavior::new_action("Unique Action".to_string()),
            ]),
            Behavior::new_while(
                repeated_action,
                Behavior::new_action("While Action".to_string()),
            ),
        ]);

        // Update indices
        tree.update_indices();
        dbg!(&tree);

        // Generate Mermaid diagram
        println!("{}", tree.to_mermaid());

        // Access the index of the root node
        println!("Root node index: {:?}", tree.index());
    }

    #[test]
    fn generate_markdown() {
        let behaviors = &ship_navigation_behaviors();
        let mut ship_behavior = behaviors.navigate_to_destination.clone();

        ship_behavior.update_indices();

        let markdown_document = Behavior::generate_markdown_with_details_without_repeat(
            ship_behavior,
            behaviors.to_labelled_sub_behaviors(),
        );
        println!("{}", markdown_document);
    }
}
