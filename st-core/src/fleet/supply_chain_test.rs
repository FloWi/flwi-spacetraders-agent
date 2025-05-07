use anyhow::Result;
use comfy_table::presets::UTF8_FULL;
use comfy_table::*;
use comfy_table::{ContentArrangement, Table};
use itertools::Itertools;
use st_domain::trading::find_trading_opportunities_sorted_by_profit_per_distance_unit;
use st_domain::{
    ActivityLevel, DeliveryRoute, FleetDecisionFacts, FleetPhaseName, MarketTradeGood, MaterializedIndividualSupplyChain, MaterializedSupplyChain, ShipSymbol,
    SupplyLevel, TradeGoodSymbol, Waypoint, WaypointSymbol,
};
use std::collections::HashMap;
use std::ops::Not;
use strum::IntoEnumIterator;
use thousands::Separable;

#[cfg(test)]
mod tests {
    use crate::bmc_blackboard::BmcBlackboard;
    use crate::fleet::fleet::collect_fleet_decision_facts;
    use crate::fleet::fleet_runner::FleetRunner;
    use crate::fleet::supply_chain_test::{calc_trading_decisions, render_cli_table_trading_opp, TradingOppRow};
    use crate::st_client::StClientTrait;
    use crate::universe_server::universe_server::{InMemoryUniverse, InMemoryUniverseClient, InMemoryUniverseOverrides};
    use anyhow::Result;
    use itertools::Itertools;
    use st_domain::{trading, FleetPhaseName, MarketTradeGood, ShipSymbol, TradeGoodSymbol, Waypoint, WaypointSymbol};
    use st_store::bmc::jump_gate_bmc::InMemoryJumpGateBmc;
    use st_store::bmc::ship_bmc::{InMemoryShips, InMemoryShipsBmc};
    use st_store::bmc::{Bmc, InMemoryBmc};
    use st_store::shipyard_bmc::InMemoryShipyardBmc;
    use st_store::trade_bmc::InMemoryTradeBmc;
    use st_store::{
        Ctx, InMemoryAgentBmc, InMemoryConstructionBmc, InMemoryFleetBmc, InMemoryMarketBmc, InMemoryStatusBmc, InMemorySupplyChainBmc, InMemorySystemsBmc,
    };
    use std::collections::{HashMap, HashSet};
    use std::ops::Not;
    use std::sync::Arc;
    use test_log::test;

    #[test(tokio::test)]
    //#[tokio::test] // for accessing runtime-infos with tokio-console
    async fn test_supply_chain_materialization() -> Result<()> {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");

        let json_path = std::path::Path::new(manifest_dir).parent().unwrap().join("resources").join("universe_snapshot.json");

        let in_memory_universe = InMemoryUniverse::from_snapshot(json_path).expect("InMemoryUniverse::from_snapshot");

        let shipyard_waypoints = in_memory_universe.shipyards.keys().cloned().collect::<HashSet<_>>();
        let marketplace_waypoints = in_memory_universe.marketplaces.keys().cloned().collect::<HashSet<_>>();

        let in_memory_client = InMemoryUniverseClient::new(in_memory_universe);

        let agent = in_memory_client.get_agent().await.expect("agent").data;
        let hq_system_symbol = agent.headquarters.system_symbol();

        let ship_bmc = InMemoryShipsBmc::new(InMemoryShips::new());
        let agent_bmc = InMemoryAgentBmc::new(agent);
        let trade_bmc = InMemoryTradeBmc::new();
        let fleet_bmc = InMemoryFleetBmc::new();
        let system_bmc = InMemorySystemsBmc::new();
        let construction_bmc = InMemoryConstructionBmc::new();

        //insert some data
        //construction_bmc.save_construction_site(&Ctx::Anonymous, in_memory_client.get_construction_site().unwrap())

        let market_bmc = InMemoryMarketBmc::new();
        let shipyard_bmc = InMemoryShipyardBmc::new();
        let jump_gate_bmc = InMemoryJumpGateBmc::new();
        let supply_chain_bmc = InMemorySupplyChainBmc::new();
        let status_bmc = InMemoryStatusBmc::new();

        let trade_bmc = Arc::new(trade_bmc);
        let market_bmc = Arc::new(market_bmc);
        let bmc = InMemoryBmc {
            in_mem_ship_bmc: Arc::new(ship_bmc),
            in_mem_fleet_bmc: Arc::new(fleet_bmc),
            in_mem_trade_bmc: Arc::clone(&trade_bmc),
            in_mem_system_bmc: Arc::new(system_bmc),
            in_mem_agent_bmc: Arc::new(agent_bmc),
            in_mem_construction_bmc: Arc::new(construction_bmc),
            in_mem_market_bmc: Arc::clone(&market_bmc),
            in_mem_jump_gate_bmc: Arc::new(jump_gate_bmc),
            in_mem_shipyard_bmc: Arc::new(shipyard_bmc),
            in_mem_supply_chain_bmc: Arc::new(supply_chain_bmc),
            in_mem_status_bmc: Arc::new(status_bmc),
        };

        let client = Arc::new(in_memory_client) as Arc<dyn StClientTrait>;
        let bmc = Arc::new(bmc) as Arc<dyn Bmc>;
        let blackboard = BmcBlackboard::new(Arc::clone(&bmc));

        FleetRunner::load_and_store_initial_data_in_bmcs(Arc::clone(&client), Arc::clone(&bmc)).await.expect("FleetRunner::load_and_store_initial_data");

        // easier to get the supply chain this way, since we need plenty of things for computing it
        let facts = collect_fleet_decision_facts(bmc, &hq_system_symbol).await?;
        let materialized_supply_chain = facts.materialized_supply_chain.unwrap();

        assert!(
            materialized_supply_chain.raw_delivery_routes.is_empty(),
            "empty on first run, since we didn't have the exact market_data yet"
        );

        Ok(())
    }

