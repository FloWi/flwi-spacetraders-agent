use crate::behavior_tree_page::BehaviorTreePage;
use crate::db_overview_page::*;
use crate::fleet_overview_page::*;
use crate::petgraph_example_page::TechTreePetgraph;
use crate::ship_overview_page::ShipOverviewPage;
use crate::supply_chain_page::*;
use leptos::prelude::*;
use leptos_meta::{provide_meta_context, MetaTags, Stylesheet, Title};
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
                    <Route path=StaticSegment("db-overview") view=DbOverviewPage />
                    <Route path=StaticSegment("ship-overview") view=ShipOverviewPage />
                    <Route path=StaticSegment("fleet-overview") view=FleetOverviewPage />
                    <Route path=StaticSegment("behavior-overview") view=BehaviorTreePage />
                    <Route path=StaticSegment("petgraph-example") view=TechTreePetgraph />
                </Routes>
            </main>
        </Router>
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

#[cfg(feature = "ssr")]
#[derive(Clone)]
pub struct AppState {
    pub db_model_manager: st_store::DbModelManager,
}
