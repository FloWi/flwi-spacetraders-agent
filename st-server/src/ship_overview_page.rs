use itertools::*;
use leptos::logging::log;
use leptos::prelude::*;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos::*;
use leptos::{component, view, IntoView};
use st_domain::agent_event::AgentEvent;
use st_domain::{Ship, ShipSymbol};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use tokio::sync::Mutex;

#[component]
pub fn ShipOverviewPage() -> impl IntoView {
    let agent_event_receiver = expect_context::<Arc<Mutex<Receiver<AgentEvent>>>>();

    let (ships, set_ships) = signal::<HashMap<ShipSymbol, Ship>>(HashMap::new());

    // Create an effect that polls for agent events

    Effect::new(move |_| {
        let agent_event_receiver = agent_event_receiver.clone();
        spawn_local(async move {
            let mut locked_receiver = agent_event_receiver.lock().await;

            while let Some(event) = locked_receiver.recv().await {
                match event {
                    AgentEvent::ShipUpdated { ship, change } => {
                        // Update the HashMap with the new ship data
                        set_ships.update(|ships_map| {
                            // Using clone to avoid ownership issues
                            // You might want to optimize this based on your specific needs
                            let ship_id = ship.symbol.clone(); // Assuming ShipOperations has an id field
                            ships_map.insert(ship_id, ship);
                        });

                        log!("Ship updated: {}", change);
                    } // Handle other event types if you add them later
                }
            }
        })
    });

    // Convert the HashMap to a Vector for easy rendering
    let ships_list = move || {
        let ships_map = ships.get();
        ships_map.values().cloned().into_iter().collect_vec()
    };

    view! {
        <div>
            <h2>"Ships Status"</h2>
            <div>
                {ships_list().into_iter().map(|ship| {
                    view! {
                        <div>
                            <h3>{format!("{}", &ship.symbol.0)}</h3>
                            <p>{format!("Location: {}", &ship.nav.waypoint_symbol.0)}</p>
                            /* Add more ship details as needed */
                        </div>
                    }
                }).collect_view()}
            </div>
        </div>
    }
}
