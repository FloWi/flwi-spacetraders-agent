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
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <AutoReload options=options.clone() />
                <HydrationScripts options/>
                <MetaTags/>
            </head>
            <body>
                <App/>
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
        <Stylesheet id="leptos" href="/pkg/flwi-spacetraders-agent.css"/>

        // sets the document title
        <Title text="Welcome to Leptos"/>

        // content for this welcome page
        <Router>
            <main>
                <Routes fallback=|| "Page not found.".into_view()>
                    <Route path=StaticSegment("") view=HomePage/>
                    <Route path=StaticSegment("supply-chain") view=SupplyChainPage/>
                </Routes>
            </main>
        </Router>
    }
}

// Server function uses conversion
#[server]
async fn get_supply_chain() -> Result<String, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        use st_core;
        let supply_chain = st_core::supply_chain::read_supply_chain().await.unwrap();
        log!("supply-chain: {supply_chain:?}");

        Ok(format!(
            "{}",
            serde_json::to_string_pretty(&supply_chain).unwrap()
        ))
    }

    #[cfg(not(feature = "ssr"))]
    {
        Err(ServerFnError::new("Server-side function"))
    }
}

#[component]
fn SupplyChainPage() -> impl IntoView {
    // Use create_resource which is the standard way to handle async data in Leptos
    let supply_chain = Resource::new(
        || (),                  // No dependencies, just load once
        |_| get_supply_chain(), // Your server function
    );

    view! {
        <Title text="Leptos + Tailwindcss"/>
        <main>
            <div class="bg-gradient-to-tl from-blue-800 to-blue-500 text-white font-mono flex flex-col min-h-screen">
                <Suspense fallback=move || view! { <p>"Loading..."</p> }>
                    <ErrorBoundary fallback=|errors| view! { <p>"Error: " {format!("{errors:?}")}</p> }>
                        {move || {
                            supply_chain.get().map(|result| {
                                match result {
                                    Ok(data) => view! { <pre>{data}</pre> }.into_any(),
                                    Err(e) => view! { <p>"Error: " {e.to_string()}</p> }.into_any(),
                                }
                            })
                        }}
                    </ErrorBoundary>
                </Suspense>
            </div>
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
        <Title text="Leptos + Tailwindcss"/>
        <main>
            <div class="bg-gradient-to-tl from-blue-800 to-blue-500 text-white font-mono flex flex-col min-h-screen">
                <div class="flex flex-row-reverse flex-wrap m-auto">
                    <button on:click=move |_| set_value.update(|value| *value += 1) class="rounded px-3 py-2 m-1 border-b-4 border-l-2 shadow-lg bg-blue-700 border-blue-800 text-white">
                        "+"
                    </button>
                    <button class="rounded px-3 py-2 m-1 border-b-4 border-l-2 shadow-lg bg-blue-800 border-blue-900 text-white">
                        {value}
                    </button>
                    <button
                        on:click=move |_| set_value.update(|value| *value -= 1)
                        class="rounded px-3 py-2 m-1 border-b-4 border-l-2 shadow-lg bg-blue-700 border-blue-800 text-white"
                        class:invisible=move || {value.get() < 1}
                    >
                        "-"
                    </button>
                </div>
            </div>
        </main>
    }
}