    #[test(tokio::test)]
    //#[tokio::test] // for accessing runtime-infos with tokio-console
    async fn test_supply_chain_materialization_with_precise_marketdata() -> Result<()> {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");

        let json_path = std::path::Path::new(manifest_dir).parent().unwrap().join("resources").join("universe_snapshot.json");

        let in_memory_universe = InMemoryUniverse::from_snapshot(json_path).expect("InMemoryUniverse::from_snapshot");

        let shipyard_waypoints = in_memory_universe.shipyards.keys().cloned().collect::<HashSet<_>>();
        let marketplace_waypoints = in_memory_universe.marketplaces.keys().cloned().collect::<HashSet<_>>();

        let in_memory_client = InMemoryUniverseClient::new_with_overrides(
            in_memory_universe,
            InMemoryUniverseOverrides {
                always_respond_with_detailed_marketplace_data: true,
            },
        );

        let agent = in_memory_client.get_agent().await.expect("agent").data;
        let hq_system_symbol = agent.headquarters.system_symbol();

        let ship_bmc = InMemoryShipsBmc::new(InMemoryShips::new());
        let agent_bmc = InMemoryAgentBmc::new(agent);
        let trade_bmc = InMemoryTradeBmc::new();
        let fleet_bmc = InMemoryFleetBmc::new();
        let system_bmc = InMemorySystemsBmc::new();
        let construction_bmc = InMemoryConstructionBmc::new();

        //insert some data
        //construction_bmc.save_construction_site(&Ctx::Anonymous, in_memory_client.get_construction_site().unwrap())

        let market_bmc = InMemoryMarketBmc::new();
        let shipyard_bmc = InMemoryShipyardBmc::new();
        let jump_gate_bmc = InMemoryJumpGateBmc::new();
        let supply_chain_bmc = InMemorySupplyChainBmc::new();
        let status_bmc = InMemoryStatusBmc::new();

        let trade_bmc = Arc::new(trade_bmc);
        let market_bmc = Arc::new(market_bmc);
        let bmc = InMemoryBmc {
            in_mem_ship_bmc: Arc::new(ship_bmc),
            in_mem_fleet_bmc: Arc::new(fleet_bmc),
            in_mem_trade_bmc: Arc::clone(&trade_bmc),
            in_mem_system_bmc: Arc::new(system_bmc),
            in_mem_agent_bmc: Arc::new(agent_bmc),
            in_mem_construction_bmc: Arc::new(construction_bmc),
            in_mem_market_bmc: Arc::clone(&market_bmc),
            in_mem_jump_gate_bmc: Arc::new(jump_gate_bmc),
            in_mem_shipyard_bmc: Arc::new(shipyard_bmc),
            in_mem_supply_chain_bmc: Arc::new(supply_chain_bmc),
            in_mem_status_bmc: Arc::new(status_bmc),
        };

        let client = Arc::new(in_memory_client) as Arc<dyn StClientTrait>;
        let bmc = Arc::new(bmc) as Arc<dyn Bmc>;
        let blackboard = BmcBlackboard::new(Arc::clone(&bmc));

        // because of the override, we should have detailed market data
        FleetRunner::load_and_store_initial_data_in_bmcs(Arc::clone(&client), Arc::clone(&bmc)).await.expect("FleetRunner::load_and_store_initial_data");

        let waypoints_of_system = bmc.system_bmc().get_waypoints_of_system(&Ctx::Anonymous, &hq_system_symbol).await?;
        let waypoint_map: HashMap<WaypointSymbol, &Waypoint> = waypoints_of_system.iter().map(|wp| (wp.symbol.clone(), wp)).collect::<HashMap<_, _>>();

        let market_data = bmc.market_bmc().get_latest_market_data_for_system(&Ctx::Anonymous, &hq_system_symbol).await.expect("market_data");

        // easier to get the supply chain this way, since we need plenty of things for computing it
        let facts = collect_fleet_decision_facts(bmc.clone(), &hq_system_symbol).await?;

        let phase = FleetPhaseName::ConstructJumpGate;

        let active_trades: Vec<(ShipSymbol, (TradeGoodSymbol, WaypointSymbol), (TradeGoodSymbol, WaypointSymbol), u32)> = vec![];

        let supply_chain = bmc.supply_chain_bmc().get_supply_chain(&Ctx::Anonymous).await?;

        let materialized_supply_chain = facts.materialized_supply_chain.clone().unwrap();

        let market_data: Vec<(WaypointSymbol, Vec<MarketTradeGood>)> = trading::to_trade_goods_with_locations(&market_data);

        calc_trading_decisions(&facts, &phase, &active_trades, &vec![], &materialized_supply_chain, &market_data, &waypoint_map)?;

        assert!(
            materialized_supply_chain.raw_delivery_routes.is_empty().not(),
            "raw_delivery_routes should not be empty"
        );

        let top_10_products = materialized_supply_chain
            .trading_opportunities
            .iter()
            .map(|topp| topp.purchase_market_trade_good_entry.symbol.clone())
            .unique_by(|tgs| tgs.clone())
            .take(10)
            .collect::<HashSet<_>>();

        let trades_of_top_10_products = materialized_supply_chain
            .trading_opportunities
            .iter()
            .filter(|topp| top_10_products.contains(&topp.purchase_market_trade_good_entry.symbol))
            .sorted_by_key(|topp| -topp.profit_per_unit_per_distance)
            .map(|topp| TradingOppRow {
                purchase_market_trade_good_entry: topp.purchase_market_trade_good_entry.symbol.to_string(),
                purchase_waypoint_symbol: topp.purchase_waypoint_symbol.to_string(),
                sell_waypoint_symbol: topp.sell_waypoint_symbol.to_string(),
                direct_distance: topp.direct_distance,
                profit_per_unit: topp.profit_per_unit as u32,
                profit_per_unit_per_distance: (topp.profit_per_unit_per_distance.0 * 100.0).round() / 100.0,
            })
            .collect_vec();

        // Immutable approach - no table mutation needed

        println!("{}", render_cli_table_trading_opp(&trades_of_top_10_products));

        Ok(())
    }
}

