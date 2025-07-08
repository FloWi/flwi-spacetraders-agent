use crate::behavior_tree::behavior_tree::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use strum::Display;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Display, Hash)]
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
    HasUncompletedTrade,
    HasPermanentExploreLocationEntry,
    SetPermanentExploreLocationAsDestination,
    SetNextObservationTime,
    SleepUntilNextObservationTimeOrShipPurchaseTicketHasBeenAssigned,
    IsAtDestination,
    IsAtObservationWaypoint,
    HasRouteToDestination,
    ComputePathToDestination,
    CollectWaypointInfos,
    RemoveDestination,
    SkipRefueling,
    PrintDestination,
    IsLateEnoughForWaypointObservation,
    SetNextTradeStopAsDestination,
    PerformTradeActionAndMarkAsCompleted,
    HasShipPurchaseTicketForWaypoint,
    RegisterProbeForPermanentObservation,
    SiphonResources,
    JettisonInvaluableCarboHydrates,
    HasCargoSpaceForSiphoning,
    SetSiphoningSiteAsDestination,
    IsAtSiphoningSite,
    WaitForCooldown,
    CreateSellTicketsForAllCargoItems,
    HasCargoSpaceForMining,
    JettisonInvaluableMinerals,
    ExtractResources,
    Survey,
    IsSurveyCapable,
    IsSurveyNecessary,
    SetMiningSiteAsDestination,
    IsAtMiningSite,
    AttemptCargoTransfer,
    AnnounceHaulerReadyForPickup,
    IsHaulerFilledEnoughForDelivery,
    HasAsteroidReachedCriticalLimit,
    SleepForNextWaypointCriticalLimitCheck,
    NegotiateContract,
    AcceptContract,
    CanAffordContract,
    HasAcceptedContract,
    HasActiveContract,
    FulfilContract,
    IsContractDeliveryComplete,
    CreateContractTicketsIfNecessary,
}

pub struct Behaviors {
    pub navigate_to_destination: Behavior<ShipAction>,
    pub adjust_flight_mode_if_necessary: Behavior<ShipAction>,
    pub orbit_if_necessary: Behavior<ShipAction>,
    pub wait_for_arrival_bt: Behavior<ShipAction>,
    pub dock_if_necessary: Behavior<ShipAction>,
    pub refuel_behavior: Behavior<ShipAction>,
    pub explorer_behavior: Behavior<ShipAction>,
    pub stationary_probe_behavior: Behavior<ShipAction>,
    pub trading_behavior: Behavior<ShipAction>,
    pub siphoning_behavior: Behavior<ShipAction>,
    pub mining_hauler_behavior: Behavior<ShipAction>,
    pub miner_behavior: Behavior<ShipAction>,
    pub contractor_behavior: Behavior<ShipAction>,
    pub surveyor_behavior: Behavior<ShipAction>,
}

impl Behaviors {
    pub fn to_labelled_sub_behaviors(&self) -> HashMap<String, Behavior<ShipAction>> {
        let mut all: [(String, Behavior<ShipAction>); 6] = [
            ("navigate_to_destination".to_string(), self.navigate_to_destination.clone()),
            ("adjust_flight_mode_if_necessary".to_string(), self.adjust_flight_mode_if_necessary.clone()),
            ("orbit_if_necessary".to_string(), self.orbit_if_necessary.clone()),
            ("wait_for_arrival_bt".to_string(), self.wait_for_arrival_bt.clone()),
            ("dock_if_necessary".to_string(), self.dock_if_necessary.clone()),
            ("refuel_behavior".to_string(), self.refuel_behavior.clone()),
        ];

        for (_, b) in all.iter_mut() {
            b.update_indices();
        }

        HashMap::from(all)
    }
}

