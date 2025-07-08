use crate::components::clipboard_button::ClipboardButton;
use crate::tables::trading_opportunity_table::TradingOpportunityRow;
use itertools::Itertools;
use leptos::prelude::*;
use leptos_meta::Title;
use leptos_struct_table::*;
use serde::{Deserialize, Serialize};
use st_domain::{
    find_complete_supply_chain, Construction, EvaluatedTradingOpportunity, FleetConfig, MarketTradeGood, MaterializedSupplyChain, SupplyChain,
    SupplyChainNodeVecExt, TradeGoodSymbol, Waypoint, WaypointSymbol,
};
use std::collections::{HashMap, HashSet};

#[derive(Serialize, Deserialize, Clone)]
pub struct RelevantMarketData {
    pub waypoint_symbol: WaypointSymbol,
    pub trade_goods: Vec<MarketTradeGood>,
}

#[server]
async fn get_supply_chain_data() -> Result<
    (
        SupplyChain,
        Vec<(WaypointSymbol, Vec<MarketTradeGood>)>,
        Option<Construction>,
        MaterializedSupplyChain,
        Vec<EvaluatedTradingOpportunity>,
        Vec<EvaluatedTradingOpportunity>,
    ),
    ServerFnError,
> {
    use st_core;
    use st_domain::trading;
    use st_store::*;

    async fn anyhow_fn() -> anyhow::Result<(
        SupplyChain,
        Vec<(WaypointSymbol, Vec<MarketTradeGood>)>,
        Option<Construction>,
        MaterializedSupplyChain,
        Vec<EvaluatedTradingOpportunity>,
        Vec<EvaluatedTradingOpportunity>,
    )> {
        let state = expect_context::<crate::app::AppState>();
        let bmc = state.bmc;
        let supply_chain = bmc
            .supply_chain_bmc()
            .get_supply_chain(&Ctx::Anonymous)
            .await
            .unwrap()
            .unwrap();

        // println!("loaded supply_chain");

        let agent = bmc.agent_bmc().load_agent(&Ctx::Anonymous).await?;
        // println!("loaded agent");

        let headquarters_waypoint = agent.headquarters;

        let market_data = bmc
            .market_bmc()
            .get_latest_market_data_for_system(&Ctx::Anonymous, &headquarters_waypoint.system_symbol())
            .await?;

        let fleets = bmc.fleet_bmc().load_fleets(&Ctx::Anonymous).await?;
        let ship_fleet_assignment = bmc
            .fleet_bmc()
            .load_ship_fleet_assignment(&Ctx::Anonymous)
            .await?;

        let market_data: Vec<(WaypointSymbol, Vec<MarketTradeGood>)> = trading::to_trade_goods_with_locations(&market_data);
        // println!("loaded market_data");

        let maybe_construction_site = bmc
            .construction_bmc()
            .get_construction_site_for_system(&Ctx::Anonymous, headquarters_waypoint.system_symbol())
            .await?;

        let waypoints_of_system = bmc
            .system_bmc()
            .get_waypoints_of_system(&Ctx::Anonymous, &headquarters_waypoint.system_symbol())
            .await?;

        let waypoint_map: HashMap<WaypointSymbol, &Waypoint> = waypoints_of_system
            .iter()
            .map(|wp| (wp.symbol.clone(), wp))
            .collect::<HashMap<_, _>>();

        let materialized_supply_chain = st_domain::supply_chain::materialize_supply_chain(
            headquarters_waypoint.system_symbol(),
            &supply_chain,
            &market_data,
            &waypoint_map,
            &maybe_construction_site,
        )?;

        let ships = bmc.ship_bmc().get_ships(&Ctx::Anonymous, None).await?;

        // println!("loaded ships");

        let trading_opportunities =
            trading::find_trading_opportunities_sorted_by_profit_per_distance_unit(&market_data, &waypoint_map, &materialized_supply_chain.no_go_trades);

        // println!("calculated {} trading_opportunities", trading_opportunities.len());

        let maybe_construction_fleet = fleets
            .iter()
            .find(|f| matches!(f.cfg, FleetConfig::ConstructJumpGateCfg { .. }));

        let trading_ships = if let Some(construction_fleet) = maybe_construction_fleet {
            let construction_fleet_ships = ship_fleet_assignment
                .iter()
                .filter_map(|(ss, fleet_id)| (fleet_id == &construction_fleet.id).then_some(ss.clone()))
                .collect::<HashSet<_>>();

            ships
                .iter()
                .filter(|ship| construction_fleet_ships.contains(&ship.symbol))
                .collect_vec()
        } else {
            vec![]
        };

        // println!("found {} ships in construction_fleet", trading_ships.len());

        let evaluated_trading_opportunities: Vec<EvaluatedTradingOpportunity> =
            trading::evaluate_trading_opportunities(&trading_ships, &waypoint_map, &trading_opportunities, agent.credits);

        // println!("calculated {} evaluated_trading_opportunities", evaluated_trading_opportunities.len());

        let active_trades = HashSet::new();

        let trading_decision = trading::find_optimal_trading_routes_exhaustive(&evaluated_trading_opportunities, &active_trades);

        // println!("calculated {} trading_decision(s)", trading_decision.len());

        Ok((
            supply_chain,
            market_data,
            maybe_construction_site,
            materialized_supply_chain,
            evaluated_trading_opportunities,
            trading_decision,
        ))
    }

    let result = anyhow_fn().await;

    result.map_err(ServerFnError::new)
}

