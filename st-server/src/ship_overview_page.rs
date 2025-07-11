use crate::components::treasury_overview::TreasuryOverview;
use crate::format_duration;
use chrono::{DateTime, Utc};
use itertools::*;
use leptos::html::*;
use leptos::prelude::*;
use leptos::{component, view, IntoView};
use phosphor_leptos::{Icon, ATOM, BINOCULARS, BRIEFCASE, CLOCK, COMPASS_ROSE, GAS_PUMP, HAMMER, HOURGLASS, MONEY_WAVY, PACKAGE, ROCKET, TRUCK};
use serde::{Deserialize, Serialize};
use st_domain::budgeting::treasury_redesign::{ActiveTrade, FinanceTicketDetails, FinanceTicketState, ImprovedTreasurer};
use st_domain::{Fleet, NavStatus, Ship, ShipSymbol, ShipTask, TradeGoodSymbol};
use std::collections::HashMap;
use thousands::Separable;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShipsOverview {
    grouped_ships: Vec<(Fleet, Vec<Ship>)>,
    ship_tasks: HashMap<ShipSymbol, ShipTask>,
    treasurer: ImprovedTreasurer,
    last_update: DateTime<Utc>,
    pub fleets: Vec<Fleet>,
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

    let treasurer = ImprovedTreasurer::from_ledger(ledger_entries).expect("treasurer");

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
        .iter()
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
        treasurer,
        fleets,
        last_update: Utc::now(),
    })
}

fn render_active_trade(trade: ActiveTrade) -> impl IntoView {
    /*
    **Trading 20 units of DRUGS for a profit of 33,160c**
    Purchase: X1-AM58-J61 (62,600c total for 3,130c/unit) ☑️
    Delivery: X1-AM58-H54 (95,760c total for 4,788c/unit)
         */

    let sell_price_per_unit = trade.delivery.details.get_price_per_unit();

    let (delivery_label, sell_total, trade_good_str) = match &trade.delivery.details {
        FinanceTicketDetails::PurchaseTradeGoods(d) => ("Purchase", 0.into(), d.trade_good.to_string()),
        FinanceTicketDetails::SellTradeGoods(s) => ("Sell", s.expected_total_sell_price, s.trade_good.to_string()),
        FinanceTicketDetails::SupplyConstructionSite(d) => ("Supply", 0.into(), d.trade_good.to_string()),
        FinanceTicketDetails::PurchaseShip(d) => ("PurchaseShip", 0.into(), d.ship_type.to_string()),
        FinanceTicketDetails::RefuelShip(_) => ("Refuel", 0.into(), TradeGoodSymbol::FUEL.to_string()),
        FinanceTicketDetails::DeliverContractCargo(d) => ("Deliver Contract Cargo", 0.into(), d.trade_good.to_string()),
    };

    let delivery_summary = format!(
        "{}: {} ({}c total for {}c/unit)",
        delivery_label,
        trade.delivery.details.get_waypoint(),
        sell_total.0.separate_with_commas(),
        sell_price_per_unit.0.separate_with_commas()
    );

    // complete trade (purchase & sell)
    if let Some((purchase_ticket, state)) = trade.maybe_purchase.clone() {
        let purchase_total = purchase_ticket.allocated_credits;
        let purchase_waypoint = purchase_ticket.details.get_waypoint();
        let purchase_price_per_unit = purchase_ticket.details.get_price_per_unit();
        let units = purchase_ticket.details.get_units();

        let profit = sell_total - purchase_total;
        let trade_summary = format!(
            "Trading {} units of {} for a profit of {}c",
            units,
            trade_good_str,
            profit.0.separate_with_commas()
        );

        let purchase_completed_checked_icon = if state == FinanceTicketState::Completed {
            " ☑️"
        } else {
            ""
        };
        let purchase_summary = format!(
            "Purchase: {} ({}c total for {}c/unit){}",
            purchase_waypoint,
            purchase_total.0.separate_with_commas(),
            purchase_price_per_unit.0.separate_with_commas(),
            purchase_completed_checked_icon
        );

        view! {
            <div class="flex flex-col gap-1">
                <p class="font-bold">{trade_summary}</p>
                <p class="">{purchase_summary}</p>
                <p class="">{delivery_summary}</p>
            </div>
        }
        .into_any()
    } else {
        // single trade action
        let trade_summary = trade.delivery.details.get_description();
        view! {
            <div class="flex flex-col gap-1">
                <p class="font-bold">{trade_summary}</p>
            </div>
        }
        .into_any()
    }
}

