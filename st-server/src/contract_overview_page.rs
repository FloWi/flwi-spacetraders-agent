use crate::ship_overview_page::ShipCard;
use anyhow::{anyhow, Result};
use leptos::html::*;
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use st_domain::{Contract, ContractEvaluationResult, Ship, ShipFrameSymbol, ShipRegistrationRole};

#[server]
async fn get_contract() -> Result<(Option<ContractEvaluationResult>, Ship), ServerFnError> {
    use st_core::contract_manager;
    use st_store::bmc::Bmc;
    use st_store::Ctx;

    async fn anyhow_fn() -> anyhow::Result<(Option<ContractEvaluationResult>, Ship)> {
        let state = expect_context::<crate::app::AppState>();
        let bmc = state.bmc;

        let agent_info = bmc.agent_bmc().get_initial_agent(&Ctx::Anonymous).await?;

        let maybe_contract = bmc
            .contract_bmc()
            .get_youngest_contract(&Ctx::Anonymous, &agent_info.headquarters.system_symbol())
            .await?;

        let latest_market_entries = bmc
            .market_bmc()
            .get_latest_market_data_for_system(&Ctx::Anonymous, &agent_info.headquarters.system_symbol())
            .await?;

        let all_ships = bmc.ship_bmc().get_ships(&Ctx::Anonymous, None).await?;

        let command_ship = all_ships
            .iter()
            .find(|s| s.registration.role == ShipRegistrationRole::Command)
            .ok_or(anyhow!("Command ship not found"))?;

        let waypoints_of_system = bmc
            .system_bmc()
            .get_waypoints_of_system(&Ctx::Anonymous, &command_ship.nav.system_symbol)
            .await?;

        let maybe_contract_result: Option<ContractEvaluationResult> = if let Some(contract) = maybe_contract.clone() {
            let contract_result = contract_manager::calculate_necessary_tickets_for_contract(
                &command_ship.cargo,
                &command_ship.nav.waypoint_symbol,
                &contract,
                &latest_market_entries,
                &waypoints_of_system,
            )?;
            Some(contract_result)
        } else {
            None
        };
        Ok((maybe_contract_result, command_ship.clone()))
    }

    match anyhow_fn().await {
        Ok(res) => Ok(res),
        Err(err) => Err(ServerFnError::ServerError(err.to_string())),
    }
}

#[component]
pub fn ContractOverviewPage() -> impl IntoView {
    let resource = OnceResource::new(get_contract());

    view! {
        // <Title text="Leptos + Tailwindcss" />
        <main>
            <div class="flex flex-col min-h-screen w-full">
                <Transition fallback=move || view! { <p>"Loading..."</p> }>
                    <ErrorBoundary fallback=|errors| {
                        view! { <p>"Error: " {format!("{errors:?}")}</p> }
                    }>
                        {move || {
                            resource
                                .get()
                                .map(|result| {
                                    match result {
                                        Ok((Some(contract_evaluation_result), command_ship)) => {

                                            view! {
                                                <div class="flex flex-col gap-4">
                                                <div class="flex flex-row gap-4">
                                                    <div class="flex flex-col gap-2">
                                                        <h2 class="text-xl font-bold">"Contract"</h2>
                                                        <pre>
                                                            {serde_json::to_string_pretty(
                                                                    &contract_evaluation_result.contract,
                                                                )
                                                                .unwrap()}
                                                        </pre>
                                                    </div>
                                                    <div class="flex flex-col gap-2">
                                                        <h2 class="text-xl font-bold">"Purchase Tickets"</h2>
                                                        <pre>
                                                            {serde_json::to_string_pretty(
                                                                    &contract_evaluation_result.purchase_tickets,
                                                                )
                                                                .unwrap()}
                                                        </pre>
                                                    </div>
                                                    <div class="flex flex-col gap-2">
                                                        <h2 class="text-xl font-bold">"Delivery Tickets"</h2>
                                                        <pre>
                                                            {serde_json::to_string_pretty(
                                                                    &contract_evaluation_result.delivery_tickets,
                                                                )
                                                                .unwrap()}
                                                        </pre>
                                                        <h2 class="text-xl font-bold">"Sell Excess Cargo Tickets"</h2>
                                                        <pre>
                                                            {serde_json::to_string_pretty(
                                                                    &contract_evaluation_result.sell_excess_cargo_tickets,
                                                                )
                                                                .unwrap()}
                                                        </pre>
                                                    </div>
                                                    </div>
                                                    <ShipCard ship=&command_ship maybe_ship_task=None active_trades=vec![] />
                                                </div>
                                            }
                                                .into_any()
                                        }
                                        Ok((None, command_ship)) => {
                                            view! { <p>{"no contract found"}</p> }.into_any()
                                        }
                                        Err(error) => {
                                            view! { <p>{format!("{error:?}")}</p> }.into_any()
                                        }
                                    }
                                })
                        }}
                    </ErrorBoundary>
                </Transition>
            </div>
        </main>
    }
}
