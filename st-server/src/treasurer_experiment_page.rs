use crate::format_duration;
use chrono::{DateTime, Utc};
use itertools::*;
use leptos::prelude::*;
use leptos::{component, view, IntoView};
use leptos_use::{use_interval, UseIntervalReturn};
use phosphor_leptos::{Icon, CLOCK, GAS_PUMP, PACKAGE, TRUCK};
use serde::{Deserialize, Serialize};
use st_domain::budgeting::budgeting::{FleetBudget, TransactionTicket};
use st_domain::budgeting::credits::Credits;
use st_domain::budgeting::treasurer::{InMemoryTreasurer, Treasurer};
use st_domain::{Fleet, FleetId, FleetTask, NavStatus, Ship, ShipSymbol, WaypointSymbol};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use uuid::Uuid;

#[component]
pub fn TreasurerExperimentPage() -> impl IntoView {
    let treasurer_resource = OnceResource::new(prepare_financials());

    view! {
        <div class="bg-blue-950 text-white flex flex-col min-h-screen">
            <h1 class="font-bold text-2xl">"Treasury Overview"</h1>
            <Suspense fallback=move || view! { <p>"Loading..."</p> }>
                <ErrorBoundary fallback=|errors| {
                    view! { <p>"Error: " {format!("{errors:?}")}</p> }
                }>
                    {move || {
                        treasurer_resource
                            .get()
                            .map(|result| {
                                match result {
                                    Ok(treasurer) => {
                                        view! {
                                            <pre>
                                                {serde_json::to_string_pretty(&treasurer)
                                                    .unwrap_or("--".to_string())}
                                            </pre>
                                        }
                                            .into_any()
                                    }
                                    Err(err) => {
                                        view! {
                                            <div>{format!("Error loading treasurer: {}", err)}</div>
                                        }
                                            .into_any()
                                    }
                                }
                            })
                    }}
                </ErrorBoundary>
            </Suspense>
        </div>
    }
}

#[server]
async fn prepare_financials() -> Result<InMemoryTreasurer, ServerFnError> {
    use st_core::fleet::fleet;
    use st_core::fleet::fleet::{collect_fleet_decision_facts, FleetAdmiral};
    use st_core::fleet::fleet_runner::FleetRunner;
    use st_core::in_memory_universe::in_memory_test_universe;
    use st_store::Ctx;

    async fn fn_returning_anyhow_result() -> anyhow::Result<InMemoryTreasurer> {
        let (bmc, client) = in_memory_test_universe::get_test_universe().await;
        let agent = client.get_agent().await?.data;
        let system_symbol = agent.headquarters.system_symbol();

        let mut finance = InMemoryTreasurer::new(Credits::new(agent.credits));

        FleetRunner::load_and_store_initial_data_in_bmcs(Arc::clone(&client), Arc::clone(&bmc))
            .await
            .expect("FleetRunner::load_and_store_initial_data");

        let facts = collect_fleet_decision_facts(Arc::clone(&bmc), &system_symbol).await?;

        let marketplaces_of_interest: HashSet<WaypointSymbol> = HashSet::from_iter(facts.marketplaces_of_interest.iter().cloned());
        let shipyards_of_interest: HashSet<WaypointSymbol> = HashSet::from_iter(facts.shipyards_of_interest.iter().cloned());
        let marketplaces_ex_shipyards: Vec<WaypointSymbol> = marketplaces_of_interest
            .difference(&shipyards_of_interest)
            .cloned()
            .collect_vec();

        let fleet_phase = fleet::create_construction_fleet_phase(&system_symbol, facts.shipyards_of_interest.len(), marketplaces_ex_shipyards.len());

        let (fleets, fleet_tasks): (Vec<Fleet>, Vec<(FleetId, FleetTask)>) =
            fleet::compute_fleets_with_tasks(&facts, &Default::default(), &Default::default(), &fleet_phase);

        let ship_map = facts
            .ships
            .iter()
            .map(|s| (s.symbol.clone(), s.clone()))
            .collect();

        let ship_price_info = bmc
            .shipyard_bmc()
            .get_latest_ship_prices(&Ctx::Anonymous, &system_symbol)
            .await?;

        let ship_fleet_assignment = FleetAdmiral::assign_ships(&fleet_tasks, &ship_map, &fleet_phase.shopping_list_in_order);

        let construction_fleet_id = fleet_tasks
            .iter()
            .find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::ConstructJumpGate { .. }).then_some(id))
            .unwrap();

        let market_observation_fleet_id = fleet_tasks
            .iter()
            .find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::ObserveAllWaypointsOfSystemWithStationaryProbes { .. }).then_some(id))
            .unwrap();

        let mining_fleet_id = fleet_tasks
            .iter()
            .find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::MineOres { .. }).then_some(id))
            .unwrap();

        let siphoning_fleet_id = fleet_tasks
            .iter()
            .find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::SiphonGases { .. }).then_some(id))
            .unwrap();

        let all_next_ship_purchases = fleet::get_all_next_ship_purchases(&ship_map, &fleet_phase);

        finance.redistribute_distribute_fleet_budgets(&fleet_phase, &fleet_tasks, &ship_fleet_assignment, &ship_price_info, &all_next_ship_purchases)?;
        Ok(finance)
    }

    fn_returning_anyhow_result()
        .await
        .map_err(ServerFnError::new)
}
