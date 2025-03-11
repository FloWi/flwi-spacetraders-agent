use chrono::{DateTime, Utc};
use itertools::*;
use leptos::logging::log;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos::*;
use leptos::*;
use leptos::{component, view, IntoView};
use leptos_use::{use_interval, UseIntervalReturn};
use serde::{Deserialize, Serialize};
use st_domain::{FlightMode, Ship, ShipSymbol, StStatusResponse};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use leptos::attr::height;
use phosphor_leptos::{Icon, IconWeight, TRUCK, ROCKET, AIRPLANE_LANDING, AIRPLANE_TAKEOFF, GAS_PUMP, GAS_CAN, PACKAGE};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShipsOverview {
    ships: Vec<Ship>,
    last_update: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum GetShipsMode {
    AllShips,
    OnlyChangesSince { filter_timestamp_gte: DateTime<Utc> },
}

#[server]
async fn get_ships_overview(get_ships_mode: GetShipsMode) -> Result<ShipsOverview, ServerFnError> {
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

    Ok(ShipsOverview {
        ships,
        last_update: Utc::now(),
    })
}

#[component]
pub fn ShipCard<'a>(ship: &'a Ship) -> impl IntoView {

    let is_traveling = match ship.nav.flight_mode {
        FlightMode::Cruise => true,
        FlightMode::Drift => false,
        FlightMode::Stealth => false,
        FlightMode::Burn => false
    };

    let fuel_str = format!("{} / {}", ship.fuel.current, ship.fuel.capacity,);
    let cargo_str = format!("{} / {}", ship.cargo.units, ship.cargo.capacity,);

    view! {
        <div class="p-3 border-4 border-blue-900 text-slate-400">
            <div class="flex flex-row gap-4">
                <Icon icon=TRUCK size="3em" />
                <div class="flex flex-col gap-1">
                    <h3 class="text-xl text-white">{format!("{}", &ship.symbol.0)}</h3>
                    <p class="text-slate-400">"Role"</p>
                </div>
            </div>
            <div class="flex flex-col gap-1">
                <div class="flex flex-row gap-2">
                    <Icon icon=TRUCK />
                    <p>{format!("Location: {}", &ship.nav.waypoint_symbol.0)}</p>
                </div>
                <div class="flex flex-row items-center gap-2">
                    <div class="flex flex-row items-center gap-1">
                        <Icon icon=GAS_PUMP />
                        <p>{fuel_str}</p>
                    </div>
                    <div class="flex flex-row items-center gap-1">
                        <Icon icon=PACKAGE />
                        <p>{cargo_str}</p>
                    </div>

                </div>
            </div>
        </div>
    }
}

#[component]
pub fn ShipOverviewPage() -> impl IntoView {
    let UseIntervalReturn {
        counter,
        reset,
        is_active,
        pause,
        resume,
    } = use_interval(5000);

    let ships_resource = Resource::new(
        move || counter.get(),
        |count| get_ships_overview(GetShipsMode::AllShips),
    );

    view! {
        <div class="bg-blue-950 text-white flex flex-col min-h-screen">
            <h1 class="font-bold text-2xl">"Ships Status"</h1>
            <div>
                <Transition>
                    {move || {
                        match ships_resource.get() {
                            Some(Ok(ships_overview)) => {

                                view! {
                                    <div class="flex flex-col gap-4 p-4">
                                        <h2 class="font-bold text-xl">
                                            {format!("Fleet has {} ships", ships_overview.ships.len())}
                                        </h2>
                                        <p>
                                            {format!("Last Update: {:?}", ships_overview.last_update)}
                                        </p>
                                        <div class="flex flex-wrap gap-2">
                                            {ships_overview
                                                .ships
                                                .iter()
                                                .map(|ship| {
                                                    view! { <ShipCard ship=ship /> }
                                                })
                                                .collect_view()}
                                        </div>
                                    </div>
                                }
                                    .into_any()
                            }
                            _ => {

                                view! { <div>"No ships"</div> }
                                    .into_any()
                            }
                        }
                    }}
                </Transition>
            </div>
        </div>
    }
}
