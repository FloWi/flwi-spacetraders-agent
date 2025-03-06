use leptos::html::{script, Div, HtmlElement, Pre, H2};
use leptos::logging::log;
use leptos::prelude::*;
use leptos::tachys::html::class::Class;
use leptos_meta::{provide_meta_context, MetaTags, Script, Stylesheet, Title};
use leptos_router::{
    components::{Route, Router, Routes},
    StaticSegment,
};
use st_domain::{
    find_complete_supply_chain, trade_map, SupplyChain, SupplyChainNodeVecExt, TradeGoodSymbol,
};

// Server function uses conversion
#[server]
async fn get_supply_chain() -> Result<SupplyChain, ServerFnError> {
    use st_core;
    let supply_chain = st_core::supply_chain::read_supply_chain().await.unwrap();

    Ok(supply_chain)
}

#[component]
pub fn SupplyChainPage() -> impl IntoView {
    // Use create_resource which is the standard way to handle async data in Leptos
    let supply_chain = OnceResource::new(get_supply_chain());

    view! {
        <Title text="Leptos + Tailwindcss" />
        <main>
            <div class="flex flex-col min-h-screen">
                <Suspense fallback=move || view! { <p>"Loading..."</p> }>
                    <ErrorBoundary fallback=|errors| {
                        view! { <p>"Error: " {format!("{errors:?}")}</p> }
                    }>
                        {move || {
                            supply_chain
                                .get()
                                .map(|result| {
                                    match result {
                                        Ok(data) => render_mermaid_chains(data).into_any(),
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
                        </div>
                    }
                })
                .collect_view()}
        </div>
    }
}
