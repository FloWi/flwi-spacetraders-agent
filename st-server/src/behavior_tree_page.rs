use crate::trading_opportunity_table::TradingOpportunityRow;
use itertools::Itertools;
use leptos::prelude::*;
use leptos_meta::Title;
use leptos_struct_table::*;
use serde::{Deserialize, Serialize};

use crate::components;
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone)]
pub struct MermaidString(String);

#[server]
async fn get_behavior_trees() -> Result<Vec<(String, MermaidString)>, ServerFnError> {
    use st_core;
    use st_core::behavior_tree::behavior_tree::Behavior;
    use st_core::behavior_tree::ship_behaviors::ShipAction;

    let behaviors = st_core::behavior_tree::ship_behaviors::ship_behaviors();
    let labelled_behaviors: HashMap<String, Behavior<ShipAction>> = behaviors.to_labelled_sub_behaviors();

    let mermaid_strings = labelled_behaviors.iter().map(|(label, behavior)| (label.clone(), MermaidString(behavior.to_mermaid()))).collect_vec();

    Ok(mermaid_strings)
}

#[server]
async fn get_trading_behavior() -> Result<Vec<(String, MermaidString)>, ServerFnError> {
    use st_core;
    use st_core::behavior_tree::behavior_tree::Behavior;
    use st_core::behavior_tree::ship_behaviors::ShipAction;

    let behavior = st_core::behavior_tree::ship_behaviors::ship_behaviors().trading_behavior;
    let all_behaviors = st_core::behavior_tree::ship_behaviors::ship_behaviors();
    let labelled_behaviors: HashMap<String, Behavior<ShipAction>> = all_behaviors.to_labelled_sub_behaviors();
    let sub_behavior_hashes = st_core::behavior_tree::behavior_tree::compute_sub_behavior_hashes(&labelled_behaviors);

    let mermaid_strings = vec![(
        "Trading Behavior".to_string(),
        MermaidString(behavior.to_mermaid_without_repeats(&sub_behavior_hashes)),
    )];

    let sub_behavior_mermaid_list = labelled_behaviors.iter().map(|(label, behavior)| (label.to_string(), MermaidString(behavior.to_mermaid()))).collect_vec();

    Ok(mermaid_strings.into_iter().chain(sub_behavior_mermaid_list.into_iter()).collect_vec())
}

#[component]
pub fn BehaviorTreePage() -> impl IntoView {
    // Use create_resource which is the standard way to handle async data in Leptos
    let explorer_behavior_resource = OnceResource::new(get_trading_behavior());
    let behavior_trees_resource = OnceResource::new(get_behavior_trees());

    view! {
        <Title text="Leptos + Tailwindcss" />
        <main>
            <div class="flex flex-col min-h-screen">
                <Suspense fallback=move || view! { <p>"Loading..."</p> }>
                    <ErrorBoundary fallback=|errors| {
                        view! { <p>"Error: " {format!("{errors:?}")}</p> }
                    }>
                        {move || {
                            explorer_behavior_resource
                                .get()
                                .map(|result| {
                                    match result {
                                        Ok((behavior_trees)) => {

                                            view! {
                                                <div class="flex flex-row gap-4">
                                                    <div class="w-full flex flex-col gap-4">
                                                        {render_mermaid_trees(behavior_trees)}
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

fn render_mermaid_trees(labelled_behaviors: Vec<(String, MermaidString)>) -> impl IntoView {
    use components::clipboard_button::*;

    view! {
        <div class="flex flex-col gap-4 w-full">
            {labelled_behaviors
                .iter()
                .cloned()
                .map(|(label, mermaid_string)| {
                    view! {
                        <div class="flex flex-col">
                            <h2 class="text-2xl font-bold">{label.to_string()}</h2>
                            <ClipboardButton
                                clipboard_text=mermaid_string.0.clone()
                                label="Copy to Clipboard".to_string()
                            />
                            <pre class="mermaid">{mermaid_string.0.clone()}</pre>
                        </div>
                    }
                })
                .collect_view()}
        </div>
    }
}