struct TradingOppRow {
    purchase_market_trade_good_entry: String,
    purchase_waypoint_symbol: String,
    sell_waypoint_symbol: String,
    direct_distance: u32,
    profit_per_unit: u32,
    profit_per_unit_per_distance: f64,
}

fn render_cli_table_trading_opp(rows: &[TradingOppRow]) -> String {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        //.set_width(80)
        .set_header(vec![
            "purchase market trade good entry",
            "purchase waypoint symbol",
            "sell waypoint symbol",
            "direct distance",
            "profit per unit",
            "profit per unit per distance",
        ]);

    for row in rows.into_iter() {
        table.add_row(vec![
            row.purchase_market_trade_good_entry.as_str(),
            row.purchase_waypoint_symbol.as_str(),
            row.sell_waypoint_symbol.as_str(),
            row.direct_distance.separate_with_commas().as_str(),
            row.profit_per_unit.separate_with_commas().as_str(),
            format_number(row.profit_per_unit_per_distance).as_str(),
        ]);
    }

    for col_idx in 3..=5 {
        table.column_mut(col_idx).unwrap().set_cell_alignment(CellAlignment::Right);
    }

    table.to_string()
}

struct SupplyChainRouteLeg {
    from: WaypointSymbol,
    to: WaypointSymbol,
    rank: u32,
    purchase_price: u32,
    sell_price: u32,
    purchase_supply: SupplyLevel,
    sell_supply: SupplyLevel,
    purchase_activity: ActivityLevel,
    sell_activity: ActivityLevel,
    purchase_trade_volume: u32,
    sell_trade_volume: u32,
}

