use crate::format_duration;
use chrono::{DateTime, Utc};
use itertools::*;
use leptos::prelude::*;
use leptos::{component, view, IntoView};
use leptos_use::{use_interval, UseIntervalReturn};
use phosphor_leptos::{Icon, CLOCK, GAS_PUMP, PACKAGE, TRUCK};
use serde::{Deserialize, Serialize};
use st_domain::{FleetDecisionFacts, FleetTask, NavStatus, Ship, WaypointSymbol};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShipsOverview {
    ships: Vec<Ship>,
    last_update: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum GetShipsMode {
    AllShips,
    OnlyChangesSince { filter_timestamp_gte: DateTime<Utc> },
}

#[server]
async fn get_fleet_decision_facts() -> Result<(FleetDecisionFacts, Vec<FleetTask>), ServerFnError> {
    use st_core::fleet::collect_fleet_decision_facts;
    use st_core::fleet::compute_fleet_tasks;
    use st_store::AgentBmc;
    use st_store::Ctx;

    let state = expect_context::<crate::app::AppState>();
    let mm = state.db_model_manager;

    let home_waypoint_symbol = WaypointSymbol(AgentBmc::get_initial_agent(&Ctx::Anonymous, &mm).await.expect("get_initial_agent").headquarters);
    let home_system_symbol = home_waypoint_symbol.system_symbol();

    let decision_facts = collect_fleet_decision_facts(&mm, home_system_symbol.clone()).await.expect("collect_fleet_decision_facts");
    let fleet_tasks = compute_fleet_tasks(home_system_symbol, decision_facts.clone());

    Ok((decision_facts, fleet_tasks))
}

#[component]
pub fn FleetOverviewPage() -> impl IntoView {
    let UseIntervalReturn {
        counter,
        reset,
        is_active,
        pause,
        resume,
    } = use_interval(5000);

    let fleet_decision_facts_resource = Resource::new(move || counter.get(), |count| get_fleet_decision_facts());

    view! {
        <div class="bg-blue-950 text-white flex flex-col min-h-screen">
            <h1 class="font-bold text-2xl">"Fleet Decision Facts"</h1>
            <div>
                <Transition>
                    {move || {
                        match fleet_decision_facts_resource.get() {
                            Some(Ok((fleet_decision_facts, fleet_tasks))) => {

                                view! {
                                    <div class="flex flex-col gap-4 p-4">
                                        <div class="flex flex-col gap-2">

                                            <h2 class="font-bold text-xl">"Fleet Tasks"</h2>
                                            <pre>
                                                {serde_json::to_string_pretty(&fleet_tasks)
                                                    .unwrap_or("---".to_string())}
                                            </pre>
                                        </div>
                                        <div class="flex flex-row gap-4 p-4">
                                            <div class="flex flex-col gap-2">

                                                <h2 class="font-bold text-xl">
                                                    "Marketplaces of interest"
                                                </h2>
                                                <pre>
                                                    {serde_json::to_string_pretty(
                                                            &fleet_decision_facts.marketplaces_of_interest,
                                                        )
                                                        .unwrap_or("---".to_string())}
                                                </pre>
                                            </div>
                                            <div class="flex flex-col gap-2">

                                                <h2 class="font-bold text-xl">"Up To Date Marketplaces"</h2>
                                                <pre>
                                                    {serde_json::to_string_pretty(
                                                            &fleet_decision_facts.marketplaces_with_up_to_date_infos,
                                                        )
                                                        .unwrap_or("---".to_string())}
                                                </pre>
                                            </div>
                                            <div class="flex flex-col gap-2">

                                                <h2 class="font-bold text-xl">"Shipyards Of Interest"</h2>
                                                <pre>
                                                    {serde_json::to_string_pretty(
                                                            &fleet_decision_facts.shipyards_of_interest,
                                                        )
                                                        .unwrap_or("---".to_string())}
                                                </pre>
                                            </div>
                                            <div class="flex flex-col gap-2">

                                                <h2 class="font-bold text-xl">
                                                    "Shipyards With Up To Date Infos"
                                                </h2>
                                                <pre>
                                                    {serde_json::to_string_pretty(
                                                            &fleet_decision_facts.shipyards_with_up_to_date_infos,
                                                        )
                                                        .unwrap_or("---".to_string())}
                                                </pre>
                                            </div>
                                            <div class="flex flex-col gap-2">

                                                <h2 class="font-bold text-xl">"Construction Site"</h2>
                                                <pre>
                                                    {serde_json::to_string_pretty(
                                                            &fleet_decision_facts.construction_site,
                                                        )
                                                        .unwrap_or("---".to_string())}
                                                </pre>
                                            </div>
                                            <div class="flex flex-col gap-2">

                                                <h2 class="font-bold text-xl">
                                                    "Materialized Supply Chain"
                                                </h2>
                                                <pre>
                                                    {serde_json::to_string_pretty(
                                                            &fleet_decision_facts.materialized_supply_chain,
                                                        )
                                                        .unwrap_or("---".to_string())}
                                                </pre>
                                            </div>
                                            <div class="flex flex-col gap-2">

                                                <h2 class="font-bold text-xl">"Ships"</h2>
                                                <pre>
                                                    {serde_json::to_string_pretty(&fleet_decision_facts.ships)
                                                        .unwrap_or("---".to_string())}
                                                </pre>
                                            </div>
                                        </div>

                                    </div>
                                }
                                    .into_any()
                            }
                            _ => {

                                view! { <div>"No Facts - it's 2025"</div> }
                                    .into_any()
                            }
                        }
                    }}
                </Transition>
            </div>
        </div>
    }
}
