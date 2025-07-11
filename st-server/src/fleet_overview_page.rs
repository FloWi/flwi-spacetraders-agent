use crate::components::clipboard_button::ClipboardButton;

use chrono::{DateTime, Utc};
use leptos::prelude::*;
use leptos::{component, view, IntoView};
use serde::{Deserialize, Serialize};
use st_domain::{FleetDecisionFacts, FleetPhase, FleetsOverview, Ship};

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
async fn get_fleet_decision_facts() -> Result<(FleetDecisionFacts, FleetsOverview, FleetPhase), ServerFnError> {
    use st_core::fleet::fleet;
    use st_store::Ctx;
    use std::sync::Arc;

    let state = expect_context::<crate::app::AppState>();
    let bmc = state.bmc;

    let home_waypoint_symbol = bmc
        .agent_bmc()
        .get_initial_agent(&Ctx::Anonymous)
        .await
        .expect("get_initial_agent")
        .headquarters;
    let home_system_symbol = home_waypoint_symbol.system_symbol();

    let decision_facts = fleet::collect_fleet_decision_facts(Arc::clone(&bmc), &home_system_symbol)
        .await
        .expect("collect_fleet_decision_facts");

    let fleet_overview = st_store::load_fleet_overview(Arc::clone(&bmc), &Ctx::Anonymous)
        .await
        .expect("load_overview");

    // Create a construction fleet phase
    let fleet_phase = fleet::compute_fleet_phase_with_tasks(home_system_symbol, &decision_facts, &fleet_overview.completed_fleet_tasks);

    Ok((decision_facts, fleet_overview, fleet_phase))
}

#[component]
pub fn FleetOverviewPage() -> impl IntoView {
    let fleet_decision_facts_resource = Resource::new(|| {}, |_| get_fleet_decision_facts());

    #[cfg(not(feature = "ssr"))]
    let _handle = leptos_use::use_interval_fn(move || fleet_decision_facts_resource.refetch(), 5_000);

    view! {
        <div class="bg-blue-950 text-white flex flex-col min-h-screen">
            <h1 class="font-bold text-2xl">"Fleet Decision Facts"</h1>
            <div>
                <Transition>
                    {move || {
                        match fleet_decision_facts_resource.get() {
                            Some(Ok((fleet_decision_facts, fleets_overview, fleet_phase))) => {

                                view! {
                                    <div class="flex flex-col gap-4 p-4">
                                        <div class="flex flex-row gap-4 p-4">
                                            <div class="flex flex-col gap-2">

                                                <h2 class="font-bold text-xl">
                                                    "Fleet Phase with Shopping List"
                                                </h2>
                                                <pre>
                                                    {serde_json::to_string_pretty(&fleet_phase)
                                                        .unwrap_or("---".to_string())}
                                                </pre>
                                            </div>
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
                                        <div class="flex flex-col gap-2">

                                            <h2 class="font-bold text-xl">"Super Fleet Admiral"</h2>
                                            <ClipboardButton
                                                clipboard_text=serde_json::to_string_pretty(
                                                        &fleets_overview,
                                                    )
                                                    .unwrap_or("---".to_string())
                                                label="Copy to Clipboard".to_string()
                                            />
                                            <pre>
                                                {serde_json::to_string_pretty(&fleets_overview)
                                                    .unwrap_or("---".to_string())}
                                            </pre>
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