fn render_supply_chain_routes_table(chain: &MaterializedIndividualSupplyChain) -> String {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        //.set_width(80)
        .set_header(vec![
            "rank",
            "trade_good",
            "from",
            "to",
            "destination",
            "source type",
            "destination type",
            "purchase_price",
            "sell_price",
            "purchase_supply",
            "sell_supply",
            "purchase_activity",
            "sell_activity",
            "purchase_trade_volume",
            "sell_trade_volume",
        ]);

    let final_export_market_entry = chain.all_routes.iter().find_map(|route| match route {
        DeliveryRoute::Raw(_) => None,
        DeliveryRoute::Processed { route, rank } => {
            (route.producing_trade_good == chain.trade_good).then_some((route.delivery_location.clone(), route.producing_market_entry.clone()))
        }
    });

    chain
        .all_routes
        .iter()
        .sorted_by_key(|delivery_route| match delivery_route {
            DeliveryRoute::Raw(_) => 0,
            DeliveryRoute::Processed { rank, .. } => *rank,
        })
        .for_each(|route| {
            match &route {
                DeliveryRoute::Raw(raw) => {
                    table.add_row(vec![
                        Cell::new(0).fg(Color::Green),
                        Cell::new(raw.source.trade_good.to_string()),
                        Cell::new(raw.source.source_waypoint.to_string()).fg(Color::Green),
                        Cell::new(raw.delivery_location.to_string()).fg(Color::Green),
                        Cell::new(raw.export_entry.symbol.to_string()).fg(Color::Green),
                        Cell::new(raw.source.source_type.to_string()),
                        Cell::new(raw.delivery_market_entry.trade_good_type.to_string()),
                        Cell::new("---"),
                        Cell::new(raw.delivery_market_entry.sell_price),
                        Cell::new("---"),
                        Cell::new(raw.delivery_market_entry.supply.to_string()),
                        Cell::new("---"),
                        Cell::new(raw.delivery_market_entry.activity.clone().map(|act| act.to_string()).unwrap_or_default()),
                        Cell::new("---").fg(Color::Green),
                        Cell::new(raw.delivery_market_entry.trade_volume),
                    ]);
                }
                DeliveryRoute::Processed { route, rank } => {
                    table.add_row(vec![
                        Cell::new(rank).fg(Color::Green),
                        Cell::new(route.trade_good.to_string()),
                        Cell::new(route.source_location.to_string()),
                        Cell::new(route.delivery_location.to_string()),
                        Cell::new(route.producing_market_entry.symbol.to_string()),
                        Cell::new(route.source_market_entry.trade_good_type.to_string()),
                        Cell::new(route.delivery_market_entry.trade_good_type.to_string()),
                        Cell::new(route.source_market_entry.purchase_price),
                        Cell::new(route.delivery_market_entry.sell_price),
                        Cell::new(route.source_market_entry.supply.to_string()),
                        Cell::new(route.delivery_market_entry.supply.to_string()),
                        Cell::new(route.source_market_entry.activity.clone().map(|act| act.to_string()).unwrap_or_default()),
                        Cell::new(route.delivery_market_entry.activity.clone().map(|act| act.to_string()).unwrap_or_default()),
                        Cell::new(route.source_market_entry.trade_volume),
                        Cell::new(route.delivery_market_entry.trade_volume),
                    ]);
                }
            };
        });

    match final_export_market_entry {
        None => {}
        Some((wp, export_entry)) => {
            table.add_row(vec![
                Cell::new("---").fg(Color::Green),                                                       //rank
                Cell::new(export_entry.symbol.to_string()),                                              //trade_good
                Cell::new(wp.to_string()),                                                               //from
                Cell::new("---"),                                                                        //to
                Cell::new("Jump gate"),                                                                  //destination
                Cell::new(export_entry.trade_good_type.to_string()),                                     //source
                Cell::new("---".to_string()),                                                            //destination
                Cell::new(export_entry.purchase_price),                                                  //purchase_price
                Cell::new("---"),                                                                        //sell_price
                Cell::new(export_entry.supply.to_string()),                                              //purchase_supply
                Cell::new("---".to_string()),                                                            //sell_supply
                Cell::new(export_entry.activity.clone().map(|act| act.to_string()).unwrap_or_default()), //purchase_activity
                Cell::new("---"),                                                                        //sell_activity
                Cell::new(export_entry.trade_volume),                                                    //purchase_trade_volume
                Cell::new("---"),                                                                        //sell_trade_volume
            ]);
        }
    }

    table.to_string()
}