fn render_trades(active_trades: Vec<ActiveTrade>) -> impl IntoView {
    active_trades
        .iter()
        .map(|t| render_active_trade(t.clone()))
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

    let maybe_cooldown_expiration = ship.cooldown.expiration;

    let calc_cooldown_time_left = move || {
        maybe_cooldown_expiration.and_then(|expiration_time| {
            let now = Utc::now();
            let delta = expiration_time - now;

            (delta.num_seconds() >= 0).then_some(delta)
        })
    };

    #[allow(unused_variables)] // rustc gets confused, because the setter is only used in non-ssr mode
    let (maybe_travel_time_left, set_maybe_travel_time_left) = signal(calc_travel_time_left());

    #[allow(unused_variables)] // rustc gets confused, because the setter is only used in non-ssr mode
    let (maybe_cooldown_time_left, set_maybe_cooldown_time_left) = signal(calc_cooldown_time_left());

    let ship_icon = if let Some(ship_task) = &maybe_ship_task {
        match ship_task {
            ShipTask::ObserveWaypointDetails { .. } => COMPASS_ROSE,
            ShipTask::ObserveAllWaypointsOnce { .. } => COMPASS_ROSE,
            ShipTask::MineMaterialsAtWaypoint { .. } => HAMMER,
            ShipTask::SurveyMiningSite { .. } => BINOCULARS,
            ShipTask::HaulMiningGoods { .. } => TRUCK,
            ShipTask::Trade => MONEY_WAVY,
            ShipTask::PrepositionShipForTrade { .. } => TRUCK,
            ShipTask::SiphonCarboHydratesAtWaypoint { .. } => ATOM,
            ShipTask::ExecuteContracts => BRIEFCASE,
        }
    } else {
        ROCKET
    };

    #[cfg(not(feature = "ssr"))]
    let _handle = leptos_use::use_interval_fn(move || set_maybe_travel_time_left.set(calc_travel_time_left()), 1_000);

    #[cfg(not(feature = "ssr"))]
    let _handle2 = leptos_use::use_interval_fn(move || set_maybe_cooldown_time_left.set(calc_cooldown_time_left()), 1_000);

    view! {
        <div class="p-3 border-4 border-blue-900 text-slate-400">
            <div class="flex flex-col gap-2">
                <div class="flex flex-row gap-4 items-start">
                    <Icon icon=ship_icon size="3em" />
                    <div class="flex flex-col gap-1">
                        <h3 class="text-xl text-white">{ship.symbol.0.to_string()}</h3>
                        <p class="text-slate-400">
                            {maybe_ship_task
                                .map(|t| t.to_string())
                                .unwrap_or("---".to_string())}
                        </p>
                    </div>
                </div>
                <div class="text-slate-400">{move || render_trades(active_trades.clone())}</div>
            </div>
            <div class="pt-2 flex flex-col gap-1">
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
                <div class="flex flex-row items-center gap-2 mt-2">
                    <ul>
                        {ship
                            .cargo
                            .inventory
                            .iter()
                            .sorted_by_key(|i| i.units)
                            .rev()
                            .map(|inventory_entry| {
                                view! {
                                    <li>
                                        {format!(
                                            "{} {}",
                                            inventory_entry.units,
                                            inventory_entry.symbol,
                                        )}
                                    </li>
                                }
                            })
                            .collect_view()}
                    </ul>
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
    treasurer: &'a ImprovedTreasurer,
) -> impl IntoView {
    let ships_with_tasks = ships_of_fleet
        .iter()
        .map(|ship| (ship.clone(), ship_tasks.get(&ship.symbol)))
        .collect_vec();

    let active_trades = treasurer.compute_active_trades();

    let single_fleet_vec = vec![fleet.clone()];

    view! {
        <div class="flex flex-col gap-4 p-4">
            <h2 class="font-bold text-xl">
                {format!("Fleet {} with {} ships", fleet.cfg, ships_of_fleet.len())}
            </h2>
            <TreasuryOverview treasurer fleets=&single_fleet_vec />

            <div class="grid grid-cols-4 gap-4">
                {ships_with_tasks
                    .iter()
                    .sorted_by_key(|(ship, maybe_ship_task)| {
                        format!(
                            "{}-{}",
                            (*maybe_ship_task)
                                .map(|st| st.to_string())
                                .unwrap_or("None".to_string()),
                            ship.symbol.0.clone(),
                        )
                    })
                    .map(|(ship, maybe_ship_task)| {
                        let active_trades = active_trades
                            .get(&ship.symbol)
                            .cloned()
                            .unwrap_or_default();

                        view! {
                            <ShipCard
                                ship=ship
                                maybe_ship_task=*maybe_ship_task
                                active_trades
                            />
                        }
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
    let _handle = leptos_use::use_interval_fn(move || ships_resource.refetch(), 5_000);

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
                                        <TreasuryOverview
                                            treasurer=&ships_overview.treasurer
                                            fleets=&ships_overview.fleets
                                        />
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
                                                            treasurer=&ships_overview.treasurer
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
