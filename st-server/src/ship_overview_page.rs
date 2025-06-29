use crate::format_duration;
use chrono::{DateTime, Utc};
use itertools::*;
use leptos::prelude::*;
use leptos::{component, view, IntoView};
use leptos_use::use_interval_fn;
use phosphor_leptos::{Icon, CLOCK, GAS_PUMP, PACKAGE, TRUCK};
use serde::{Deserialize, Serialize};
use st_domain::{NavStatus, Ship, ShipSymbol, ShipTask};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShipsOverview {
    ships: Vec<Ship>,
    ship_tasks: HashMap<ShipSymbol, ShipTask>,
    last_update: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum GetShipsMode {
    AllShips,
    OnlyChangesSince { filter_timestamp_gte: DateTime<Utc> },
}

#[server]
async fn get_ships_overview(get_ships_mode: GetShipsMode) -> Result<ShipsOverview, ServerFnError> {
    use st_store::Ctx;

    let state = expect_context::<crate::app::AppState>();
    let bmc = state.bmc;

    let filter_timestamp_gte = match get_ships_mode {
        GetShipsMode::AllShips => None,
        GetShipsMode::OnlyChangesSince { filter_timestamp_gte } => Some(filter_timestamp_gte),
    };

    let ships = bmc
        .ship_bmc()
        .get_ships(&Ctx::Anonymous, filter_timestamp_gte)
        .await
        .expect("get_ships");

    let ship_tasks = bmc
        .ship_bmc()
        .load_ship_tasks(&Ctx::Anonymous)
        .await
        .expect("load_ship_tasks");

    Ok(ShipsOverview {
        ships,
        ship_tasks,
        last_update: Utc::now(),
    })
}

#[component]
pub fn ShipCard<'a>(ship: &'a Ship, maybe_ship_task: Option<&'a ShipTask>) -> impl IntoView {
    let is_traveling = match ship.nav.status {
        NavStatus::InTransit => true,
        NavStatus::InOrbit => false,
        NavStatus::Docked => false,
    };

    let fuel_str = format!("{} / {}", ship.fuel.current, ship.fuel.capacity,);
    let cargo_str = format!("{} / {}", ship.cargo.units, ship.cargo.capacity,);

    let arrival_time = ship.nav.route.arrival;

    let calc_travel_time_left = move || {
        is_traveling
            .then(|| {
                let now = Utc::now();

                arrival_time - now
            })
            .and_then(|delta| (delta.num_seconds() >= 0).then_some(delta)) // ship nav status might not have been fixed after we've arrived
    };

    let (maybe_travel_time_left, set_maybe_travel_time_left) = signal(calc_travel_time_left());

    #[cfg(not(feature = "ssr"))]
    let _handle = use_interval_fn(move || set_maybe_travel_time_left.set(calc_travel_time_left()), 1_000);

    view! {
        <div class="p-3 border-4 border-blue-900 text-slate-400">
            <div class="flex flex-row gap-4 items-center">
                <Icon icon=TRUCK size="3em" />
                <div class="flex flex-col gap-1">
                    <h3 class="text-xl text-white">{ship.symbol.0.to_string()}</h3>
                    <p class="text-slate-400">
                        {maybe_ship_task
                            .clone()
                            .map(|t| t.to_string())
                            .unwrap_or("---".to_string())}
                    </p>
                </div>
            </div>
            <div class="flex flex-col gap-1">
                <div class="flex flex-row gap-2 items-center">
                    <Icon icon=TRUCK />
                    <p>{ship.nav.waypoint_symbol.0.to_string()}</p>
                    {move || {
                        maybe_travel_time_left
                            .get()
                            .map(|duration| {
                                view! {
                                    <>
                                        <Icon icon=CLOCK />
                                        <p>{format_duration(&duration)}</p>
                                    </>
                                }
                            })
                    }}
                // <pre>{serde_json::to_string_pretty(&ship.nav)}</pre>
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
    let ships_resource = Resource::new(|| {}, |_| get_ships_overview(GetShipsMode::AllShips));

    #[cfg(not(feature = "ssr"))]
    let _handle = use_interval_fn(move || ships_resource.refetch(), 5_000);

    view! {
        <div class="text-white flex flex-col min-h-screen">
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
                                                .sorted_by_key(|s| s.symbol.0.clone())
                                                .map(|ship| {
                                                    let maybe_ship_task = ships_overview
                                                        .ship_tasks
                                                        .get(&ship.symbol);
                                                    view! {
                                                        <ShipCard ship=ship maybe_ship_task=maybe_ship_task />
                                                    }
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
