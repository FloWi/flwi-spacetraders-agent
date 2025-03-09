use chrono::{DateTime, Utc};
use itertools::*;
use leptos::logging::log;
use leptos::prelude::*;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos::*;
use leptos::{component, view, IntoView};
use serde::{Deserialize, Serialize};
use st_domain::{Ship, ShipSymbol, StStatusResponse};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShipsOverview {
    ships: Vec<Ship>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum GetShipsMode {
    AllShips,
    OnlyChangesSince { filter_timestamp_gte: DateTime<Utc> },
}

#[server]
async fn get_ships_overview(get_ships_mode: GetShipsMode) -> Result<Vec<Ship>, ServerFnError> {
    use st_store::{Ctx, ShipBmc};

    let state = expect_context::<crate::app::AppState>();
    let mm = state.db_model_manager;

    let filter_timestamp_gte = match get_ships_mode {
        GetShipsMode::AllShips => None,
        GetShipsMode::OnlyChangesSince {
            filter_timestamp_gte,
        } => Some(filter_timestamp_gte),
    };

    let ships = ShipBmc::get_ships(&Ctx::Anonymous, &mm, filter_timestamp_gte)
        .await
        .expect("get_ships");

    Ok(ships)
}

#[component]
pub fn ShipOverviewPage() -> impl IntoView {
    let (ships, set_ships) = signal::<HashMap<ShipSymbol, Ship>>(HashMap::new());
    let (last_update_ts, set_last_update_ts) = signal::<Option<DateTime<Utc>>>(None);

    // Convert the HashMap to a Vector for easy rendering
    let ships_list = move || {
        let ships_map = ships.get();
        ships_map.values().cloned().into_iter().collect_vec()
    };

    let get_ships_mode = move || {
        match last_update_ts.get() {
            None => { GetShipsMode::AllShips },
            Some(ts) => { GetShipsMode::OnlyChangesSince { filter_timestamp_gte: ts.clone() } }
    } };


    view! {
        <Await future=get_ships_overview(get_ships_mode()) let:data>
            {match data {
                Err(err) => view! { <p>"Error: " {err.to_string()}</p> }.into_any(),
                Ok(ships) => {
                    view! {
                        <div>
                            <h2>"Ships Status"</h2>
                            <div>
                                {ships
                                    .into_iter()
                                    .map(|ship| {
                                        view! {
                                            <div>
                                                <h3>{format!("{}", &ship.symbol.0)}</h3>
                                                <p>
                                                    {format!("Location: {}", &ship.nav.waypoint_symbol.0)}
                                                </p>

                                            </div>
                                        }
                                    })
                                    .collect_view()}
                            </div>
                        </div>
                    }
                        .into_any()
                }
            }}
        </Await>
    }
}
