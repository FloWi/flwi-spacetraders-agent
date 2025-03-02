use leptos::html::{script, Div, HtmlElement, Pre, H2};
use leptos::logging::log;
use leptos::prelude::*;
use leptos::tachys::html::class::Class;
use leptos_meta::{provide_meta_context, MetaTags, Script, Stylesheet, Title};
use leptos_router::{
    components::{Route, Router, Routes},
    StaticSegment,
};

pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <AutoReload options=options.clone() />
                <HydrationScripts options />
                <MetaTags />
            </head>
            <body>
                <App />
            </body>
        </html>
    }
}

#[component]
pub fn App() -> impl IntoView {
    // Provides context that manages stylesheets, titles, meta tags, etc.
    provide_meta_context();

    view! {
        // injects a stylesheet into the document <head>
        // id=leptos means cargo-leptos will hot-reload this stylesheet
        <Stylesheet id="leptos" href="/pkg/flwi-spacetraders-agent.css" />

        // sets the document title
        <Title text="Welcome to Leptos" />

        // content for this welcome page
        <Router>
            <main>
                <Routes fallback=|| "Page not found.".into_view()>
                    <Route path=StaticSegment("") view=HomePage />
                    <Route path=StaticSegment("supply-chain") view=SupplyChainPage />
                </Routes>
            </main>
        </Router>
    }
}

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

#[component]
fn SupplyChainPage() -> impl IntoView {
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

/// Renders the home page of your application.
#[component]
fn HomePage() -> impl IntoView {
    // Creates a reactive value to update the button
    let (value, set_value) = signal(0);

    // thanks to https://tailwindcomponents.com/component/blue-buttons-example for the showcase layout
    view! {
        <Title text="Leptos + Tailwindcss" />
        <main>
            <div class="bg-gradient-to-tl from-blue-800 to-blue-500 text-white font-mono flex flex-col min-h-screen">
                <div class="flex flex-row-reverse flex-wrap m-auto">
                    <button
                        on:click=move |_| set_value.update(|value| *value += 1)
                        class="rounded px-3 py-2 m-1 border-b-4 border-l-2 shadow-lg bg-blue-700 border-blue-800 text-white"
                    >
                        "+"
                    </button>
                    <button class="rounded px-3 py-2 m-1 border-b-4 border-l-2 shadow-lg bg-blue-800 border-blue-900 text-white">
                        {value}
                    </button>
                    <button
                        on:click=move |_| set_value.update(|value| *value -= 1)
                        class="rounded px-3 py-2 m-1 border-b-4 border-l-2 shadow-lg bg-blue-700 border-blue-800 text-white"
                        class:invisible=move || { value.get() < 1 }
                    >
                        "-"
                    </button>
                </div>
            </div>
        </main>
    }
}
