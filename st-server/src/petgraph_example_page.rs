use leptos::html::*;
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use st_domain::{
    calc_scored_supply_chain_routes, ActivityLevel, MarketTradeGood, MaterializedIndividualSupplyChain, MaterializedSupplyChain, RawMaterialSource,
    ScoredSupplyChainSupportRoute, SupplyLevel, TradeGoodSymbol, WaypointSymbol,
};
use std::collections::HashSet;

use crate::components::supply_chain_graph::{get_activity_fill_color, get_supply_fill_color, SupplyChainGraph};
use crate::tables::scored_supply_chain_route_table::ScoredSupplyChainRouteRow;
use crate::tables::trade_good_overview_table::TradeGoodsOverviewRow;
use itertools::Itertools;
use leptos_struct_table::TableContent;
use st_domain::DeliveryRoute;
use std::ops::Not;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TechNodeSource {
    Raw(RawMaterialSource),
    Market(MarketTradeGood),
}

// Define data structures for tech tree
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TechNode {
    pub id: String,
    pub name: TradeGoodSymbol,
    pub waypoint_symbol: WaypointSymbol,
    pub source: TechNodeSource,
    pub supply_level: Option<SupplyLevel>,
    pub activity_level: Option<ActivityLevel>,
    pub cost: u32,
    pub volume: u32,
    pub width: f64,
    pub height: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
}

#[derive(Default)]
pub struct ColoredLabel {
    pub label: String,
    pub color_class: String,
}

impl ColoredLabel {
    pub(crate) fn empty() -> ColoredLabel {
        Self::new("".to_string(), "".to_string())
    }

    pub fn new(label: String, color_class: String) -> Self {
        Self { label, color_class }
    }
}

impl TechNode {
    pub(crate) fn maybe_supply_text(&self) -> Option<ColoredLabel> {
        match &self.source {
            TechNodeSource::Raw(_) => None,
            TechNodeSource::Market(mtg) => Some(ColoredLabel {
                label: mtg.supply.to_string(),
                color_class: get_supply_fill_color(&mtg.supply),
            }),
        }
    }

    pub(crate) fn maybe_activity_text(&self) -> Option<ColoredLabel> {
        match &self.source {
            TechNodeSource::Raw(_) => None,
            TechNodeSource::Market(mtg) => Some(ColoredLabel {
                label: mtg
                    .activity
                    .clone()
                    .map(|activity| activity.to_string())
                    .unwrap_or("---".to_string()),
                color_class: mtg
                    .activity
                    .clone()
                    .map(|activity| get_activity_fill_color(&activity))
                    .unwrap_or_default(),
            }),
        }
    }
}

impl TechEdge {
    pub(crate) fn maybe_activity_text(&self) -> Option<ColoredLabel> {
        Some(ColoredLabel {
            label: self
                .activity
                .clone()
                .map(|activity| activity.to_string())
                .unwrap_or("---".to_string()),
            color_class: self
                .activity
                .clone()
                .map(|activity| get_activity_fill_color(&activity))
                .unwrap_or_default(),
        })
    }

