use itertools::Itertools;
use leptos::prelude::*;
use leptos_meta::Title;
use serde::{Deserialize, Serialize};
use st_domain::{find_complete_supply_chain, trade_map, GetConstructionResponse, MarketTradeGood, SupplyChain, SupplyChainNodeVecExt, TradeGoodSymbol, WaypointSymbol};

// Server function uses conversion
#[server]
async fn get_supply_chain() -> Result<SupplyChain, ServerFnError> {
    use st_core;
    let supply_chain = st_core::supply_chain::read_supply_chain().await.unwrap();

    Ok(supply_chain)
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RelevantMarketData {
    waypoint_symbol: WaypointSymbol,
    trade_goods: Vec<MarketTradeGood>
}

#[server]
async fn get_supply_chain_data() -> Result<(SupplyChain, Vec<RelevantMarketData>, Option<GetConstructionResponse>), ServerFnError> {
    use st_core;
    let supply_chain = st_core::supply_chain::read_supply_chain().await.unwrap();

    use st_store::{Ctx,AgentBmc, StatusBmc, MarketBmc, ConstructionBmc};

    let state = expect_context::<crate::app::AppState>();
    let mm = state.db_model_manager;

    let agent = AgentBmc::get_initial_agent(&Ctx::Anonymous, &mm).await.expect("get_initial_agent");
    let headquarters_waypoint = WaypointSymbol(agent.headquarters);
    let market_data = MarketBmc::get_latest_market_data_for_system(&Ctx::Anonymous, &mm, headquarters_waypoint.system_symbol().0).await.expect("status");
    let relevant_market_data: Vec<RelevantMarketData> = market_data.iter().map(|md| RelevantMarketData{ waypoint_symbol: md.symbol.clone(), trade_goods: md.trade_goods.clone().unwrap_or_default() } ).collect_vec();
    let construction_site = ConstructionBmc::get_construction_site_for_system(&Ctx::Anonymous, &mm, headquarters_waypoint.system_symbol()).await.expect("construction_site");

    Ok((supply_chain, relevant_market_data, construction_site))
}

#[component]
pub fn SupplyChainPage() -> impl IntoView {
    // Use create_resource which is the standard way to handle async data in Leptos
    let supply_chain_resource = OnceResource::new(get_supply_chain_data());


    view! {
        <Title text="Leptos + Tailwindcss" />
        <main>
            <div class="flex flex-col min-h-screen">
                <Suspense fallback=move || view! { <p>"Loading..."</p> }>
                    <ErrorBoundary fallback=|errors| {
                        view! { <p>"Error: " {format!("{errors:?}")}</p> }
                    }>
                        {move || {
                            supply_chain_resource
                                .get()
                                .map(|result| {
                                    match result {
                                        Ok((supply_chain_data, market_data, maybe_construction_site)) => {
                                            view! {
                                                <div class="flex flex-row gap-4">
                                                    <pre class="w-1/2">
                                                        {serde_json::to_string_pretty(&maybe_construction_site)}
                                                    </pre>
                                                    <pre class="w-1/2">
                                                        {serde_json::to_string_pretty(&market_data)}
                                                    </pre>
                                                    <div class="w-1/2">
                                                        {render_mermaid_chains(supply_chain_data).into_any()}
                                                    </div>

                                                </div>
                                            }
                                                .into_any()
                                        }
                                        Err(e) => {
                                            view! { <p>"Error: " {e.to_string()}</p> }.into_any()
                                        }
                                    }
                                })
                        }}
                    </ErrorBoundary>
                </Suspense>
            </div>
            <script type="module">
                "import mermaid from 'https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs';"
            </script>
        </main>
    }
}

fn render_mermaid_chains(supply_chain: SupplyChain) -> impl IntoView {
    let trade_map = trade_map(&supply_chain);

    let goods_of_interest = [
        TradeGoodSymbol::ADVANCED_CIRCUITRY,
        TradeGoodSymbol::FAB_MATS,
        TradeGoodSymbol::SHIP_PLATING,
        TradeGoodSymbol::MICROPROCESSORS,
        TradeGoodSymbol::CLOTHING,
    ];

    let complete_chain = find_complete_supply_chain(goods_of_interest.clone().into(), &trade_map);

    view! {
        <div class="flex flex-col gap-4">
            {
                view! {
                    <div class="flex flex-col">
                        <h2 class="text-2xl font-bold">"Complete chain"</h2>
                        <pre class="mermaid">{complete_chain.to_mermaid()}</pre>
                    </div>
                }
            }
            {goods_of_interest
                .clone()
                .map(|trade_good| {
                    let chain = find_complete_supply_chain(
                        Vec::from([trade_good.clone()]),
                        &trade_map,
                    );
                    view! {
                        <div class="flex flex-col">
                            <h2 class="text-2xl font-bold">{trade_good.to_string()}</h2>
                            <pre class="mermaid">{chain.to_mermaid()}</pre>
                            <pre class="no-mermaid">{chain.to_mermaid()}</pre>
                        </div>
                    }
                })
                .collect_view()}
        </div>
    }
}
