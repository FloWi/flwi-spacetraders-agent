use crate::format_duration;
use chrono::{DateTime, Utc};
use itertools::*;
use leptos::prelude::*;
use leptos::{component, view, IntoView};
use leptos_use::use_interval_fn;
use phosphor_leptos::{Icon, ATOM, BINOCULARS, CLOCK, COMPASS_ROSE, GAS_PUMP, HAMMER, HOURGLASS, PACKAGE, ROCKET, TRUCK};
use serde::{Deserialize, Serialize};
use st_domain::budgeting::treasury_redesign::{ActiveTrade, FinanceTicket, FinanceTicketDetails, FinanceTicketState, ImprovedTreasurer, LedgerEntry};
use st_domain::{Fleet, NavStatus, Ship, ShipSymbol, ShipTask, TicketId};
use std::collections::{HashMap, HashSet};
use std::ops::Not;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShipsOverview {
    grouped_ships: Vec<(Fleet, Vec<Ship>)>,
    ship_tasks: HashMap<ShipSymbol, ShipTask>,
    active_trades: HashMap<ShipSymbol, Vec<ActiveTrade>>,
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

    let ledger_entries = bmc
        .ledger_bmc()
        .get_ledger_entries_in_order(&Ctx::Anonymous)
        .await
        .expect("get_ledger_entries");
    let active_trades: HashMap<ShipSymbol, Vec<ActiveTrade>> = get_active_trades_from_ledger_entries(ledger_entries);

    let fleets = bmc
        .fleet_bmc()
        .load_fleets(&Ctx::Anonymous)
        .await
        .expect("load_fleets");
    let ship_fleet_assignment = bmc
        .fleet_bmc()
        .load_ship_fleet_assignment(&Ctx::Anonymous)
        .await
        .expect("load_ship_fleet_assignment");

    let ship_map = ships
        .into_iter()
        .map(|ship| (ship.symbol.clone(), ship.clone()))
        .collect::<HashMap<_, _>>();

    let grouped_ships: Vec<(Fleet, Vec<Ship>)> = fleets
        .into_iter()
        .sorted_by_key(|f| f.id.0)
        .map(|f| {
            (
                f.clone(),
                ship_fleet_assignment
                    .iter()
                    .filter_map(|(ss, fleet_id)| {
                        if fleet_id == &f.id {
                            ship_map.get(&ss).cloned()
                        } else {
                            None
                        }
                    })
                    .collect_vec(),
            )
        })
        .collect_vec();

    Ok(ShipsOverview {
        grouped_ships,
        ship_tasks,
        active_trades,
        last_update: Utc::now(),
    })
}

fn get_active_trades_from_ledger_entries(ledger_entries: Vec<LedgerEntry>) -> HashMap<ShipSymbol, Vec<ActiveTrade>> {
    let mut active_tickets: HashMap<TicketId, FinanceTicket> = HashMap::new();
    let mut completed_tickets: HashMap<TicketId, FinanceTicket> = HashMap::new();
    for entry in ledger_entries.iter().rev() {
        match entry {
            LedgerEntry::TicketCreated { ticket_details, .. } => {
                if completed_tickets
                    .contains_key(&ticket_details.ticket_id)
                    .not()
                {
                    active_tickets.insert(ticket_details.ticket_id.clone(), ticket_details.clone());
                }
            }
            LedgerEntry::TicketCompleted {
                fleet_id,
                finance_ticket,
                actual_units,
                actual_price_per_unit,
                total,
            } => {
                completed_tickets.insert(finance_ticket.ticket_id.clone(), finance_ticket.clone());
                active_tickets.remove(&finance_ticket.ticket_id);
            }
            _ => {}
        }
    }

    let get_ticket_with_state = |id: &TicketId| {
        active_tickets
            .get(id)
            .map(|ticket| (ticket.clone(), FinanceTicketState::Open))
            .or_else(|| {
                completed_tickets
                    .get(&id)
                    .map(|ticket| (ticket.clone(), FinanceTicketState::Completed))
            })
    };

    let mut active_trades: HashMap<ShipSymbol, Vec<ActiveTrade>> = HashMap::new();

    for ticket in active_tickets.values() {
        let maybe_matching_purchase_ticket = match &ticket.details {
            FinanceTicketDetails::SupplyConstructionSite(d) => d
                .maybe_matching_purchase_ticket
                .and_then(|purchase_ticket_id| get_ticket_with_state(&purchase_ticket_id)),
            FinanceTicketDetails::SellTradeGoods(d) => d
                .maybe_matching_purchase_ticket
                .and_then(|purchase_ticket_id| get_ticket_with_state(&purchase_ticket_id)),
            FinanceTicketDetails::PurchaseTradeGoods(_) => None,
            FinanceTicketDetails::PurchaseShip(_) => None,
            FinanceTicketDetails::RefuelShip(_) => None,
        };

        active_trades
            .entry(ticket.ship_symbol.clone())
            .or_default()
            .push(ActiveTrade {
                maybe_purchase: maybe_matching_purchase_ticket,
                delivery: ticket.clone(),
            });
    }

    active_trades
}

