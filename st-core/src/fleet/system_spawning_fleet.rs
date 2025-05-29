use crate::fleet::fleet::{diff_waypoint_symbols, FleetAdmiral};
use anyhow::*;
use chrono::Utc;
use itertools::Itertools;
use st_domain::{Fleet, FleetDecisionFacts, FleetTask, FleetTaskCompletion, Ship, ShipSymbol, ShipTask, ShipTaskCompletionAnalysis, SystemSpawningFleetConfig};
use std::collections::HashMap;
use tracing::{event, span};
use tracing_core::Level;

pub struct SystemSpawningFleet;

impl SystemSpawningFleet {
    pub(crate) fn check_for_task_completion(
        ship_task: &ShipTask,
        fleet: &Fleet,
        fleet_tasks: &[FleetTask],
        cfg: &SystemSpawningFleetConfig,
        facts: &FleetDecisionFacts,
    ) -> Option<ShipTaskCompletionAnalysis> {
        let ship_span = span!(
            Level::INFO,
            "SystemSpawningFleet",
            fleet_id = &fleet.id.0,
            fleet = "SystemSpawningFleet",
            ship_task = ship_task.to_string()
        );

        let _enter = ship_span.enter();

        let result: Option<ShipTaskCompletionAnalysis> = match ship_task {
            ShipTask::ObserveAllWaypointsOnce { waypoint_symbols } => {
                let marketplaces_to_explore = diff_waypoint_symbols(&cfg.marketplace_waypoints_of_interest, &facts.marketplaces_with_up_to_date_infos);
                let shipyards_to_explore = diff_waypoint_symbols(&cfg.shipyard_waypoints_of_interest, &facts.shipyards_with_up_to_date_infos);

                event!(
                    Level::DEBUG,
                    r#"SystemSpawningFleet::check_for_task_completion:
{} marketplace_waypoints_of_interest: {:?}
{} marketplaces_with_up_to_date_infos: {:?}
{} marketplaces_to_explore: {:?}
{} shipyard_waypoints_of_interest: {:?}
{} facts.shipyards_with_up_to_date_infos: {:?}
{} shipyards_to_explore: {:?}
                "#,
                    &cfg.marketplace_waypoints_of_interest.len(),
                    &cfg.marketplace_waypoints_of_interest,
                    &facts.marketplaces_with_up_to_date_infos.len(),
                    &facts.marketplaces_with_up_to_date_infos,
                    &marketplaces_to_explore.len(),
                    &marketplaces_to_explore,
                    &cfg.shipyard_waypoints_of_interest.len(),
                    &cfg.shipyard_waypoints_of_interest,
                    &facts.shipyards_with_up_to_date_infos.len(),
                    &facts.shipyards_with_up_to_date_infos,
                    &shipyards_to_explore.len(),
                    &shipyards_to_explore,
                );

                if marketplaces_to_explore.is_empty() && shipyards_to_explore.is_empty() {
                    let maybe_matching_task = fleet_tasks
                        .iter()
                        .find(|ft| matches!(ft, FleetTask::InitialExploration { .. }));
                    maybe_matching_task.map(|ft| {
                        ShipTaskCompletionAnalysis::ShipTaskDone(FleetTaskCompletion {
                            task: ft.clone(),
                            completed_at: Utc::now(),
                        })
                    })
                } else {
                    // Not completed
                    let open_waypoint_symbols = marketplaces_to_explore
                        .into_iter()
                        .chain(shipyards_to_explore.into_iter())
                        .unique()
                        .collect_vec();
                    Some(ShipTaskCompletionAnalysis::ShipTaskNotDone(ShipTask::ObserveAllWaypointsOnce {
                        waypoint_symbols: open_waypoint_symbols,
                    }))
                }
            }
            _ => {
                // Irrelevant task for this fleet
                None
            }
        };
        match &result {
            None => {
                event!(Level::DEBUG, "FleetTask not finished yet");
            }
            Some(ShipTaskCompletionAnalysis::ShipTaskDone(completed_task)) => {
                event!(
                    Level::INFO,
                    message = "FleetTask completed by finishing this ShipTask",
                    fleet_task = completed_task.task.to_string()
                );
            }
            Some(ShipTaskCompletionAnalysis::ShipTaskNotDone(_)) => {
                event!(Level::INFO, message = "FleetTask not completed yet finishing this ShipTask",);
            }
        }

        result
    }

    pub fn compute_ship_tasks(
        admiral: &FleetAdmiral,
        cfg: &SystemSpawningFleetConfig,
        fleet: &Fleet,
        facts: &FleetDecisionFacts,
        ships: &[&Ship],
    ) -> Result<HashMap<ShipSymbol, ShipTask>> {
        //assert_eq!(ships.len(), 1, "expecting 1 ship");

        // TODO: to optimize we could remove the waypoint from the list where the probe has been spawned

        let marketplaces_to_explore = diff_waypoint_symbols(&cfg.marketplace_waypoints_of_interest, &facts.marketplaces_with_up_to_date_infos);
        let shipyards_to_explore = diff_waypoint_symbols(&cfg.shipyard_waypoints_of_interest, &facts.shipyards_with_up_to_date_infos);

        let all_locations_of_interest = marketplaces_to_explore
            .iter()
            .chain(shipyards_to_explore.iter())
            .unique()
            .cloned()
            .collect_vec();

        //         println!(
        //             r#"SystemSpawningFleet::compute_ship_tasks
        // {} marketplaces_to_explore: {:?}
        // {} marketplace_waypoints_of_interest: {:?}
        // {} marketplaces_with_up_to_date_infos: {:?}
        // {} shipyards_to_explore: {:?}
        // {} shipyard_waypoints_of_interest: {:?}
        // {} shipyards_with_up_to_date_infos: {:?}
        // "#,
        //             &marketplaces_to_explore.len(),
        //             &marketplaces_to_explore,
        //             &cfg.marketplace_waypoints_of_interest.len(),
        //             &cfg.marketplace_waypoints_of_interest,
        //             &facts.marketplaces_with_up_to_date_infos.len(),
        //             &facts.marketplaces_with_up_to_date_infos,
        //             &shipyards_to_explore.len(),
        //             &shipyards_to_explore,
        //             &cfg.shipyard_waypoints_of_interest.len(),
        //             &cfg.shipyard_waypoints_of_interest,
        //             &facts.shipyards_with_up_to_date_infos.len(),
        //             &facts.shipyards_with_up_to_date_infos,
        //         );

        let result = ships
            .iter()
            .map(|s| {
                (
                    s.symbol.clone(),
                    ShipTask::ObserveAllWaypointsOnce {
                        waypoint_symbols: all_locations_of_interest.clone(),
                    },
                )
            })
            .collect();
        Ok(result)
    }
}