#[component]
pub fn SupplyChainPage() -> impl IntoView {
    let supply_chain_resource = Resource::new(|| {}, |_| get_supply_chain_data());

    view! {
        <Title text="Leptos + Tailwindcss" />
        <main>
            <div class="flex flex-col">
                <Transition fallback=move || view! { <p>"Loading..."</p> }>
                    <ErrorBoundary fallback=|errors| {
                        view! {
                            <div>
                                <p>"Error: " {format!("{errors:?}")}</p>
                                <p>
                                    "In order to compute the supply chain, we need detailed information about all marketplaces"
                                </p>
                            </div>
                        }
                    }>
                        {move || {
                            supply_chain_resource
                                .get()
                                .map(|result| {
                                    match result {
                                        Ok(
                                            (
                                                supply_chain,
                                                market_data,
                                                maybe_construction_site,
                                                materialized_supply_chain,
                                                evaluated_trading_opportunities,
                                                trading_decision,
                                            ),
                                        ) => {
                                            let trading_opportunities_table_data: Vec<
                                                TradingOpportunityRow,
                                            > = materialized_supply_chain
                                                .trading_opportunities
                                                .iter()
                                                .cloned()
                                                .sorted_by_key(|tr_opp| tr_opp.profit_per_unit_per_distance)
                                                .rev()
                                                .map(TradingOpportunityRow::from)
                                                .collect_vec();

                                            view! {
                                                <div class="flex flex-col gap-4">
                                                    <div class="w-full flex flex-col gap-4">
                                                        <h2 class="text-2xl font-bold">"Trading Opportunities"</h2>
                                                        <div class="rounded-md overflow-clip border dark:border-gray-700 w-full"
                                                            .to_string()>
                                                            <table class="text-sm text-left text-gray-500 dark:text-gray-400 mb-[-1px]">
                                                                <TableContent
                                                                    rows=trading_opportunities_table_data
                                                                    scroll_container="html"
                                                                />

                                                            </table>
                                                        </div>

                                                        <h2 class="text-2xl font-bold">"Explanation"</h2>
                                                        <pre>{materialized_supply_chain.explanation}</pre>
                                                        <div class="w-1/2">
                                                            {render_mermaid_chains(
                                                                    supply_chain,
                                                                    &materialized_supply_chain.goods_of_interest,
                                                                )
                                                                .into_any()}
                                                        </div>
                                                        <h2 class="text-2xl font-bold">"Raw Delivery Routes"</h2>
                                                        <ClipboardButton
                                                            clipboard_text=serde_json::to_string_pretty(
                                                                    &materialized_supply_chain.raw_delivery_routes,
                                                                )
                                                                .unwrap_or("---".to_string())
                                                            label="Copy to Clipboard".to_string()
                                                        />
                                                        <pre>
                                                            {serde_json::to_string_pretty(
                                                                    &materialized_supply_chain.raw_delivery_routes,
                                                                )
                                                                .unwrap()}
                                                        </pre>
                                                        <h2 class="text-2xl font-bold">"Trading Decision"</h2>
                                                        <ClipboardButton
                                                            clipboard_text=serde_json::to_string_pretty(
                                                                    &trading_decision,
                                                                )
                                                                .unwrap_or("---".to_string())
                                                            label="Copy to Clipboard".to_string()
                                                        />
                                                        <pre>
                                                            {serde_json::to_string_pretty(&trading_decision).unwrap()}
                                                        </pre>
                                                        <h2 class="text-2xl font-bold">
                                                            "Evaluated Trading Opportunities"
                                                        </h2>
                                                        <ClipboardButton
                                                            clipboard_text=serde_json::to_string_pretty(
                                                                    &evaluated_trading_opportunities,
                                                                )
                                                                .unwrap_or("---".to_string())
                                                            label="Copy to Clipboard".to_string()
                                                        />
                                                        <pre>
                                                            {serde_json::to_string_pretty(
                                                                    &evaluated_trading_opportunities,
                                                                )
                                                                .unwrap()}
                                                        </pre>

                                                        <h2 class="text-2xl font-bold">"Construction Site"</h2>
                                                        <pre>
                                                            {serde_json::to_string_pretty(&maybe_construction_site)}
                                                        </pre>
                                                        <h2 class="text-2xl font-bold">"Market Data"</h2>
                                                        <pre>{serde_json::to_string_pretty(&market_data)}</pre>
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
                </Transition>
            </div>
            <script type="module">
                r#"import mermaid from 'https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs';
                  mermaid.initialize({ startOnLoad: false, theme: 'dark' });
                
                  // Function to render mermaid diagrams
                  function renderMermaid() {
                    const elements = document.querySelectorAll('pre code.language-mermaid, .mermaid');
                    elements.forEach((element, index) => {
                      const graphDefinition = element.textContent;
                      const graphId = `mermaid-${index}`;
                      mermaid.render(graphId, graphDefinition).then(({svg}) => {
                        element.innerHTML = svg;
                      });
                    });
                  }
                
                  // Run after DOM is loaded and on updates
                  document.addEventListener('DOMContentLoaded', renderMermaid);
                  // // For Leptos reactivity, you might need to call this after updates
                  // setTimeout(renderMermaid, 10);
                "#
            </script>
        </main>
    }
}

fn render_mermaid_chains(supply_chain: SupplyChain, goods_of_interest: &[TradeGoodSymbol]) -> impl IntoView {
    let complete_chain = find_complete_supply_chain(goods_of_interest, &supply_chain.trade_map);

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
                .iter()
                .cloned()
                .map(|trade_good| {
                    let chain = find_complete_supply_chain(
                        &[trade_good.clone()],
                        &supply_chain.trade_map,
                    );
                    view! {
                        <div class="flex flex-col">
                            <h2 class="text-2xl font-bold">{trade_good.to_string()}</h2>
                            <pre class="mermaid">{chain.to_mermaid()}</pre>
                        // <pre class="no-mermaid">{chain.to_mermaid()}</pre>
                        </div>
                    }
                })
                .collect_view()}
        </div>
    }
}