fn calc_trading_decisions(
    facts: &FleetDecisionFacts,
    phase: &FleetPhaseName,
    active_trades: &[(ShipSymbol, (TradeGoodSymbol, WaypointSymbol), (TradeGoodSymbol, WaypointSymbol), u32)],
    active_construction_deliveries: &[(ShipSymbol, (TradeGoodSymbol, u32))],
    materialized_supply_chain: &MaterializedSupplyChain,
    market_data: &[(WaypointSymbol, Vec<MarketTradeGood>)],
    waypoint_map: &HashMap<WaypointSymbol, &Waypoint>,
) -> Result<()> {
    let missing_construction_material: HashMap<TradeGoodSymbol, u32> = facts.missing_construction_materials();

    let missing_construction_material: HashMap<TradeGoodSymbol, u32> = missing_construction_material
        .into_iter()
        .map(|(good, amount)| {
            // Calculate how much of this good is already being delivered
            let en_route_amount = active_construction_deliveries
                .iter()
                .filter(|(_, (delivery_good, _))| delivery_good == &good)
                .map(|(_, (_, delivery_amount))| delivery_amount)
                .sum::<u32>();

            // Return the good and the remaining amount needed (if any)
            (good, amount.saturating_sub(en_route_amount))
        })
        // Filter out materials that are fully covered by en-route deliveries
        .filter(|(_, remaining_amount)| *remaining_amount > 0)
        .collect();

    //Check supply chain health of construction materials
    println!("Checking health of supply chain routes for construction material");
    missing_construction_material.keys().for_each(|missing_construction_mat| {
        if let Some(chain) = materialized_supply_chain.individual_materialized_routes.get(missing_construction_mat) {
            println!(
                "\nEvaluation of supply chain for {}\n{}",
                missing_construction_mat,
                render_supply_chain_routes_table(chain)
            )
        }
    });

    println!(
        "Found {} out of {} trade goods for sale that don't conflict with the supply chains of the construction materials:\nnon conflicting goods: {:?}\n    conflicting_goods: {:?}",
        materialized_supply_chain.goods_for_sale_not_conflicting_with_construction.len(),
        materialized_supply_chain.products_for_sale.len(),
        materialized_supply_chain.goods_for_sale_not_conflicting_with_construction,
        materialized_supply_chain.goods_for_sale_conflicting_with_construction,
    );

    let trading_opportunities = find_trading_opportunities_sorted_by_profit_per_distance_unit(market_data, waypoint_map);
    //evaluate_trading_opportunities()

    println!("found {} trading opportunities", trading_opportunities.len());

    Ok(())
}

/// Print a number with 2 decimal places and comma-separated
pub fn format_number(value: f64) -> String {
    // thousands will format floating point numbers just fine, but we can't
    // format the number this way _and_ specify the precision. So we're going
    // to separate out the fractional part and format that separately, but this
    // means we have to count the digits in the fractional part (up to 2).
    let fractional = ((value - value.floor()) * 100.0).round() as u64;
    let separated = (value.floor() as i64).separate_with_commas();

    // because we multiply the fractional component by only 100.0, we can only
    // ever have up to 2 digits.
    let num_digits = fractional.checked_ilog10().unwrap_or_default() + 1;
    match num_digits {
        1 => format!("{}.0{}", separated, fractional),
        _ => format!("{}.{}", separated, fractional),
    }
}