    pub(crate) fn supply_text(&self) -> Option<ColoredLabel> {
        Some(ColoredLabel {
            label: self.supply.to_string(),
            color_class: get_supply_fill_color(&self.supply),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TechEdge {
    pub source: String,
    pub target: String,
    pub cost: u32,
    pub activity: Option<ActivityLevel>,
    pub volume: u32,
    pub supply: SupplyLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<Point>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curve_factor: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) distance: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) profit: Option<i32>, // Can be negative
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Point {
    pub(crate) x: f64,
    pub(crate) y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[server]
async fn get_materialized_supply_chain() -> Result<Option<(MaterializedSupplyChain, Vec<ScoredSupplyChainSupportRoute>)>, ServerFnError> {
    use st_core::fleet::fleet::collect_fleet_decision_facts;
    use st_core::fleet::fleet_runner::FleetRunner;
    use st_core::st_client::StClientTrait;
    use st_store::bmc::Bmc;
    use st_store::Ctx;

    let state = expect_context::<crate::app::AppState>();
    let bmc = state.bmc;

    let home_waypoint_symbol = bmc
        .agent_bmc()
        .get_initial_agent(&Ctx::Anonymous)
        .await
        .expect("get_initial_agent")
        .headquarters;
    let home_system_symbol = home_waypoint_symbol.system_symbol();

    // easier to get the supply chain this way, since we need plenty of things for computing it
    let facts = collect_fleet_decision_facts(bmc, &home_system_symbol)
        .await
        .expect("collect_fleet_decision_facts");

    if let Some(materialized_supply_chain) = facts.materialized_supply_chain {
        let scored_supply_chain_routes = calc_scored_supply_chain_routes(&materialized_supply_chain, materialized_supply_chain.goods_of_interest.clone());
        assert!(
            materialized_supply_chain
                .raw_delivery_routes
                .is_empty()
                .not(),
            "raw_delivery_routes should not be empty"
        );

        Ok(Some((materialized_supply_chain, scored_supply_chain_routes)))
    } else {
        Ok(None)
    }
}

#[component]
pub fn TechTreePetgraph() -> impl IntoView {
    // Define hardcoded tech tree data
    let resource = Resource::new(|| {}, |_| get_materialized_supply_chain());

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
                                        Ok(
                                            Some(
                                                (materialized_supply_chain, scored_supply_chain_routes),
                                            ),
                                        ) => {
                                            let scored_supply_chains_table_data: Vec<
                                                ScoredSupplyChainRouteRow,
                                            > = scored_supply_chain_routes
                                                .iter()
                                                .cloned()
                                                .map(ScoredSupplyChainRouteRow::from)
                                                .collect_vec();

                                            view! {
                                                <div class="flex flex-col gap-4">
                                                    <div>
                                                        <h2 class="text-2xl font-bold">
                                                            {format!(
                                                                "Scored Supply Chain Routes for System {}",
                                                                materialized_supply_chain.system_symbol,
                                                            )}
                                                        </h2>
                                                        <div class="rounded-md overflow-clip border dark:border-gray-700 w-full mt-4"
                                                            .to_string()>
                                                            <table class="text-sm text-left mb-[-1px]">
                                                                <TableContent
                                                                    rows=scored_supply_chains_table_data
                                                                    scroll_container="html"
                                                                />
                                                            </table>
                                                        </div>
                                                    </div>

                                                    <div>
                                                        {render_overview_of_trade_goods(&materialized_supply_chain)}
                                                    </div>
                                                    // Looks like sugiyama doesn't render independent graphs (no common nodes)
                                                    // disabling this for now and rendering the chains individually
                                                    // <div class="flex flex-col gap-4">
                                                    // <SupplyChainGraph
                                                    // routes=materialized_supply_chain.all_routes.clone()
                                                    // label="Combined Supply Chain".to_string()
                                                    // />
                                                    // <pre>{serde_json::to_string(&materialized_supply_chain.all_routes).unwrap()}</pre>
                                                    // 
                                                    // </div>
                                                    <div>
                                                        <h2 class="text-2xl font-bold my-4">
                                                            "Individual Supply Chains for goods of interest"
                                                        </h2>

                                                        {render_multiple_supply_chains(
                                                            materialized_supply_chain
                                                                .individual_routes_of_goods_for_sale
                                                                .iter()
                                                                .filter(|(tg, _)| {
                                                                    materialized_supply_chain.goods_of_interest.contains(*tg)
                                                                })
                                                                .sorted_by_key(|(tg, _)| {
                                                                    materialized_supply_chain
                                                                        .goods_of_interest
                                                                        .iter()
                                                                        .position(|tg_of_interest| tg_of_interest == *tg)
                                                                        .unwrap_or(usize::MAX)
                                                                })
                                                                .collect_vec(),
                                                        )}

                                                    </div>
                                                    <div>
                                                        <h2 class="text-2xl font-bold my-4">
                                                            "Individual Supply Chains for other goods (that are for sale in system)"
                                                        </h2>

                                                        {render_multiple_supply_chains(
                                                            materialized_supply_chain
                                                                .individual_routes_of_goods_for_sale
                                                                .iter()
                                                                .filter(|(tg, _)| {
                                                                    materialized_supply_chain
                                                                        .goods_of_interest
                                                                        .contains(*tg)
                                                                        .not()
                                                                })
                                                                .sorted_by_key(|(tg, _)| {
                                                                    materialized_supply_chain
                                                                        .goods_of_interest
                                                                        .iter()
                                                                        .position(|tg_of_interest| tg_of_interest == *tg)
                                                                        .unwrap_or(usize::MAX)
                                                                })
                                                                .collect_vec(),
                                                        )}

                                                    </div>

                                                </div>
                                            }
                                                .into_any()
                                        }
                                        Ok(None) => {
                                            view! {
                                                <p>
                                                    "Supply Chain not ready yet - we first need to collect detailed information about all marketplaces"
                                                </p>
                                            }
                                                .into_any()
                                        }
                                        Err(e) => {

                                            view! { <p>"Error: " {e.to_string()}</p> }
                                                .into_any()
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

fn render_overview_of_trade_goods(materialized_supply_chain: &MaterializedSupplyChain) -> impl IntoView {
    let description_rows = vec![
        TradeGoodsOverviewRow::new("Goods Of Interest".to_string(), materialized_supply_chain.goods_of_interest.iter()),
        TradeGoodsOverviewRow::new(
            "Goods For Sale Not Conflicting With Construction".to_string(),
            materialized_supply_chain
                .goods_for_sale_not_conflicting_with_construction
                .iter(),
        ),
        TradeGoodsOverviewRow::new(
            "Goods For Sale Conflicting With Construction".to_string(),
            materialized_supply_chain
                .goods_for_sale_conflicting_with_construction
                .iter(),
        ),
        TradeGoodsOverviewRow::new(
            "Goods With Export Market".to_string(),
            materialized_supply_chain.goods_with_export_market.iter(),
        ),
        TradeGoodsOverviewRow::new(
            "Goods With Exchange Market".to_string(),
            materialized_supply_chain.goods_with_exchange_market.iter(),
        ),
        TradeGoodsOverviewRow::new(
            "Goods With Import Market".to_string(),
            materialized_supply_chain.goods_with_import_market.iter(),
        ),
        TradeGoodsOverviewRow::new(
            "Goods With Supply Market".to_string(),
            materialized_supply_chain.goods_with_supply_market.iter(),
        ),
        TradeGoodsOverviewRow::new(
            "Goods With Supply Market And Import Market".to_string(),
            materialized_supply_chain
                .goods_with_supply_market_and_import_market
                .iter(),
        ),
    ];

    let trade_pairs_for_construction_materials_str = materialized_supply_chain
        .trade_pairs_for_construction_materials
        .iter()
        .map(|(from, to)| format!("{from} --> {to}"))
        .sorted()
        .join("\n");

    let trade_pairs_for_goods_for_sale_conflicting_with_construction_str = materialized_supply_chain
        .trade_pairs_for_goods_for_sale_conflicting_with_construction
        .iter()
        .map(|(from, to)| format!("{from} --> {to}"))
        .sorted()
        .join("\n");

    let trade_pairs_for_goods_for_sale_not_conflicting_with_construction_str = materialized_supply_chain
        .trade_pairs_for_goods_for_sale_not_conflicting_with_construction
        .iter()
        .map(|(from, to)| format!("{from} --> {to}"))
        .sorted()
        .join("\n");

    let no_go_trades_str = materialized_supply_chain
        .no_go_trades
        .iter()
        .map(|(from, to)| format!("{from} --> {to}"))
        .sorted()
        .join("\n");

    view! {
        <div class="flex flex-row gap-4">
            <div class="rounded-md overflow-clip border dark:border-gray-700 w-1/3 mt-4"
                .to_string()>
                <table class="text-sm text-left mb-[-1px]">
                    <TableContent rows=description_rows scroll_container="html" />
                </table>
            </div>
            <div>
                <h3 class="text-xl font-bold">"Trade Pairs For Construction Materials"</h3>
                <pre>{trade_pairs_for_construction_materials_str}</pre>
            </div>
            <div>
                <h3 class="text-xl font-bold">
                    "Trade Pairs For Goods For Sale Not Conflicting With Construction"
                </h3>
                <pre>{trade_pairs_for_goods_for_sale_not_conflicting_with_construction_str}</pre>
            </div>
            <div>
                <h3 class="text-xl font-bold">
                    "Trade Pairs For Goods For Sale Conflicting With Construction"
                </h3>
                <pre>{trade_pairs_for_goods_for_sale_conflicting_with_construction_str}</pre>
            </div>
            <div>
                <h3 class="text-xl font-bold">"No Go Trades"</h3>
                <pre>{no_go_trades_str}</pre>
            </div>
        </div>
    }
}

fn render_multiple_supply_chains(chains: Vec<(&TradeGoodSymbol, &MaterializedIndividualSupplyChain)>) -> impl IntoView {
    chains
        .iter()
        .map(|(tg, materialized_individual_chain)| {
            view! {
                <SupplyChainGraph
                    routes=materialized_individual_chain.all_routes.clone()
                    label=format!("Individual Supply Chain for {tg}")
                />
            }
        })
        .collect_view()
}