fn render_trades(active_trades: Vec<ActiveTrade>) -> impl IntoView {
    active_trades
        .iter()
        .map(|t| {
            if let Some((purchase_ticket, state)) = t.maybe_purchase.clone() {
                view! {
                    <ul>
                        <li>
                            {format!("{} ({state})", purchase_ticket.details.get_description())}
                        </li>
                        <li>{format!("{}", t.delivery.details.get_description())}</li>
                    </ul>
                }
                .into_any()
            } else {
                view! {
                    <ul>
                        <li>{format!("{}", t.delivery.details.get_description())}</li>
                    </ul>
                }
                .into_any()
            }
        })
        .collect_view()
}

#[component]
pub fn ShipCard<'a>(ship: &'a Ship, maybe_ship_task: Option<&'a ShipTask>, active_trades: Vec<ActiveTrade>) -> impl IntoView {
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

    let maybe_cooldown_expiration = ship.cooldown.expiration.clone();

    let calc_cooldown_time_left = move || {
        maybe_cooldown_expiration.and_then(|expiration_time| {
            let now = Utc::now();
            let delta = expiration_time - now;

            (delta.num_seconds() >= 0).then_some(delta)
        })
    };

    let (maybe_travel_time_left, set_maybe_travel_time_left) = signal(calc_travel_time_left());
    let (maybe_cooldown_time_left, set_maybe_cooldown_time_left) = signal(calc_cooldown_time_left());

    let ship_icon = if let Some(ship_task) = &maybe_ship_task {
        let icon = match ship_task {
            ShipTask::ObserveWaypointDetails { .. } => COMPASS_ROSE,
            ShipTask::ObserveAllWaypointsOnce { .. } => COMPASS_ROSE,
            ShipTask::MineMaterialsAtWaypoint { .. } => HAMMER,
            ShipTask::SurveyMiningSite { .. } => BINOCULARS,
            ShipTask::HaulMiningGoods { .. } => TRUCK,
            ShipTask::Trade => phosphor_leptos::MONEY_WAVY,
            ShipTask::PrepositionShipForTrade { .. } => TRUCK,
            ShipTask::SiphonCarboHydratesAtWaypoint { .. } => ATOM,
        };
        icon
    } else {
        ROCKET
    };

    #[cfg(not(feature = "ssr"))]
    let _handle = use_interval_fn(move || set_maybe_travel_time_left.set(calc_travel_time_left()), 1_000);
    #[cfg(not(feature = "ssr"))]
    let _handle2 = use_interval_fn(move || set_maybe_cooldown_time_left.set(calc_cooldown_time_left()), 1_000);

    view! {
        <div class="p-3 border-4 border-blue-900 text-slate-400">
            <div class="flex flex-row gap-4 items-center">
                <Icon icon=ship_icon size="3em" />
                <div class="flex flex-col gap-1">
                    <h3 class="text-xl text-white">{ship.symbol.0.to_string()}</h3>
                    <p class="text-slate-400">
                        {maybe_ship_task
                            .clone()
                            .map(|t| t.to_string())
                            .unwrap_or("---".to_string())}
                    </p>
                    <div class="text-slate-400">{move || render_trades(active_trades.clone())}</div>
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
                    {move || {
                        maybe_cooldown_time_left
                            .get()
                            .map(|duration| {
                                view! {
                                    <>
                                        <Icon icon=HOURGLASS />
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
pub fn FleetOverview<'a>(
    fleet: &'a Fleet,
    ships_of_fleet: &'a [Ship],
    ship_tasks: &'a HashMap<ShipSymbol, ShipTask>,
    active_trades: &'a HashMap<ShipSymbol, Vec<ActiveTrade>>,
) -> impl IntoView {
    view! {
        <div class="flex flex-col gap-4 p-4">
            <h2 class="font-bold text-xl">
                {format!("Fleet {} with {} ships", fleet.cfg.to_string(), ships_of_fleet.len())}
            </h2>
        <div class="grid grid-cols-4 gap-4">
            {ships_of_fleet
                .iter()
                .sorted_by_key(|s| s.symbol.0.clone())
                .map(|ship| {
                    let maybe_ship_task = ship_tasks.get(&ship.symbol);
                    let active_trades = active_trades
                        .get(&ship.symbol)
                        .cloned()
                        .unwrap_or_default();
                    view! { <ShipCard ship=ship maybe_ship_task=maybe_ship_task active_trades /> }
                })
                .collect_view()}
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
                                        <p>
                                            {format!("Last Update: {:?}", ships_overview.last_update)}
                                        </p>
                                        <div class="flex flex-col">
                                            {ships_overview
                                                .grouped_ships
                                                .iter()
                                                .map(|(fleet, ships_of_fleet)| {

                                                    view! {
                                                        <FleetOverview
                                                            fleet
                                                            ships_of_fleet
                                                            ship_tasks=&ships_overview.ship_tasks
                                                            active_trades=&ships_overview.active_trades
                                                        />
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