pub fn ship_behaviors() -> Behaviors {
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

    let wait_for_cooldown_bt = Behavior::new_sequence(vec![Behavior::new_action(ShipAction::WaitForCooldown)]);

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

    let travel_action_behavior = Behavior::new_select(vec![execute_navigate_travel_action, execute_refuel_travel_action.clone()]);

    let while_condition_travel_action = Behavior::new_sequence(vec![
        wait_for_arrival_bt.clone(),
        Behavior::new_action(ShipAction::HasTravelActionEntry),
    ]);

    let follow_travel_actions = Behavior::new_while(
        while_condition_travel_action,
        Behavior::new_sequence(vec![
            wait_for_arrival_bt.clone(),
            Behavior::new_select(vec![travel_action_behavior.clone()]),
        ]),
    );

    let mut navigate_to_destination = Behavior::new_select(vec![
        Behavior::new_action(ShipAction::IsAtDestination),
        Behavior::new_sequence(vec![
            Behavior::new_action(ShipAction::FixNavStatusIfNecessary),
            Behavior::new_action(ShipAction::HasDestination),
            wait_for_arrival_bt.clone(),
            Behavior::new_select(vec![
                Behavior::new_action(ShipAction::HasRouteToDestination),
                Behavior::new_action(ShipAction::ComputePathToDestination),
                Behavior::new_action(ShipAction::PrintTravelActions),
            ]),
            follow_travel_actions.clone(),
            Behavior::new_action(ShipAction::FixNavStatusIfNecessary),
            Behavior::new_action(ShipAction::RemoveDestination),
        ]),
    ]);

    let purchase_ship_if_has_ticket = Behavior::new_select(vec![
        Behavior::new_invert(Behavior::new_action(ShipAction::HasShipPurchaseTicketForWaypoint)),
        Behavior::new_sequence(vec![
            dock_if_necessary.clone(),
            Behavior::new_action(ShipAction::PerformTradeActionAndMarkAsCompleted),
            Behavior::new_action(ShipAction::CollectWaypointInfos),
            orbit_if_necessary.clone(),
        ]),
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
            purchase_ship_if_has_ticket.clone(),
            Behavior::new_action(ShipAction::PopExploreLocationAsDestination),
        ]),
    );
    let mut explorer_behavior = Behavior::new_sequence(vec![
        prime_explorer_destination_with_first_explorer_location,
        process_explorer_queue_until_empty,
    ]);

    // if it's late enough for observation: observe and set next observation time
    // else: sleep until observation time (but wake up if we got a ship ticket)
    let observe_waypoint_if_necessary_or_sleep = Behavior::new_select(vec![
        Behavior::new_sequence(vec![
            Behavior::new_action(ShipAction::IsLateEnoughForWaypointObservation),
            Behavior::new_sequence(vec![
                Behavior::new_action(ShipAction::CollectWaypointInfos),
                Behavior::new_action(ShipAction::SetNextObservationTime),
            ]),
        ]),
        Behavior::new_action(ShipAction::SleepUntilNextObservationTimeOrShipPurchaseTicketHasBeenAssigned),
    ]);

    let mut stationary_probe_behavior = Behavior::new_sequence(vec![
        Behavior::new_select(vec![
            Behavior::new_action(ShipAction::HasDestination),
            Behavior::new_sequence(vec![
                Behavior::new_action(ShipAction::HasPermanentExploreLocationEntry),
                Behavior::new_action(ShipAction::SetPermanentExploreLocationAsDestination),
            ]),
        ]),
        navigate_to_destination.clone(),
        dock_if_necessary.clone(),
        Behavior::new_action(ShipAction::RegisterProbeForPermanentObservation),
        Behavior::new_while(
            Behavior::new_action(ShipAction::IsAtObservationWaypoint), //this should be true, because we navigated here ==> intentional endless loop
            Behavior::new_sequence(vec![
                // if we have a ship purchase ticket, we perform the purchase and collect waypoint infos after that
                Behavior::new_select(vec![
                    Behavior::new_invert(Behavior::new_action(ShipAction::HasShipPurchaseTicketForWaypoint)),
                    Behavior::new_sequence(vec![
                        Behavior::new_action(ShipAction::PerformTradeActionAndMarkAsCompleted), //we might have gotten a ship_purchase ticket
                        Behavior::new_action(ShipAction::CollectWaypointInfos),
                    ]),
                ]),
                observe_waypoint_if_necessary_or_sleep,
            ]),
        ),
    ]);

    let mut trading_behavior = Behavior::new_sequence(vec![Behavior::new_while(
        Behavior::new_action(ShipAction::HasUncompletedTrade),
        Behavior::new_sequence(vec![
            Behavior::new_action(ShipAction::SetNextTradeStopAsDestination),
            navigate_to_destination.clone(),
            wait_for_arrival_bt.clone(),
            dock_if_necessary.clone(),
            Behavior::new_action(ShipAction::PerformTradeActionAndMarkAsCompleted),
            Behavior::new_action(ShipAction::CollectWaypointInfos),
        ]),
    )]);

    /*
     fulfillContractIfPossibleNode,
     negotiateContractIfNecessaryNode,
     acceptContractIfNecessaryNode,
    */

    let fulfill_contract_if_possible = Behavior::new_select(vec![
        Behavior::new_invert(Behavior::new_action(ShipAction::IsContractDeliveryComplete)),
        Behavior::new_action(ShipAction::FulfilContract),
    ]);

    let accept_contract_if_necessary_and_within_budget = Behavior::new_sequence(vec![
        Behavior::new_action(ShipAction::HasActiveContract),
        Behavior::new_select(vec![
            Behavior::new_action(ShipAction::HasAcceptedContract),
            Behavior::new_invert(Behavior::new_action(ShipAction::CanAffordContract)),
            Behavior::new_sequence(vec![
                Behavior::new_action(ShipAction::AcceptContract),
                Behavior::new_action(ShipAction::CreateContractTicketsIfNecessary),
            ]),
        ]),
    ]);

    // if we have no contract, stop at any waypoint and negotiate a new one
    let negotiate_contract_if_necessary = Behavior::new_select(vec![
        Behavior::new_action(ShipAction::HasActiveContract),
        Behavior::new_sequence(vec![wait_for_arrival_bt.clone(), Behavior::new_action(ShipAction::NegotiateContract)]),
    ]);

    let purchase_and_deliver_contract_materials = Behavior::new_while(
        Behavior::new_action(ShipAction::HasUncompletedTrade), // contract deliveries are handled the same as normal trades
        Behavior::new_sequence(vec![
            Behavior::new_action(ShipAction::SetNextTradeStopAsDestination),
            navigate_to_destination.clone(),
            wait_for_arrival_bt.clone(),
            dock_if_necessary.clone(),
            Behavior::new_action(ShipAction::PerformTradeActionAndMarkAsCompleted),
            Behavior::new_action(ShipAction::CollectWaypointInfos),
            fulfill_contract_if_possible.clone(),
        ]),
    );

    let mut contractor_behavior = Behavior::new_sequence(vec![
        Behavior::new_action(ShipAction::FixNavStatusIfNecessary),
        fulfill_contract_if_possible,
        negotiate_contract_if_necessary,
        accept_contract_if_necessary_and_within_budget,
        Behavior::new_action(ShipAction::CreateContractTicketsIfNecessary),
        purchase_and_deliver_contract_materials,
    ]);

    let go_to_siphoning_site_if_necessary = Behavior::new_select(vec![
        Behavior::new_sequence(vec![
            Behavior::new_action(ShipAction::FixNavStatusIfNecessary),
            Behavior::new_action(ShipAction::IsAtSiphoningSite),
        ]),
        Behavior::new_sequence(vec![
            Behavior::new_action(ShipAction::SetSiphoningSiteAsDestination),
            navigate_to_destination.clone(),
        ]),
    ]);

    let go_to_mining_site_if_necessary = Behavior::new_select(vec![
        Behavior::new_sequence(vec![
            Behavior::new_action(ShipAction::FixNavStatusIfNecessary),
            Behavior::new_action(ShipAction::IsAtMiningSite),
        ]),
        Behavior::new_sequence(vec![
            Behavior::new_action(ShipAction::SetMiningSiteAsDestination),
            navigate_to_destination.clone(),
        ]),
    ]);

    let siphon_until_full_behavior = Behavior::new_sequence(vec![
        go_to_siphoning_site_if_necessary.clone(),
        wait_for_arrival_bt.clone(),
        Behavior::new_action(ShipAction::JettisonInvaluableCarboHydrates),
        Behavior::new_while(
            Behavior::new_action(ShipAction::HasCargoSpaceForSiphoning),
            Behavior::new_sequence(vec![
                wait_for_cooldown_bt.clone(),
                orbit_if_necessary.clone(),
                Behavior::new_action(ShipAction::SiphonResources),
                Behavior::new_action(ShipAction::JettisonInvaluableCarboHydrates),
            ]),
        ),
    ]);

    let deliver_all_goods_behavior = Behavior::new_sequence(vec![
        Behavior::new_action(ShipAction::CreateSellTicketsForAllCargoItems),
        trading_behavior.clone(),
    ]);

    let mut siphoning_behavior = Behavior::new_sequence(vec![siphon_until_full_behavior, deliver_all_goods_behavior.clone()]);

    let survey_if_necessary = Behavior::new_select(vec![
        Behavior::new_invert(Behavior::new_action(ShipAction::IsSurveyNecessary)),
        Behavior::new_invert(Behavior::new_action(ShipAction::IsSurveyCapable)),
        Behavior::new_sequence(vec![
            wait_for_arrival_bt.clone(),
            wait_for_cooldown_bt.clone(),
            Behavior::new_action(ShipAction::Survey),
        ]),
    ]);

    let extract_resources = Behavior::new_sequence(vec![
        wait_for_arrival_bt.clone(),
        wait_for_cooldown_bt.clone(),
        orbit_if_necessary.clone(),
        Behavior::new_action(ShipAction::ExtractResources),
        Behavior::new_action(ShipAction::JettisonInvaluableMinerals),
    ]);

    let mine_if_necessary = Behavior::new_sequence(vec![survey_if_necessary, extract_resources]);

    let mut mining_hauler_behavior = Behavior::new_select(vec![
        Behavior::new_sequence(vec![
            Behavior::new_action(ShipAction::IsHaulerFilledEnoughForDelivery),
            deliver_all_goods_behavior.clone(),
        ]),
        Behavior::new_sequence(vec![
            go_to_mining_site_if_necessary.clone(),
            wait_for_arrival_bt.clone(),
            orbit_if_necessary.clone(),
            Behavior::new_action(ShipAction::AnnounceHaulerReadyForPickup),
            deliver_all_goods_behavior.clone(),
        ]),
    ]);

    let mut miner_behavior = Behavior::new_sequence(vec![
        go_to_mining_site_if_necessary.clone(),
        wait_for_arrival_bt.clone(),
        orbit_if_necessary.clone(),
        Behavior::new_while(
            Behavior::new_invert(Behavior::new_action(ShipAction::HasCargoSpaceForMining)),
            Behavior::new_action(ShipAction::AttemptCargoTransfer),
        ),
        Behavior::new_while(
            Behavior::new_action(ShipAction::HasAsteroidReachedCriticalLimit),
            Behavior::new_sequence(vec![
                Behavior::new_action(ShipAction::SleepForNextWaypointCriticalLimitCheck),
                Behavior::new_action(ShipAction::CollectWaypointInfos),
            ]),
        ),
        mine_if_necessary.clone(),
    ]);

    let mut surveyor_behavior = Behavior::new_sequence(vec![
        go_to_mining_site_if_necessary.clone(),
        wait_for_arrival_bt.clone(),
        orbit_if_necessary.clone(),
        Behavior::new_while(
            Behavior::new_action(ShipAction::IsAtMiningSite), // intentional endless loop
            Behavior::new_sequence(vec![wait_for_cooldown_bt, Behavior::new_action(ShipAction::Survey)]),
        ),
    ]);

    Behaviors {
        wait_for_arrival_bt: wait_for_arrival_bt.update_indices().clone(),
        orbit_if_necessary: orbit_if_necessary.update_indices().clone(),
        dock_if_necessary: dock_if_necessary.update_indices().clone(),
        adjust_flight_mode_if_necessary: adjust_flight_mode_if_necessary.update_indices().clone(),
        refuel_behavior: execute_refuel_travel_action.update_indices().clone(),
        navigate_to_destination: navigate_to_destination.update_indices().clone(),
        explorer_behavior: explorer_behavior.update_indices().clone(),
        stationary_probe_behavior: stationary_probe_behavior.update_indices().clone(),
        trading_behavior: trading_behavior.update_indices().clone(),
        siphoning_behavior: siphoning_behavior.update_indices().clone(),
        mining_hauler_behavior: mining_hauler_behavior.update_indices().clone(),
        miner_behavior: miner_behavior.update_indices().clone(),
        contractor_behavior: contractor_behavior.update_indices().clone(),
        surveyor_behavior: surveyor_behavior.update_indices().clone(),
    }
}

#[cfg(test)]
mod tests {
    use crate::behavior_tree::behavior_tree::Behavior;
    use crate::behavior_tree::ship_behaviors::ship_behaviors;

    #[tokio::test]
    async fn generate_mermaid_chart() {
        let behaviors = ship_behaviors();

        let mut behavior = behaviors.explorer_behavior;
        behavior.update_indices();

        println!("{}", behavior.to_mermaid())
    }

    #[tokio::test]
    async fn generate_mermaid_chart_2() {
        let repeated_action = Behavior::new_action("Repeated Action".to_string());
        let mut tree = Behavior::new_select(vec![
            repeated_action.clone(),
            Behavior::new_sequence(vec![repeated_action.clone(), Behavior::new_action("Unique Action".to_string())]),
            Behavior::new_while(repeated_action, Behavior::new_action("While Action".to_string())),
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
        let behaviors = &ship_behaviors();
        let mut ship_behavior = behaviors.navigate_to_destination.clone();

        ship_behavior.update_indices();

        let markdown_document = Behavior::generate_markdown_with_details_without_repeat(ship_behavior, behaviors.to_labelled_sub_behaviors());
        println!("{}", markdown_document);
    }
}
