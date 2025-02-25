use anyhow::Result;
use chrono::{DateTime, Utc};
use flwi_spacetraders_agent::st_model::{
    ActivityLevel, LabelledCoordinate, SupplyLevel, SystemSymbol, TradeGood, TradeGoodSymbol,
    TradeGoodType, WaypointSymbol, WaypointType,
};
pub use flwi_spacetraders_agent::supply_chain::*;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use strum_macros::Display;

#[tokio::main]
async fn main() -> Result<()> {
    let supply_chain = read_supply_chain().await?;

    //println!("Complete Supply Chain");
    //dbg!(supply_chain.clone());

    let trade_map = supply_chain.trade_map();

    let goods_of_interest = [
        TradeGoodSymbol::ADVANCED_CIRCUITRY,
        TradeGoodSymbol::FAB_MATS,
        TradeGoodSymbol::SHIP_PLATING,
        TradeGoodSymbol::MICROPROCESSORS,
        TradeGoodSymbol::CLOTHING,
    ];
    for trade_good in goods_of_interest.clone() {
        let chain = find_complete_supply_chain(Vec::from([trade_good.clone()]), &trade_map);
        println!("\n\n## {} Supply Chain", trade_good);
        println!("{}", chain.to_mermaid());
    }

    let complete_chain = find_complete_supply_chain(Vec::from(&goods_of_interest), &trade_map);
    println!("\n\n## Complete Supply Chain");
    println!("{}", complete_chain.to_mermaid());

    let market_entries = get_market_entries()?;
    let waypoints = get_waypoint_entries()?;

    let ranked = rank_supply_chain(&complete_chain);

    let ordered: Vec<(TradeGoodSymbol, u32)> =
        ranked.into_iter().sorted_by_key(|kv| kv.1).collect();

    println!(
        "ranked supply chain sorted: \n```text\n{}\n```",
        &ordered
            .into_iter()
            .map(|(tg, rank)| format!("#{}: {}", rank, tg.to_string()))
            .join("\n")
    );

    let materialized = materialize_supply_chain(&complete_chain, &market_entries, &waypoints);

    Ok(())
}

fn get_raw_material_source_map() -> HashMap<TradeGoodSymbol, RawMaterialSourceType> {
    use RawMaterialSourceType::*;
    use TradeGoodSymbol::*;

    HashMap::from([
        (AMMONIA_ICE, Extraction),
        (IRON_ORE, Extraction),
        (COPPER_ORE, Extraction),
        (SILICON_CRYSTALS, Extraction),
        (QUARTZ_SAND, Extraction),
        (ALUMINUM_ORE, Extraction),
        (LIQUID_NITROGEN, Siphoning),
        (LIQUID_HYDROGEN, Siphoning),
        (HYDROCARBON, Siphoning),
    ])
}

pub fn materialize_supply_chain(
    complete_chain: &Vec<SupplyChainNode>,
    market_entries: &Vec<MarketEntry>,
    waypoints: &Vec<Waypoint>,
) -> (Vec<TradeGoodRoute>, ApiSupplyChain) {
    let ranked = rank_supply_chain(&complete_chain);

    let raw_materials: Vec<TradeGoodSymbol> = ranked
        .clone()
        .into_iter()
        .filter(|kv| kv.1 == 0)
        .map(|kv| kv.0)
        .collect();

    let raw_materials_source_types = get_raw_material_source_map();

    let engineered_asteroid = waypoints
        .iter()
        .find(|wp| wp.r#type == WaypointType::ENGINEERED_ASTEROID)
        .unwrap();

    let gas_giant = waypoints
        .iter()
        .find(|wp| wp.r#type == WaypointType::GAS_GIANT)
        .unwrap();

    let raw_material_extract_locations: Vec<(TradeGoodSymbol, WaypointSymbol)> =
        raw_materials_source_types
            .clone()
            .into_iter()
            .map(|(tgs, source_type)| {
                let waypoint = match source_type {
                    RawMaterialSourceType::Extraction => engineered_asteroid,
                    RawMaterialSourceType::Siphoning => gas_giant,
                };
                (tgs, waypoint.symbol.clone())
            })
            .collect();

    // now we decide, where to ship the raw materials. Either
    // directly to the market that depends on it or
    // an intermediate exchange market, that is close by

    let exchange_markets_of_raw_materials: HashMap<
        &TradeGoodSymbol,
        Vec<(&MarketEntry, WaypointSymbol)>,
    > = raw_materials
        .iter()
        .map(|tgs| {
            let waypoints_of_exchange_marketplaces: Vec<(&MarketEntry, WaypointSymbol)> =
                market_entries
                    .into_iter()
                    .filter(|me| {
                        me.trade_good_symbol == *tgs
                            && me.trade_good_type == TradeGoodType::Exchange
                    })
                    .filter_map(|me| {
                        waypoints
                            .iter()
                            .find(|wp| wp.symbol == me.waypoint_symbol)
                            .map(|wp| (me, wp.symbol.clone()))
                    })
                    .collect();

            (tgs, waypoints_of_exchange_marketplaces)
        })
        .collect();

    // we find the marketplaces that import a raw material
    let markets_requiring_raw_materials: HashMap<
        &TradeGoodSymbol,
        Vec<(&MarketEntry, WaypointSymbol)>,
    > = complete_chain
        .into_iter()
        .flat_map(|scn| {
            scn.dependencies
                .iter()
                .filter(|import_dep| raw_materials.iter().contains(import_dep))
                .flat_map(|import_tgs| {
                    let export_markets_that_import_raw_material: Vec<(
                        &MarketEntry,
                        WaypointSymbol,
                    )> = market_entries
                        .into_iter()
                        .filter(|me| {
                            me.trade_good_symbol == scn.good
                                && me.trade_good_type == TradeGoodType::Export
                        })
                        .filter_map(|me| {
                            waypoints
                                .iter()
                                .find(|wp| wp.symbol == me.waypoint_symbol)
                                .map(|wp| (me, wp.symbol.clone()))
                        })
                        .collect();
                    export_markets_that_import_raw_material
                        .into_iter()
                        .map(move |(me, wps)| (import_tgs, (me, wps)))
                })
        })
        .into_group_map();

    let waypoints_importing_raw_materials: HashMap<
        &TradeGoodSymbol,
        Vec<(&MarketEntry, WaypointSymbol)>,
    > = merge_hashmaps(
        &exchange_markets_of_raw_materials,
        &markets_requiring_raw_materials,
    );

    let routes_of_raw_materials: Vec<TradeGoodRoute> = waypoints_importing_raw_materials
        .clone()
        .into_iter()
        .flat_map(|(tgs, delivery_destinations)| {
            let source_wps = raw_material_extract_locations
                .iter()
                .find(|(raw_tgs, extract_wp)| raw_tgs == tgs)
                .map(|kv| kv.1.clone())
                .unwrap();

            let source_wp = waypoints.iter().find(|wp| wp.symbol == source_wps).unwrap();

            let delivery_destination_wps_and_distances =
                delivery_destinations.iter().map(|(me, delivery_wps)| {
                    let delivery_wp = waypoints
                        .iter()
                        .find(|wp| wp.symbol == *delivery_wps)
                        .unwrap();
                    (delivery_wp, *me, source_wp.distance_to(delivery_wp))
                });

            let export_markets_to_supply: Vec<(&Waypoint, &MarketEntry, u32)> =
                delivery_destination_wps_and_distances
                    .clone()
                    .filter(|(wp, me, distance)| me.trade_good_type == TradeGoodType::Export)
                    .sorted_by_key(|kv| kv.2)
                    .collect_vec();

            let exchange_markets: Vec<(&Waypoint, &MarketEntry, u32)> =
                delivery_destination_wps_and_distances
                    .clone()
                    .filter(|(wp, me, distance)| me.trade_good_type == TradeGoodType::Exchange)
                    .sorted_by_key(|kv| kv.2)
                    .collect_vec();

            let closest_one: (&Waypoint, &MarketEntry, u32) =
                delivery_destination_wps_and_distances
                    .min_by_key(|kv| kv.2)
                    .unwrap();

            let best_one: (&Waypoint, &MarketEntry, u32) =
                if closest_one.1.trade_good_type == TradeGoodType::Exchange {
                    Some(closest_one)
                } else if export_markets_to_supply.len() == 1 {
                    export_markets_to_supply.clone().get(0).cloned()
                } else if export_markets_to_supply.len() > 1 && !exchange_markets.is_empty() {
                    Some(
                        exchange_markets
                            .iter()
                            .min_by_key(|kv| kv.2)
                            .unwrap()
                            .clone(),
                    )
                } else {
                    None
                }
                .unwrap();

            let delivery_market_entry = if best_one.1.trade_good_type == TradeGoodType::Exchange {
                best_one.1
            } else {
                assert_eq!(best_one.1.trade_good_type, TradeGoodType::Export);
                // we are delivering directly to an export market. For displaying the "health" of the supply-chain we need the corresponding import-entry at that marketplace
                market_entries
                    .iter()
                    .find(|me| {
                        me.trade_good_type == TradeGoodType::Import
                            && me.waypoint_symbol == best_one.1.waypoint_symbol
                            && me.trade_good_symbol == tgs.clone()
                    })
                    .unwrap()
            };

            let route = TradeGoodRoute {
                trade_good_symbol: tgs.clone(),
                source_waypoint: source_wp.clone(),
                maybe_source_market_entry: None,
                delivery_waypoint: best_one.0.clone(),
                delivery_market_entry: delivery_market_entry.clone(),
                destination_market_entry: best_one.1.clone(),
                maybe_raw_material_source_type: raw_materials_source_types
                    .get(&tgs.clone())
                    .cloned(),
                level: 0,
                distance: best_one.2,
            };

            let additional_routes = if route.delivery_market_entry.trade_good_type
                == TradeGoodType::Exchange
            {
                // if we deliver to an exchange market in between, we need to add a route to each of the export_markets that require this trade_good
                export_markets_to_supply
                    .into_iter()
                    .map(|(export_wp, export_me, _distance)| {
                        // we are now delivering to an export market. For displaying the "health" of the supply-chain we need the corresponding import-entry at that marketplace
                        let import_entry_at_export_market = market_entries
                            .iter()
                            .find(|me| {
                                me.trade_good_type == TradeGoodType::Import
                                    && me.waypoint_symbol == export_me.waypoint_symbol
                                    && me.trade_good_symbol == tgs.clone()
                            })
                            .unwrap();

                        let result = TradeGoodRoute {
                            trade_good_symbol: tgs.clone(),
                            source_waypoint: best_one.0.clone(),
                            maybe_source_market_entry: Some(route.destination_market_entry.clone()),
                            delivery_waypoint: export_wp.clone(),
                            delivery_market_entry: import_entry_at_export_market.clone(),
                            destination_market_entry: export_me.clone(),
                            maybe_raw_material_source_type: None,
                            level: 1,
                            distance: best_one.0.distance_to(export_wp),
                        };

                        match &result.maybe_source_market_entry {
                            None => {}
                            Some(me) => {
                                if me.trade_good_type == TradeGoodType::Import {
                                    // dbg!(&result);
                                    // dbg!(best_one);
                                    println!("found broken entry - source market entry is IMPORT");
                                }
                            }
                        }

                        result
                    })
                    .collect()
            } else {
                vec![]
            };

            vec![route]
                .into_iter()
                .chain(additional_routes.into_iter())
                .collect::<Vec<_>>()
        })
        .collect();

    let all_routes = recurse_collect_rest_routes(
        routes_of_raw_materials.clone(),
        &complete_chain,
        market_entries,
        waypoints,
    );

    let api_supply_chain = routes_to_api_supply_chain(all_routes.as_slice());

    // println!(
    //     "\n\n## raw_material_extract_locations: \n\n```json\n\n{}\n ```",
    //     serde_json::to_string_pretty(&raw_material_extract_locations).unwrap()
    // );
    //
    // println!(
    //     "\n\n## exchange_markets_of_raw_materials: \n\n```json\n\n{}\n ```",
    //     serde_json::to_string_pretty(&exchange_markets_of_raw_materials).unwrap()
    // );
    //
    // println!(
    //     "\n\n## markets_requiring_raw_materials: \n\n```json\n\n{}\n ```",
    //     serde_json::to_string_pretty(&markets_requiring_raw_materials).unwrap()
    // );
    //
    // println!(
    //     "\n\n## waypoints_importing_raw_materials: \n\n```json\n\n{}\n ```",
    //     serde_json::to_string_pretty(&waypoints_importing_raw_materials).unwrap()
    // );
    //
    // println!(
    //     "\n\n## routes_of_raw_materials: \n\n```json\n\n{}\n ```",
    //     serde_json::to_string_pretty(&routes_of_raw_materials).unwrap()
    // );

    // println!(
    //     "\n\n## all_routes: \n\n```json\n\n{}\n ```",
    //     serde_json::to_string_pretty(&all_routes).unwrap()
    // );
    //
    // println!(
    //     "\n\n## api_supply_chain: \n\n```json\n\n{}\n ```",
    //     serde_json::to_string_pretty(&api_supply_chain).unwrap()
    // );
    // we ship

    // println!(
    //     "materialize_supply_chain called\n\n\ncomplete_chain: {}\n\n\nmarket_entries: {}\n\n\nwaypoints: {}",
    //     serde_json::to_string(complete_chain).unwrap(),
    //     serde_json::to_string(market_entries).unwrap(),
    //     serde_json::to_string(waypoints).unwrap()
    // );

    (all_routes, api_supply_chain)
}

fn merge_hashmaps<'a>(
    map1: &'a HashMap<&'a TradeGoodSymbol, Vec<(&'a MarketEntry, WaypointSymbol)>>,
    map2: &'a HashMap<&'a TradeGoodSymbol, Vec<(&'a MarketEntry, WaypointSymbol)>>,
) -> HashMap<&'a TradeGoodSymbol, Vec<(&'a MarketEntry, WaypointSymbol)>> {
    map1.iter()
        .chain(map2.iter())
        .into_group_map_by(|(&k, _)| k)
        .into_iter()
        .map(|(k, v)| {
            (
                k,
                v.into_iter()
                    .flat_map(|(_, vec)| vec.iter().cloned())
                    .collect(),
            )
        })
        .collect()
}

fn recurse_collect_rest_routes(
    current_routes: Vec<TradeGoodRoute>,
    complete_chain: &Vec<SupplyChainNode>,
    market_entries: &Vec<MarketEntry>,
    waypoints: &Vec<Waypoint>,
) -> Vec<TradeGoodRoute> {
    let ranked = rank_supply_chain(&complete_chain);

    // which routes need to exist (tuples of materials)

    let offerings: Vec<(TradeGoodSymbol, WaypointSymbol, Option<MarketEntry>)> = current_routes
        .iter()
        .map(|tgr| {
            (
                tgr.trade_good_symbol.clone(),
                tgr.source_waypoint.symbol.clone(),
                tgr.maybe_source_market_entry.clone(),
            )
        })
        .chain(current_routes.iter().map(|tgr| {
            (
                tgr.destination_market_entry.trade_good_symbol.clone(),
                tgr.destination_market_entry.waypoint_symbol.clone(),
                Some(tgr.destination_market_entry.clone()),
            )
        }))
        .collect_vec();

    let all_import_export_pairs = complete_chain
        .iter()
        .flat_map(|scn| {
            scn.dependencies
                .iter()
                .map(|import_dep| (import_dep.clone(), scn.good.clone()))
        })
        .collect_vec();

    let already_fulfilled_pairs = current_routes
        .iter()
        .map(|tgr| {
            (
                tgr.trade_good_symbol.clone(),
                tgr.destination_market_entry.trade_good_symbol.clone(),
            )
        })
        .collect_vec();

    let open_pairs: Vec<&(TradeGoodSymbol, TradeGoodSymbol)> = all_import_export_pairs
        .iter()
        .filter(|pair| !already_fulfilled_pairs.contains(pair))
        .collect();

    // ending recursion - we're done
    if open_pairs.is_empty() {
        return current_routes;
    }

    let new_routes = open_pairs
        .iter()
        .filter_map(|(import_good, export_good)| {
            let receiving_market = market_entries
                .iter()
                .find(|me| {
                    me.trade_good_symbol == *export_good
                        && me.trade_good_type == TradeGoodType::Export
                })
                .unwrap();

            let import_market_at_receiving_market = market_entries
                .iter()
                .find(|me| {
                    me.trade_good_symbol == *import_good
                        && me.waypoint_symbol == receiving_market.waypoint_symbol
                        && me.trade_good_type == TradeGoodType::Import
                })
                .unwrap();

            let supplying_markets = offerings
                .iter()
                .filter(|(offered_good, pickup_waypoint, maybe_market_entry)| {
                    offered_good == import_good
                })
                .collect_vec();

            if supplying_markets.len() > 1 {
                println!(
                    "found multiple supplying markets for import/export pair {}-{}: {:?}",
                    &import_good, &export_good, &supplying_markets
                );
            }

            let result = supplying_markets.first().map(
                |(offered_good, pickup_waypoint_symbol, maybe_market_entry)| {
                    let pickup_waypoint = waypoints
                        .iter()
                        .find(|wp| wp.symbol == *pickup_waypoint_symbol)
                        .unwrap();
                    let delivery_waypoint = waypoints
                        .iter()
                        .find(|wp| wp.symbol == receiving_market.waypoint_symbol)
                        .unwrap();
                    TradeGoodRoute {
                        trade_good_symbol: offered_good.clone(),
                        source_waypoint: pickup_waypoint.clone(),
                        maybe_source_market_entry: maybe_market_entry.clone(),
                        delivery_waypoint: delivery_waypoint.clone(),
                        delivery_market_entry: import_market_at_receiving_market.clone(),
                        destination_market_entry: receiving_market.clone(),
                        maybe_raw_material_source_type: None,
                        level: 42,
                        distance: pickup_waypoint.distance_to(delivery_waypoint),
                    }
                },
            );
            println!("Hello, debug");
            result
        })
        .collect_vec();

    let new_current_routes = current_routes
        .into_iter()
        .chain(new_routes.into_iter())
        .collect_vec();

    recurse_collect_rest_routes(
        new_current_routes,
        complete_chain,
        market_entries,
        waypoints,
    )
}

fn rank_supply_chain(complete_chain: &Vec<SupplyChainNode>) -> HashMap<TradeGoodSymbol, u32> {
    fn recurse(
        complete_chain: &Vec<SupplyChainNode>,
        current_ranks: HashMap<TradeGoodSymbol, u32>,
    ) -> HashMap<TradeGoodSymbol, u32> {
        // we're done - all entries are in rank map
        if complete_chain.len() == current_ranks.len() {
            return current_ranks;
        }

        let to_visit = complete_chain
            .into_iter()
            .filter(|scn| !current_ranks.contains_key(&scn.good));

        // all dependencies explored
        let can_visit = to_visit.filter(|scn| {
            scn.dependencies
                .iter()
                .all(|dep| current_ranks.contains_key(dep))
        });

        // take the max rank of the dependencies and add the current node with +1
        let newly_ranked: Vec<(TradeGoodSymbol, u32)> = can_visit
            .map(|scn| {
                let max_rank = scn
                    .dependencies
                    .iter()
                    .filter_map(|dep| current_ranks.get(dep))
                    .max()
                    .unwrap();
                (scn.good.clone(), max_rank + 1)
            })
            .collect();

        let mut new_rank_map = current_ranks.clone();
        new_rank_map.extend(newly_ranked);
        recurse(complete_chain, new_rank_map)
    }

    // all raw materials are rank 0
    let raw_ranks: HashMap<TradeGoodSymbol, u32> = HashMap::from_iter(
        complete_chain
            .into_iter()
            .filter(|scn| scn.dependencies.is_empty())
            .map(|scn| (scn.good.clone(), 0)),
    );
    let ranked = recurse(complete_chain, raw_ranks);

    ranked
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash, Display)]
enum RawMaterialSourceType {
    Extraction,
    Siphoning,
}

struct SupplyChainProcessingNode {
    trade_good: TradeGood,
    waypoint_symbol: WaypointSymbol,
    trade_good_type: TradeGoodType,
    //import: ImportDetails,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")] // add back later - data in db is wrong
struct TradeGoodRoute {
    trade_good_symbol: TradeGoodSymbol,
    source_waypoint: Waypoint,
    maybe_source_market_entry: Option<MarketEntry>,
    delivery_waypoint: Waypoint,
    delivery_market_entry: MarketEntry,
    destination_market_entry: MarketEntry,
    maybe_raw_material_source_type: Option<RawMaterialSourceType>,
    level: u32,
    distance: u32,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash)]
// #[serde(rename_all = "camelCase")] // custom select - need to refactor to work with MarketData struct (and the flattened field trade_goods: Option<Vec<MarketTradeGood>>
struct MarketEntry {
    pub system_symbol: SystemSymbol,
    pub waypoint_symbol: WaypointSymbol,
    pub created_at: DateTime<Utc>,
    pub trade_good_type: TradeGoodType,
    pub trade_good_supply: SupplyLevel,
    pub trade_good_symbol: TradeGoodSymbol,
    pub trade_good_activity: Option<ActivityLevel>,
    pub trade_good_sell_price: u32,
    pub trade_good_purchase_price: u32,
    pub trade_good_trade_volume: u32,
}

use flwi_spacetraders_agent::st_model::Waypoint;

fn get_market_entries() -> Result<Vec<MarketEntry>> {
    let test_data_json = r#"
        [{"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-A1","created_at":"2024-09-02T13:06:13.758979+00:00","trade_good_type":"IMPORT","trade_good_supply":"LIMITED","trade_good_symbol":"FOOD","trade_good_activity":"WEAK","trade_good_sell_price":2168,"trade_good_trade_volume":60,"trade_good_purchase_price":4468}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-A1","created_at":"2024-09-02T13:06:13.758979+00:00","trade_good_type":"IMPORT","trade_good_supply":"LIMITED","trade_good_symbol":"MEDICINE","trade_good_activity":"WEAK","trade_good_sell_price":4617,"trade_good_trade_volume":20,"trade_good_purchase_price":9568}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-A1","created_at":"2024-09-02T13:06:13.758979+00:00","trade_good_type":"IMPORT","trade_good_supply":"MODERATE","trade_good_symbol":"CLOTHING","trade_good_activity":"WEAK","trade_good_sell_price":4757,"trade_good_trade_volume":20,"trade_good_purchase_price":9906}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-A1","created_at":"2024-09-02T13:06:13.758979+00:00","trade_good_type":"IMPORT","trade_good_supply":"LIMITED","trade_good_symbol":"EQUIPMENT","trade_good_activity":"WEAK","trade_good_sell_price":3213,"trade_good_trade_volume":20,"trade_good_purchase_price":6636}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-A1","created_at":"2024-09-02T13:06:13.758979+00:00","trade_good_type":"IMPORT","trade_good_supply":"MODERATE","trade_good_symbol":"JEWELRY","trade_good_activity":"WEAK","trade_good_sell_price":3284,"trade_good_trade_volume":20,"trade_good_purchase_price":6808}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-A1","created_at":"2024-09-02T13:06:13.758979+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"MICRO_FUSION_GENERATORS","trade_good_activity":"WEAK","trade_good_sell_price":77112,"trade_good_trade_volume":6,"trade_good_purchase_price":154558}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-A1","created_at":"2024-09-02T13:06:13.758979+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"MODERATE","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":68,"trade_good_trade_volume":180,"trade_good_purchase_price":72}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-A2","created_at":"2024-09-12T15:30:18.289581+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"SHIP_PLATING","trade_good_activity":"GROWING","trade_good_sell_price":7656,"trade_good_trade_volume":6,"trade_good_purchase_price":15462}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-A2","created_at":"2024-09-12T15:30:18.289581+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"SHIP_PARTS","trade_good_activity":"GROWING","trade_good_sell_price":7705,"trade_good_trade_volume":6,"trade_good_purchase_price":15558}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-A2","created_at":"2024-09-12T15:30:18.289581+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"MODERATE","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":68,"trade_good_trade_volume":180,"trade_good_purchase_price":72}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-A3","created_at":"2024-09-12T14:09:45.816475+00:00","trade_good_type":"EXPORT","trade_good_supply":"MODERATE","trade_good_symbol":"MICROPROCESSORS","trade_good_activity":"RESTRICTED","trade_good_sell_price":1450,"trade_good_trade_volume":43,"trade_good_purchase_price":3187}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-A3","created_at":"2024-09-12T14:09:45.816475+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"SILICON_CRYSTALS","trade_good_activity":"WEAK","trade_good_sell_price":47,"trade_good_trade_volume":60,"trade_good_purchase_price":94}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-A3","created_at":"2024-09-12T14:09:45.816475+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"COPPER","trade_good_activity":"WEAK","trade_good_sell_price":322,"trade_good_trade_volume":60,"trade_good_purchase_price":648}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-A3","created_at":"2024-09-12T14:09:45.816475+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"MODERATE","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":68,"trade_good_trade_volume":180,"trade_good_purchase_price":72}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-A4","created_at":"2024-09-12T14:10:49.666793+00:00","trade_good_type":"EXPORT","trade_good_supply":"ABUNDANT","trade_good_symbol":"LASER_RIFLES","trade_good_activity":"RESTRICTED","trade_good_sell_price":6145,"trade_good_trade_volume":6,"trade_good_purchase_price":12357}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-A4","created_at":"2024-09-12T14:10:49.666793+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"DIAMONDS","trade_good_activity":"RESTRICTED","trade_good_sell_price":117,"trade_good_trade_volume":60,"trade_good_purchase_price":236}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-A4","created_at":"2024-09-12T14:10:49.666793+00:00","trade_good_type":"IMPORT","trade_good_supply":"MODERATE","trade_good_symbol":"PLATINUM","trade_good_activity":"RESTRICTED","trade_good_sell_price":294,"trade_good_trade_volume":60,"trade_good_purchase_price":620}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-A4","created_at":"2024-09-12T14:10:49.666793+00:00","trade_good_type":"IMPORT","trade_good_supply":"ABUNDANT","trade_good_symbol":"ADVANCED_CIRCUITRY","trade_good_activity":"RESTRICTED","trade_good_sell_price":3218,"trade_good_trade_volume":20,"trade_good_purchase_price":7762}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-A4","created_at":"2024-09-12T14:10:49.666793+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"MODERATE","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":68,"trade_good_trade_volume":180,"trade_good_purchase_price":72}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-B6","created_at":"2024-09-12T14:17:51.851025+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"MODERATE","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":68,"trade_good_trade_volume":180,"trade_good_purchase_price":72}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-B7","created_at":"2024-09-12T14:19:28.795818+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"MODERATE","trade_good_symbol":"ICE_WATER","trade_good_activity":null,"trade_good_sell_price":13,"trade_good_trade_volume":180,"trade_good_purchase_price":17}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-B7","created_at":"2024-09-12T14:19:28.795818+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"LIMITED","trade_good_symbol":"AMMONIA_ICE","trade_good_activity":null,"trade_good_sell_price":38,"trade_good_trade_volume":180,"trade_good_purchase_price":42}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-B7","created_at":"2024-09-12T14:19:28.795818+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"MODERATE","trade_good_symbol":"QUARTZ_SAND","trade_good_activity":null,"trade_good_sell_price":18,"trade_good_trade_volume":180,"trade_good_purchase_price":22}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-B7","created_at":"2024-09-12T14:19:28.795818+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"MODERATE","trade_good_symbol":"SILICON_CRYSTALS","trade_good_activity":null,"trade_good_sell_price":33,"trade_good_trade_volume":180,"trade_good_purchase_price":37}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-B7","created_at":"2024-09-12T14:19:28.795818+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"MODERATE","trade_good_symbol":"IRON_ORE","trade_good_activity":null,"trade_good_sell_price":48,"trade_good_trade_volume":180,"trade_good_purchase_price":52}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-B7","created_at":"2024-09-12T14:19:28.795818+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"LIMITED","trade_good_symbol":"COPPER_ORE","trade_good_activity":null,"trade_good_sell_price":53,"trade_good_trade_volume":180,"trade_good_purchase_price":59}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-B7","created_at":"2024-09-12T14:19:28.795818+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"MODERATE","trade_good_symbol":"ALUMINUM_ORE","trade_good_activity":null,"trade_good_sell_price":58,"trade_good_trade_volume":180,"trade_good_purchase_price":62}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-B7","created_at":"2024-09-12T14:19:28.795818+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"HIGH","trade_good_symbol":"URANITE_ORE","trade_good_activity":null,"trade_good_sell_price":309,"trade_good_trade_volume":180,"trade_good_purchase_price":322}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-B7","created_at":"2024-09-12T14:19:28.795818+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"MODERATE","trade_good_symbol":"MERITIUM_ORE","trade_good_activity":null,"trade_good_sell_price":1186,"trade_good_trade_volume":180,"trade_good_purchase_price":1212}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-B7","created_at":"2024-09-12T14:19:28.795818+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"MODERATE","trade_good_symbol":"PRECIOUS_STONES","trade_good_activity":null,"trade_good_sell_price":73,"trade_good_trade_volume":180,"trade_good_purchase_price":77}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-B7","created_at":"2024-09-12T14:19:28.795818+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"SCARCE","trade_good_symbol":"DIAMONDS","trade_good_activity":null,"trade_good_sell_price":89,"trade_good_trade_volume":180,"trade_good_purchase_price":97}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-B7","created_at":"2024-09-12T14:19:28.795818+00:00","trade_good_type":"EXPORT","trade_good_supply":"LIMITED","trade_good_symbol":"GOLD","trade_good_activity":"RESTRICTED","trade_good_sell_price":174,"trade_good_trade_volume":60,"trade_good_purchase_price":389}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-B7","created_at":"2024-09-12T14:19:28.795818+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"GOLD_ORE","trade_good_activity":"WEAK","trade_good_sell_price":110,"trade_good_trade_volume":60,"trade_good_purchase_price":222}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-B7","created_at":"2024-09-12T14:19:28.795818+00:00","trade_good_type":"EXPORT","trade_good_supply":"MODERATE","trade_good_symbol":"PLATINUM","trade_good_activity":"RESTRICTED","trade_good_sell_price":120,"trade_good_trade_volume":60,"trade_good_purchase_price":252}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-B7","created_at":"2024-09-12T14:19:28.795818+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"SCARCE","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":68,"trade_good_trade_volume":180,"trade_good_purchase_price":74}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-B7","created_at":"2024-09-12T14:19:28.795818+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"SILVER_ORE","trade_good_activity":"WEAK","trade_good_sell_price":93,"trade_good_trade_volume":60,"trade_good_purchase_price":188}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-B7","created_at":"2024-09-12T14:19:28.795818+00:00","trade_good_type":"EXPORT","trade_good_supply":"LIMITED","trade_good_symbol":"SILVER","trade_good_activity":"RESTRICTED","trade_good_sell_price":143,"trade_good_trade_volume":60,"trade_good_purchase_price":312}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-B7","created_at":"2024-09-12T14:19:28.795818+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"PLATINUM_ORE","trade_good_activity":"WEAK","trade_good_sell_price":107,"trade_good_trade_volume":60,"trade_good_purchase_price":216}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-C46","created_at":"2024-09-12T15:34:05.15023+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"SCARCE","trade_good_symbol":"HYDROCARBON","trade_good_activity":null,"trade_good_sell_price":43,"trade_good_trade_volume":180,"trade_good_purchase_price":49}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-C46","created_at":"2024-09-12T15:34:05.15023+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"LIMITED","trade_good_symbol":"LIQUID_HYDROGEN","trade_good_activity":null,"trade_good_sell_price":23,"trade_good_trade_volume":180,"trade_good_purchase_price":28}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-C46","created_at":"2024-09-12T15:34:05.15023+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"SCARCE","trade_good_symbol":"LIQUID_NITROGEN","trade_good_activity":null,"trade_good_sell_price":29,"trade_good_trade_volume":180,"trade_good_purchase_price":34}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-C46","created_at":"2024-09-12T15:34:05.15023+00:00","trade_good_type":"EXPORT","trade_good_supply":"ABUNDANT","trade_good_symbol":"LAB_INSTRUMENTS","trade_good_activity":"RESTRICTED","trade_good_sell_price":1594,"trade_good_trade_volume":20,"trade_good_purchase_price":3234}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-C46","created_at":"2024-09-12T15:34:05.15023+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"ELECTRONICS","trade_good_activity":"WEAK","trade_good_sell_price":2853,"trade_good_trade_volume":20,"trade_good_purchase_price":5792}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-C46","created_at":"2024-09-12T15:34:05.15023+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"EQUIPMENT","trade_good_activity":"WEAK","trade_good_sell_price":3378,"trade_good_trade_volume":20,"trade_good_purchase_price":6806}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-C46","created_at":"2024-09-12T15:34:05.15023+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"SHIP_PLATING","trade_good_activity":"GROWING","trade_good_sell_price":7419,"trade_good_trade_volume":6,"trade_good_purchase_price":14982}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-C46","created_at":"2024-09-12T15:34:05.15023+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"SHIP_PARTS","trade_good_activity":"GROWING","trade_good_sell_price":7731,"trade_good_trade_volume":6,"trade_good_purchase_price":15600}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-C46","created_at":"2024-09-12T15:34:05.15023+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"MODERATE","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":68,"trade_good_trade_volume":180,"trade_good_purchase_price":72}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-C47","created_at":"2024-09-12T14:12:25.334133+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"HIGH","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":67,"trade_good_trade_volume":180,"trade_good_purchase_price":72}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-D48","created_at":"2024-09-12T14:14:18.190541+00:00","trade_good_type":"EXPORT","trade_good_supply":"SCARCE","trade_good_symbol":"SHIP_PARTS","trade_good_activity":"RESTRICTED","trade_good_sell_price":3429,"trade_good_trade_volume":15,"trade_good_purchase_price":7757}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-D48","created_at":"2024-09-12T14:14:18.190541+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"EQUIPMENT","trade_good_activity":"WEAK","trade_good_sell_price":3486,"trade_good_trade_volume":20,"trade_good_purchase_price":7030}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-D48","created_at":"2024-09-12T14:14:18.190541+00:00","trade_good_type":"IMPORT","trade_good_supply":"LIMITED","trade_good_symbol":"ELECTRONICS","trade_good_activity":"WEAK","trade_good_sell_price":2747,"trade_good_trade_volume":20,"trade_good_purchase_price":5620}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-D48","created_at":"2024-09-12T14:14:18.190541+00:00","trade_good_type":"EXPORT","trade_good_supply":"LIMITED","trade_good_symbol":"MEDICINE","trade_good_activity":"RESTRICTED","trade_good_sell_price":2270,"trade_good_trade_volume":20,"trade_good_purchase_price":5105}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-D48","created_at":"2024-09-12T14:14:18.190541+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"LIMITED","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":68,"trade_good_trade_volume":180,"trade_good_purchase_price":73}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-D48","created_at":"2024-09-12T14:14:18.190541+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"POLYNUCLEOTIDES","trade_good_activity":"WEAK","trade_good_sell_price":295,"trade_good_trade_volume":20,"trade_good_purchase_price":594}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-D48","created_at":"2024-09-12T14:14:18.190541+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"FABRICS","trade_good_activity":"WEAK","trade_good_sell_price":2510,"trade_good_trade_volume":60,"trade_good_purchase_price":5054}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-D49","created_at":"2024-09-12T12:49:28.836873+00:00","trade_good_type":"EXPORT","trade_good_supply":"SCARCE","trade_good_symbol":"SHIP_PLATING","trade_good_activity":"RESTRICTED","trade_good_sell_price":3156,"trade_good_trade_volume":15,"trade_good_purchase_price":7050}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-D49","created_at":"2024-09-12T12:49:28.836873+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"ALUMINUM","trade_good_activity":"WEAK","trade_good_sell_price":267,"trade_good_trade_volume":60,"trade_good_purchase_price":538}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-D49","created_at":"2024-09-12T12:49:28.836873+00:00","trade_good_type":"EXPORT","trade_good_supply":"ABUNDANT","trade_good_symbol":"ADVANCED_CIRCUITRY","trade_good_activity":"GROWING","trade_good_sell_price":1508,"trade_good_trade_volume":20,"trade_good_purchase_price":3077}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-D49","created_at":"2024-09-12T12:49:28.836873+00:00","trade_good_type":"IMPORT","trade_good_supply":"LIMITED","trade_good_symbol":"ELECTRONICS","trade_good_activity":"RESTRICTED","trade_good_sell_price":2774,"trade_good_trade_volume":20,"trade_good_purchase_price":5662}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-D49","created_at":"2024-09-12T12:49:28.836873+00:00","trade_good_type":"IMPORT","trade_good_supply":"LIMITED","trade_good_symbol":"MICROPROCESSORS","trade_good_activity":"RESTRICTED","trade_good_sell_price":3483,"trade_good_trade_volume":53,"trade_good_purchase_price":7122}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-D49","created_at":"2024-09-12T12:49:28.836873+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"LIMITED","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":68,"trade_good_trade_volume":180,"trade_good_purchase_price":74}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-D49","created_at":"2024-09-12T12:49:28.836873+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"MACHINERY","trade_good_activity":"GROWING","trade_good_sell_price":3230,"trade_good_trade_volume":20,"trade_good_purchase_price":6510}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-DE5F","created_at":"2024-09-11T15:08:24.697076+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"MODERATE","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":68,"trade_good_trade_volume":180,"trade_good_purchase_price":72}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-E50","created_at":"2024-09-12T14:38:40.789998+00:00","trade_good_type":"EXPORT","trade_good_supply":"SCARCE","trade_good_symbol":"ASSAULT_RIFLES","trade_good_activity":"RESTRICTED","trade_good_sell_price":2112,"trade_good_trade_volume":20,"trade_good_purchase_price":4728}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-E50","created_at":"2024-09-12T14:38:40.789998+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"ALUMINUM","trade_good_activity":"WEAK","trade_good_sell_price":268,"trade_good_trade_volume":60,"trade_good_purchase_price":540}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-E50","created_at":"2024-09-12T14:38:40.789998+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"AMMUNITION","trade_good_activity":"GROWING","trade_good_sell_price":1825,"trade_good_trade_volume":20,"trade_good_purchase_price":3674}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-E50","created_at":"2024-09-12T14:38:40.789998+00:00","trade_good_type":"EXPORT","trade_good_supply":"LIMITED","trade_good_symbol":"FIREARMS","trade_good_activity":"RESTRICTED","trade_good_sell_price":1854,"trade_good_trade_volume":20,"trade_good_purchase_price":4196}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-E50","created_at":"2024-09-12T14:38:40.789998+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"IRON","trade_good_activity":"WEAK","trade_good_sell_price":161,"trade_good_trade_volume":60,"trade_good_purchase_price":324}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-E50","created_at":"2024-09-12T14:38:40.789998+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"MODERATE","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":68,"trade_good_trade_volume":180,"trade_good_purchase_price":73}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-E51","created_at":"2024-09-12T14:37:23.645556+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"IRON","trade_good_activity":"WEAK","trade_good_sell_price":165,"trade_good_trade_volume":60,"trade_good_purchase_price":334}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-E51","created_at":"2024-09-12T14:37:23.645556+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"LIMITED","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":68,"trade_good_trade_volume":180,"trade_good_purchase_price":73}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-E51","created_at":"2024-09-12T14:37:23.645556+00:00","trade_good_type":"EXPORT","trade_good_supply":"LIMITED","trade_good_symbol":"POLYNUCLEOTIDES","trade_good_activity":"RESTRICTED","trade_good_sell_price":112,"trade_good_trade_volume":43,"trade_good_purchase_price":244}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-E51","created_at":"2024-09-12T14:37:23.645556+00:00","trade_good_type":"EXPORT","trade_good_supply":"SCARCE","trade_good_symbol":"FABRICS","trade_good_activity":"RESTRICTED","trade_good_sell_price":1246,"trade_good_trade_volume":60,"trade_good_purchase_price":2768}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-E51","created_at":"2024-09-12T14:37:23.645556+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"FERTILIZERS","trade_good_activity":"WEAK","trade_good_sell_price":241,"trade_good_trade_volume":60,"trade_good_purchase_price":486}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-E51","created_at":"2024-09-12T14:37:23.645556+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"LIQUID_NITROGEN","trade_good_activity":"WEAK","trade_good_sell_price":41,"trade_good_trade_volume":60,"trade_good_purchase_price":84}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-E51","created_at":"2024-09-12T14:37:23.645556+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"LIQUID_HYDROGEN","trade_good_activity":"WEAK","trade_good_sell_price":36,"trade_good_trade_volume":60,"trade_good_purchase_price":72}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-E51","created_at":"2024-09-12T14:37:23.645556+00:00","trade_good_type":"EXPORT","trade_good_supply":"LIMITED","trade_good_symbol":"MACHINERY","trade_good_activity":"RESTRICTED","trade_good_sell_price":1406,"trade_good_trade_volume":43,"trade_good_purchase_price":3129}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-F52","created_at":"2024-09-12T14:16:45.637247+00:00","trade_good_type":"EXPORT","trade_good_supply":"LIMITED","trade_good_symbol":"AMMUNITION","trade_good_activity":"RESTRICTED","trade_good_sell_price":794,"trade_good_trade_volume":43,"trade_good_purchase_price":1761}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-F52","created_at":"2024-09-12T14:16:45.637247+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"IRON","trade_good_activity":"WEAK","trade_good_sell_price":161,"trade_good_trade_volume":60,"trade_good_purchase_price":324}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-F52","created_at":"2024-09-12T14:16:45.637247+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"LIQUID_NITROGEN","trade_good_activity":"WEAK","trade_good_sell_price":38,"trade_good_trade_volume":60,"trade_good_purchase_price":76}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-F52","created_at":"2024-09-12T14:16:45.637247+00:00","trade_good_type":"EXPORT","trade_good_supply":"ABUNDANT","trade_good_symbol":"EXPLOSIVES","trade_good_activity":"RESTRICTED","trade_good_sell_price":39,"trade_good_trade_volume":20,"trade_good_purchase_price":80}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-F52","created_at":"2024-09-12T14:16:45.637247+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"LIQUID_HYDROGEN","trade_good_activity":"RESTRICTED","trade_good_sell_price":32,"trade_good_trade_volume":60,"trade_good_purchase_price":66}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-F52","created_at":"2024-09-12T14:16:45.637247+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"SCARCE","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":69,"trade_good_trade_volume":180,"trade_good_purchase_price":75}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-F53","created_at":"2024-09-12T14:15:28.556668+00:00","trade_good_type":"EXPORT","trade_good_supply":"SCARCE","trade_good_symbol":"ELECTRONICS","trade_good_activity":"RESTRICTED","trade_good_sell_price":1268,"trade_good_trade_volume":43,"trade_good_purchase_price":2765}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-F53","created_at":"2024-09-12T14:15:28.556668+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"SILICON_CRYSTALS","trade_good_activity":"WEAK","trade_good_sell_price":45,"trade_good_trade_volume":60,"trade_good_purchase_price":92}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-F53","created_at":"2024-09-12T14:15:28.556668+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"COPPER","trade_good_activity":"WEAK","trade_good_sell_price":329,"trade_good_trade_volume":60,"trade_good_purchase_price":664}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-F53","created_at":"2024-09-12T14:15:28.556668+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"SCARCE","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":69,"trade_good_trade_volume":180,"trade_good_purchase_price":74}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-F53","created_at":"2024-09-12T14:15:28.556668+00:00","trade_good_type":"EXPORT","trade_good_supply":"ABUNDANT","trade_good_symbol":"FAB_MATS","trade_good_activity":"RESTRICTED","trade_good_sell_price":520,"trade_good_trade_volume":20,"trade_good_purchase_price":1057}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-F53","created_at":"2024-09-12T14:15:28.556668+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"IRON","trade_good_activity":"RESTRICTED","trade_good_sell_price":156,"trade_good_trade_volume":60,"trade_good_purchase_price":314}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-F53","created_at":"2024-09-12T14:15:28.556668+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"QUARTZ_SAND","trade_good_activity":"RESTRICTED","trade_good_sell_price":27,"trade_good_trade_volume":60,"trade_good_purchase_price":54}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-G54","created_at":"2024-09-12T14:11:46.176607+00:00","trade_good_type":"EXPORT","trade_good_supply":"ABUNDANT","trade_good_symbol":"FUEL","trade_good_activity":"RESTRICTED","trade_good_sell_price":25,"trade_good_trade_volume":60,"trade_good_purchase_price":49}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-G54","created_at":"2024-09-12T14:11:46.176607+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"HYDROCARBON","trade_good_activity":"RESTRICTED","trade_good_sell_price":64,"trade_good_trade_volume":60,"trade_good_purchase_price":130}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-G54","created_at":"2024-09-12T14:11:46.176607+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"LIQUID_HYDROGEN","trade_good_activity":"WEAK","trade_good_sell_price":32,"trade_good_trade_volume":60,"trade_good_purchase_price":66}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-G54","created_at":"2024-09-12T14:11:46.176607+00:00","trade_good_type":"EXPORT","trade_good_supply":"LIMITED","trade_good_symbol":"FERTILIZERS","trade_good_activity":"RESTRICTED","trade_good_sell_price":110,"trade_good_trade_volume":60,"trade_good_purchase_price":248}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-G54","created_at":"2024-09-12T14:11:46.176607+00:00","trade_good_type":"EXPORT","trade_good_supply":"LIMITED","trade_good_symbol":"PLASTICS","trade_good_activity":"RESTRICTED","trade_good_sell_price":90,"trade_good_trade_volume":60,"trade_good_purchase_price":196}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-G54","created_at":"2024-09-12T14:11:46.176607+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"LIQUID_NITROGEN","trade_good_activity":"WEAK","trade_good_sell_price":41,"trade_good_trade_volume":60,"trade_good_purchase_price":84}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-H55","created_at":"2024-09-10T15:24:21.330004+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"DRUGS","trade_good_activity":"GROWING","trade_good_sell_price":5641,"trade_good_trade_volume":20,"trade_good_purchase_price":11372}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-H55","created_at":"2024-09-10T15:24:21.330004+00:00","trade_good_type":"EXPORT","trade_good_supply":"LIMITED","trade_good_symbol":"IRON","trade_good_activity":"RESTRICTED","trade_good_sell_price":68,"trade_good_trade_volume":60,"trade_good_purchase_price":150}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-H55","created_at":"2024-09-10T15:24:21.330004+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"IRON_ORE","trade_good_activity":"WEAK","trade_good_sell_price":69,"trade_good_trade_volume":60,"trade_good_purchase_price":140}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-H55","created_at":"2024-09-10T15:24:21.330004+00:00","trade_good_type":"EXPORT","trade_good_supply":"SCARCE","trade_good_symbol":"ALUMINUM","trade_good_activity":"RESTRICTED","trade_good_sell_price":124,"trade_good_trade_volume":60,"trade_good_purchase_price":278}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-H55","created_at":"2024-09-10T15:24:21.330004+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"MODERATE","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":68,"trade_good_trade_volume":180,"trade_good_purchase_price":72}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-H55","created_at":"2024-09-10T15:24:21.330004+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"ALUMINUM_ORE","trade_good_activity":"WEAK","trade_good_sell_price":78,"trade_good_trade_volume":60,"trade_good_purchase_price":156}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-H55","created_at":"2024-09-10T15:24:21.330004+00:00","trade_good_type":"EXPORT","trade_good_supply":"SCARCE","trade_good_symbol":"COPPER","trade_good_activity":"RESTRICTED","trade_good_sell_price":144,"trade_good_trade_volume":60,"trade_good_purchase_price":321}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-H55","created_at":"2024-09-10T15:24:21.330004+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"COPPER_ORE","trade_good_activity":"WEAK","trade_good_sell_price":74,"trade_good_trade_volume":60,"trade_good_purchase_price":148}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-H56","created_at":"2024-09-02T13:06:13.758979+00:00","trade_good_type":"IMPORT","trade_good_supply":"LIMITED","trade_good_symbol":"SHIP_PLATING","trade_good_activity":"WEAK","trade_good_sell_price":6520,"trade_good_trade_volume":6,"trade_good_purchase_price":13520}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-H56","created_at":"2024-09-02T13:06:13.758979+00:00","trade_good_type":"IMPORT","trade_good_supply":"MODERATE","trade_good_symbol":"SHIP_PARTS","trade_good_activity":"WEAK","trade_good_sell_price":6802,"trade_good_trade_volume":6,"trade_good_purchase_price":14194}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-H56","created_at":"2024-09-02T13:06:13.758979+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"MODERATE","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":68,"trade_good_trade_volume":180,"trade_good_purchase_price":72}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-H57","created_at":"2024-09-12T11:15:10.201052+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"HIGH","trade_good_symbol":"ICE_WATER","trade_good_activity":null,"trade_good_sell_price":13,"trade_good_trade_volume":180,"trade_good_purchase_price":17}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-H57","created_at":"2024-09-12T11:15:10.201052+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"MODERATE","trade_good_symbol":"AMMONIA_ICE","trade_good_activity":null,"trade_good_sell_price":38,"trade_good_trade_volume":180,"trade_good_purchase_price":42}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-H57","created_at":"2024-09-12T11:15:10.201052+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"HIGH","trade_good_symbol":"QUARTZ_SAND","trade_good_activity":null,"trade_good_sell_price":18,"trade_good_trade_volume":180,"trade_good_purchase_price":22}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-H57","created_at":"2024-09-12T11:15:10.201052+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"SCARCE","trade_good_symbol":"SILICON_CRYSTALS","trade_good_activity":null,"trade_good_sell_price":34,"trade_good_trade_volume":180,"trade_good_purchase_price":40}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-H57","created_at":"2024-09-12T11:15:10.201052+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"MODERATE","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":68,"trade_good_trade_volume":180,"trade_good_purchase_price":72}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-H58","created_at":"2024-09-12T11:16:19.946207+00:00","trade_good_type":"EXPORT","trade_good_supply":"LIMITED","trade_good_symbol":"JEWELRY","trade_good_activity":"RESTRICTED","trade_good_sell_price":1600,"trade_good_trade_volume":43,"trade_good_purchase_price":3582}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-H58","created_at":"2024-09-12T11:16:19.946207+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"GOLD","trade_good_activity":"WEAK","trade_good_sell_price":389,"trade_good_trade_volume":60,"trade_good_purchase_price":784}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-H58","created_at":"2024-09-12T11:16:19.946207+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"SILVER","trade_good_activity":"WEAK","trade_good_sell_price":348,"trade_good_trade_volume":60,"trade_good_purchase_price":702}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-H58","created_at":"2024-09-12T11:16:19.946207+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"PRECIOUS_STONES","trade_good_activity":"WEAK","trade_good_sell_price":101,"trade_good_trade_volume":60,"trade_good_purchase_price":202}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-H58","created_at":"2024-09-12T11:16:19.946207+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"DIAMONDS","trade_good_activity":"WEAK","trade_good_sell_price":121,"trade_good_trade_volume":60,"trade_good_purchase_price":244}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-H58","created_at":"2024-09-12T11:16:19.946207+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"MODERATE","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":68,"trade_good_trade_volume":180,"trade_good_purchase_price":72}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-I59","created_at":"2024-09-12T14:32:28.651089+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"HIGH","trade_good_symbol":"ANTIMATTER","trade_good_activity":null,"trade_good_sell_price":13310,"trade_good_trade_volume":18,"trade_good_purchase_price":14027}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-I59","created_at":"2024-09-12T14:32:28.651089+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"SCARCE","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":80,"trade_good_trade_volume":180,"trade_good_purchase_price":96}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-I60","created_at":"2024-09-12T14:35:23.324835+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"SCARCE","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":74,"trade_good_trade_volume":180,"trade_good_purchase_price":86}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-J61","created_at":"2024-09-12T14:31:05.967549+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"MODERATE","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":68,"trade_good_trade_volume":180,"trade_good_purchase_price":72}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-J62","created_at":"2024-09-12T14:29:57.394258+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"FOOD","trade_good_activity":"WEAK","trade_good_sell_price":2394,"trade_good_trade_volume":60,"trade_good_purchase_price":4836}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-J62","created_at":"2024-09-12T14:29:57.394258+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"CLOTHING","trade_good_activity":"GROWING","trade_good_sell_price":5199,"trade_good_trade_volume":20,"trade_good_purchase_price":10472}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-J62","created_at":"2024-09-12T14:29:57.394258+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"EQUIPMENT","trade_good_activity":"WEAK","trade_good_sell_price":3487,"trade_good_trade_volume":20,"trade_good_purchase_price":7032}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-J62","created_at":"2024-09-12T14:29:57.394258+00:00","trade_good_type":"EXPORT","trade_good_supply":"ABUNDANT","trade_good_symbol":"IRON_ORE","trade_good_activity":"RESTRICTED","trade_good_sell_price":17,"trade_good_trade_volume":60,"trade_good_purchase_price":34}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-J62","created_at":"2024-09-12T14:29:57.394258+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"EXPLOSIVES","trade_good_activity":"WEAK","trade_good_sell_price":194,"trade_good_trade_volume":20,"trade_good_purchase_price":390}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-J62","created_at":"2024-09-12T14:29:57.394258+00:00","trade_good_type":"EXPORT","trade_good_supply":"ABUNDANT","trade_good_symbol":"COPPER_ORE","trade_good_activity":"RESTRICTED","trade_good_sell_price":18,"trade_good_trade_volume":60,"trade_good_purchase_price":36}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-J62","created_at":"2024-09-12T14:29:57.394258+00:00","trade_good_type":"EXPORT","trade_good_supply":"ABUNDANT","trade_good_symbol":"ALUMINUM_ORE","trade_good_activity":"RESTRICTED","trade_good_sell_price":21,"trade_good_trade_volume":60,"trade_good_purchase_price":42}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-J62","created_at":"2024-09-12T14:29:57.394258+00:00","trade_good_type":"EXPORT","trade_good_supply":"HIGH","trade_good_symbol":"PRECIOUS_STONES","trade_good_activity":"RESTRICTED","trade_good_sell_price":30,"trade_good_trade_volume":60,"trade_good_purchase_price":62}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-J62","created_at":"2024-09-12T14:29:57.394258+00:00","trade_good_type":"EXPORT","trade_good_supply":"SCARCE","trade_good_symbol":"DRUGS","trade_good_activity":"RESTRICTED","trade_good_sell_price":2652,"trade_good_trade_volume":20,"trade_good_purchase_price":5946}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-J62","created_at":"2024-09-12T14:29:57.394258+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"AMMONIA_ICE","trade_good_activity":"WEAK","trade_good_sell_price":52,"trade_good_trade_volume":60,"trade_good_purchase_price":104}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-J62","created_at":"2024-09-12T14:29:57.394258+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"POLYNUCLEOTIDES","trade_good_activity":"WEAK","trade_good_sell_price":304,"trade_good_trade_volume":20,"trade_good_purchase_price":614}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-J62","created_at":"2024-09-12T14:29:57.394258+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"FIREARMS","trade_good_activity":"GROWING","trade_good_sell_price":4269,"trade_good_trade_volume":20,"trade_good_purchase_price":8618}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-J62","created_at":"2024-09-12T14:29:57.394258+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"ASSAULT_RIFLES","trade_good_activity":"GROWING","trade_good_sell_price":4560,"trade_good_trade_volume":20,"trade_good_purchase_price":9196}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-J62","created_at":"2024-09-12T14:29:57.394258+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"SCARCE","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":69,"trade_good_trade_volume":180,"trade_good_purchase_price":77}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-J62","created_at":"2024-09-12T14:29:57.394258+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"MEDICINE","trade_good_activity":"WEAK","trade_good_sell_price":4771,"trade_good_trade_volume":20,"trade_good_purchase_price":9606}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-K83","created_at":"2024-09-12T14:36:44.433336+00:00","trade_good_type":"EXPORT","trade_good_supply":"SCARCE","trade_good_symbol":"FOOD","trade_good_activity":"RESTRICTED","trade_good_sell_price":1120,"trade_good_trade_volume":60,"trade_good_purchase_price":2508}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-K83","created_at":"2024-09-12T14:36:44.433336+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"ALUMINUM","trade_good_activity":"WEAK","trade_good_sell_price":268,"trade_good_trade_volume":60,"trade_good_purchase_price":540}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-K83","created_at":"2024-09-12T14:36:44.433336+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"PLASTICS","trade_good_activity":"WEAK","trade_good_sell_price":213,"trade_good_trade_volume":60,"trade_good_purchase_price":430}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-K83","created_at":"2024-09-12T14:36:44.433336+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"FABRICS","trade_good_activity":"WEAK","trade_good_sell_price":2667,"trade_good_trade_volume":60,"trade_good_purchase_price":5384}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-K83","created_at":"2024-09-12T14:36:44.433336+00:00","trade_good_type":"EXPORT","trade_good_supply":"ABUNDANT","trade_good_symbol":"BIOCOMPOSITES","trade_good_activity":"RESTRICTED","trade_good_sell_price":1453,"trade_good_trade_volume":20,"trade_good_purchase_price":2967}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-K83","created_at":"2024-09-12T14:36:44.433336+00:00","trade_good_type":"EXCHANGE","trade_good_supply":"SCARCE","trade_good_symbol":"FUEL","trade_good_activity":null,"trade_good_sell_price":69,"trade_good_trade_volume":180,"trade_good_purchase_price":76}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-K83","created_at":"2024-09-12T14:36:44.433336+00:00","trade_good_type":"IMPORT","trade_good_supply":"SCARCE","trade_good_symbol":"FERTILIZERS","trade_good_activity":"WEAK","trade_good_sell_price":254,"trade_good_trade_volume":60,"trade_good_purchase_price":514}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-K83","created_at":"2024-09-12T14:36:44.433336+00:00","trade_good_type":"EXPORT","trade_good_supply":"SCARCE","trade_good_symbol":"CLOTHING","trade_good_activity":"RESTRICTED","trade_good_sell_price":2359,"trade_good_trade_volume":43,"trade_good_purchase_price":5115}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-K83","created_at":"2024-09-12T14:36:44.433336+00:00","trade_good_type":"EXPORT","trade_good_supply":"SCARCE","trade_good_symbol":"EQUIPMENT","trade_good_activity":"RESTRICTED","trade_good_sell_price":1604,"trade_good_trade_volume":43,"trade_good_purchase_price":3559}, {"system_symbol":"X1-BA38","waypoint_symbol":"X1-BA38-K83","created_at":"2024-09-12T14:36:44.433336+00:00","trade_good_type":"IMPORT","trade_good_supply":"MODERATE","trade_good_symbol":"POLYNUCLEOTIDES","trade_good_activity":"RESTRICTED","trade_good_sell_price":255,"trade_good_trade_volume":53,"trade_good_purchase_price":530}]
        "#;

    let market_entries: Vec<MarketEntry> = serde_json::from_str(&test_data_json)?;

    Ok(market_entries)
}

fn get_waypoint_entries() -> Result<Vec<Waypoint>> {
    let test_data_json = r#"
        [{"x": 52, "y": -446, "type": "JUMP_GATE", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.741Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-I59", "traits": [{"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": true}, {"x": -729, "y": 190, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.743Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-J67", "traits": [{"name": "Mineral Deposits", "symbol": "MINERAL_DEPOSITS", "description": "Abundant mineral resources, attracting mining operations and providing valuable materials such as silicon crystals and quartz sand for various industries."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 356, "y": -130, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.743Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B37", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Explosive Gases", "symbol": "EXPLOSIVE_GASES", "description": "A volatile environment filled with highly reactive gases, posing a constant risk to those who venture too close and offering opportunities for harvesting valuable materials such as hydrocarbons."}, {"name": "Shallow Craters", "symbol": "SHALLOW_CRATERS", "description": "Numerous shallow craters, offering easier access to sub-surface resources but also creating an uneven terrain that can complicate land-based activities."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -47, "y": 69, "type": "MOON", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.851Z", "waypointSymbol": null}, "orbits": "X1-BA38-D48", "symbol": "X1-BA38-D49", "traits": [{"name": "Barren", "symbol": "BARREN", "description": "A desolate world with little to no vegetation or water, presenting unique challenges for habitation and resource extraction."}, {"name": "Scattered Settlements", "symbol": "SCATTERED_SETTLEMENTS", "description": "A collection of dispersed communities, each independent yet connected through trade and communication networks."}, {"name": "Extreme Temperatures", "symbol": "EXTREME_TEMPERATURES", "description": "A waypoint with scorching heat or freezing cold, requiring specialized equipment and technology to survive and thrive in these harsh environments."}, {"name": "Terraformed", "symbol": "TERRAFORMED", "description": "A waypoint that has been artificially transformed to support life, showcasing the engineering prowess of its inhabitants and providing a hospitable environment for colonization."}, {"name": "Industrial", "symbol": "INDUSTRIAL", "description": "A waypoint dominated by factories, refineries, and other heavy industries, often accompanied by pollution and a bustling workforce."}, {"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 23, "y": -48, "type": "MOON", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.852Z", "waypointSymbol": null}, "orbits": "X1-BA38-E50", "symbol": "X1-BA38-E51", "traits": [{"name": "Volcanic", "symbol": "VOLCANIC", "description": "A volatile world marked by intense volcanic activity, creating a hazardous environment with the potential for valuable resource extraction, such as rare metals and geothermal energy."}, {"name": "Scattered Settlements", "symbol": "SCATTERED_SETTLEMENTS", "description": "A collection of dispersed communities, each independent yet connected through trade and communication networks."}, {"name": "Vibrant Auroras", "symbol": "VIBRANT_AURORAS", "description": "A celestial light show caused by the interaction of charged particles with the waypoint's atmosphere, creating a mesmerizing spectacle and attracting tourists from across the galaxy."}, {"name": "Thin Atmosphere", "symbol": "THIN_ATMOSPHERE", "description": "A location with a sparse atmosphere, making it difficult to support life without specialized life-support systems."}, {"name": "Industrial", "symbol": "INDUSTRIAL", "description": "A waypoint dominated by factories, refineries, and other heavy industries, often accompanied by pollution and a bustling workforce."}, {"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 323, "y": -26, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.744Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B44", "traits": [{"name": "Precious Metal Deposits", "symbol": "PRECIOUS_METAL_DEPOSITS", "description": "A source of valuable metals like gold, silver, and platinum, as well as their ores, prized for their rarity, beauty, and various applications."}, {"name": "Explosive Gases", "symbol": "EXPLOSIVE_GASES", "description": "A volatile environment filled with highly reactive gases, posing a constant risk to those who venture too close and offering opportunities for harvesting valuable materials such as hydrocarbons."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -254, "y": -257, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.742Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B28", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Deep Craters", "symbol": "DEEP_CRATERS", "description": "Marked by deep, expansive craters, potentially formed by ancient meteor impacts. These formations may offer hidden resources but also pose challenges for mobility and construction."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 333, "y": -143, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.744Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B43", "traits": [{"name": "Mineral Deposits", "symbol": "MINERAL_DEPOSITS", "description": "Abundant mineral resources, attracting mining operations and providing valuable materials such as silicon crystals and quartz sand for various industries."}, {"name": "Unstable Composition", "symbol": "UNSTABLE_COMPOSITION", "description": "A location with volatile geological composition, making it prone to frequent seismic activities and necessitating specialized construction techniques for long-term habitation or activity."}, {"name": "Hollowed Interior", "symbol": "HOLLOWED_INTERIOR", "description": "A location with large hollow spaces beneath its surface, providing unique opportunities for subterranean construction and resource extraction, but also posing risks of structural instability."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 84, "y": -715, "type": "ASTEROID_BASE", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.742Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-J62", "traits": [{"name": "Hollowed Interior", "symbol": "HOLLOWED_INTERIOR", "description": "A location with large hollow spaces beneath its surface, providing unique opportunities for subterranean construction and resource extraction, but also posing risks of structural instability."}, {"name": "Pirate Base", "symbol": "PIRATE_BASE", "description": "A hidden stronghold for pirates and other outlaws, providing a safe haven for their illicit activities and a base of operations for raids and other criminal activities. You wouldn't support pirates against your faction, would you?"}, {"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -672, "y": 225, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.743Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-J68", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 350, "y": -102, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.743Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B38", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Radioactive", "symbol": "RADIOACTIVE", "description": "A hazardous location with elevated levels of radiation, requiring specialized equipment and shielding for safe habitation and exploration."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 309, "y": 61, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.743Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B39", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -305, "y": 143, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.741Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B21", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Shallow Craters", "symbol": "SHALLOW_CRATERS", "description": "Numerous shallow craters, offering easier access to sub-surface resources but also creating an uneven terrain that can complicate land-based activities."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -340, "y": 169, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.741Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B22", "traits": [{"name": "Rare Metal Deposits", "symbol": "RARE_METAL_DEPOSITS", "description": "A treasure trove of scarce metal ores such as uranite and meritium, highly sought after for their unique properties and uses."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -280, "y": 264, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.741Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B18", "traits": [{"name": "Mineral Deposits", "symbol": "MINERAL_DEPOSITS", "description": "Abundant mineral resources, attracting mining operations and providing valuable materials such as silicon crystals and quartz sand for various industries."}, {"name": "Explosive Gases", "symbol": "EXPLOSIVE_GASES", "description": "A volatile environment filled with highly reactive gases, posing a constant risk to those who venture too close and offering opportunities for harvesting valuable materials such as hydrocarbons."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 27, "y": -230, "type": "FUEL_STATION", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.741Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-I60", "traits": [{"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 0, "y": 729, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.742Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-J82", "traits": [{"name": "Precious Metal Deposits", "symbol": "PRECIOUS_METAL_DEPOSITS", "description": "A source of valuable metals like gold, silver, and platinum, as well as their ores, prized for their rarity, beauty, and various applications."}, {"name": "Deep Craters", "symbol": "DEEP_CRATERS", "description": "Marked by deep, expansive craters, potentially formed by ancient meteor impacts. These formations may offer hidden resources but also pose challenges for mobility and construction."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -299, "y": -189, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.742Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B25", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -3, "y": 45, "type": "MOON", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.855Z", "waypointSymbol": null}, "orbits": "X1-BA38-H55", "symbol": "X1-BA38-H56", "traits": [{"name": "Barren", "symbol": "BARREN", "description": "A desolate world with little to no vegetation or water, presenting unique challenges for habitation and resource extraction."}, {"name": "Scattered Settlements", "symbol": "SCATTERED_SETTLEMENTS", "description": "A collection of dispersed communities, each independent yet connected through trade and communication networks."}, {"name": "Corrosive Atmosphere", "symbol": "CORROSIVE_ATMOSPHERE", "description": "A hostile environment with an atmosphere that can rapidly degrade materials and equipment, requiring advanced engineering solutions to ensure the safety and longevity of structures and vehicles."}, {"name": "Industrial", "symbol": "INDUSTRIAL", "description": "A waypoint dominated by factories, refineries, and other heavy industries, often accompanied by pollution and a bustling workforce."}, {"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}, {"name": "Shipyard", "symbol": "SHIPYARD", "description": "A bustling hub for the construction, repair, and sale of various spacecraft, from humble shuttles to mighty warships."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 231, "y": -225, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.743Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B34", "traits": [{"name": "Mineral Deposits", "symbol": "MINERAL_DEPOSITS", "description": "Abundant mineral resources, attracting mining operations and providing valuable materials such as silicon crystals and quartz sand for various industries."}, {"name": "Unstable Composition", "symbol": "UNSTABLE_COMPOSITION", "description": "A location with volatile geological composition, making it prone to frequent seismic activities and necessitating specialized construction techniques for long-term habitation or activity."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -260, "y": 260, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.741Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B19", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 70, "y": -597, "type": "FUEL_STATION", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.741Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-J61", "traits": [{"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 398, "y": -615, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.741Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-J74", "traits": [{"name": "Mineral Deposits", "symbol": "MINERAL_DEPOSITS", "description": "Abundant mineral resources, attracting mining operations and providing valuable materials such as silicon crystals and quartz sand for various industries."}, {"name": "Radioactive", "symbol": "RADIOACTIVE", "description": "A hazardous location with elevated levels of radiation, requiring specialized equipment and shielding for safe habitation and exploration."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -262, "y": -707, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.743Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-J70", "traits": [{"name": "Precious Metal Deposits", "symbol": "PRECIOUS_METAL_DEPOSITS", "description": "A source of valuable metals like gold, silver, and platinum, as well as their ores, prized for their rarity, beauty, and various applications."}, {"name": "Shallow Craters", "symbol": "SHALLOW_CRATERS", "description": "Numerous shallow craters, offering easier access to sub-surface resources but also creating an uneven terrain that can complicate land-based activities."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -6, "y": -718, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.743Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-J71", "traits": [{"name": "Mineral Deposits", "symbol": "MINERAL_DEPOSITS", "description": "Abundant mineral resources, attracting mining operations and providing valuable materials such as silicon crystals and quartz sand for various industries."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 300, "y": -171, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.743Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B40", "traits": [{"name": "Mineral Deposits", "symbol": "MINERAL_DEPOSITS", "description": "Abundant mineral resources, attracting mining operations and providing valuable materials such as silicon crystals and quartz sand for various industries."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 328, "y": -178, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.744Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B41", "traits": [{"name": "Mineral Deposits", "symbol": "MINERAL_DEPOSITS", "description": "Abundant mineral resources, attracting mining operations and providing valuable materials such as silicon crystals and quartz sand for various industries."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 7, "y": 344, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.741Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B9", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Shallow Craters", "symbol": "SHALLOW_CRATERS", "description": "Numerous shallow craters, offering easier access to sub-surface resources but also creating an uneven terrain that can complicate land-based activities."}, {"name": "Deep Craters", "symbol": "DEEP_CRATERS", "description": "Marked by deep, expansive craters, potentially formed by ancient meteor impacts. These formations may offer hidden resources but also pose challenges for mobility and construction."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -151, "y": -31, "type": "ORBITAL_STATION", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.855Z", "waypointSymbol": null}, "orbits": "X1-BA38-C45", "symbol": "X1-BA38-C46", "traits": [{"name": "Outpost", "symbol": "OUTPOST", "description": "A small, remote settlement providing essential services and a safe haven for travelers passing through."}, {"name": "Industrial", "symbol": "INDUSTRIAL", "description": "A waypoint dominated by factories, refineries, and other heavy industries, often accompanied by pollution and a bustling workforce."}, {"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}, {"name": "Shipyard", "symbol": "SHIPYARD", "description": "A bustling hub for the construction, repair, and sale of various spacecraft, from humble shuttles to mighty warships."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 340, "y": 72, "type": "ASTEROID_BASE", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.736Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B7", "traits": [{"name": "Hollowed Interior", "symbol": "HOLLOWED_INTERIOR", "description": "A location with large hollow spaces beneath its surface, providing unique opportunities for subterranean construction and resource extraction, but also posing risks of structural instability."}, {"name": "Outpost", "symbol": "OUTPOST", "description": "A small, remote settlement providing essential services and a safe haven for travelers passing through."}, {"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 157, "y": 104, "type": "FUEL_STATION", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.733Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B6", "traits": [{"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -157, "y": 319, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.738Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B12", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 1, "y": 29, "type": "ENGINEERED_ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.738Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-DE5F", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Stripped", "symbol": "STRIPPED", "description": "A location that has been over-mined or over-harvested, resulting in depleted resources and barren landscapes."}, {"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 667, "y": 336, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.741Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-J76", "traits": [{"name": "Mineral Deposits", "symbol": "MINERAL_DEPOSITS", "description": "Abundant mineral resources, attracting mining operations and providing valuable materials such as silicon crystals and quartz sand for various industries."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -283, "y": 683, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.742Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-J64", "traits": [{"name": "Mineral Deposits", "symbol": "MINERAL_DEPOSITS", "description": "Abundant mineral resources, attracting mining operations and providing valuable materials such as silicon crystals and quartz sand for various industries."}, {"name": "Radioactive", "symbol": "RADIOACTIVE", "description": "A hazardous location with elevated levels of radiation, requiring specialized equipment and shielding for safe habitation and exploration."}, {"name": "Micro-Gravity Anomalies", "symbol": "MICRO_GRAVITY_ANOMALIES", "description": "Unpredictable gravity fields, making navigation and construction particularly challenging. These anomalies may also yield unique scientific research opportunities."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -254, "y": -202, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.742Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B26", "traits": [{"name": "Mineral Deposits", "symbol": "MINERAL_DEPOSITS", "description": "Abundant mineral resources, attracting mining operations and providing valuable materials such as silicon crystals and quartz sand for various industries."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -229, "y": -274, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.742Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B27", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -75, "y": -354, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.743Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B30", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Shallow Craters", "symbol": "SHALLOW_CRATERS", "description": "Numerous shallow craters, offering easier access to sub-surface resources but also creating an uneven terrain that can complicate land-based activities."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -158, "y": 271, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.738Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B13", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Explosive Gases", "symbol": "EXPLOSIVE_GASES", "description": "A volatile environment filled with highly reactive gases, posing a constant risk to those who venture too close and offering opportunities for harvesting valuable materials such as hydrocarbons."}, {"name": "Unstable Composition", "symbol": "UNSTABLE_COMPOSITION", "description": "A location with volatile geological composition, making it prone to frequent seismic activities and necessitating specialized construction techniques for long-term habitation or activity."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -113, "y": -23, "type": "FUEL_STATION", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.738Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-C47", "traits": [{"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -136, "y": 287, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.739Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B14", "traits": [{"name": "Mineral Deposits", "symbol": "MINERAL_DEPOSITS", "description": "Abundant mineral resources, attracting mining operations and providing valuable materials such as silicon crystals and quartz sand for various industries."}, {"name": "Unstable Composition", "symbol": "UNSTABLE_COMPOSITION", "description": "A location with volatile geological composition, making it prone to frequent seismic activities and necessitating specialized construction techniques for long-term habitation or activity."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 262, "y": -259, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.743Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B33", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Deep Craters", "symbol": "DEEP_CRATERS", "description": "Marked by deep, expansive craters, potentially formed by ancient meteor impacts. These formations may offer hidden resources but also pose challenges for mobility and construction."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 349, "y": -656, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.741Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-J75", "traits": [{"name": "Mineral Deposits", "symbol": "MINERAL_DEPOSITS", "description": "Abundant mineral resources, attracting mining operations and providing valuable materials such as silicon crystals and quartz sand for various industries."}, {"name": "Explosive Gases", "symbol": "EXPLOSIVE_GASES", "description": "A volatile environment filled with highly reactive gases, posing a constant risk to those who venture too close and offering opportunities for harvesting valuable materials such as hydrocarbons."}, {"name": "Micro-Gravity Anomalies", "symbol": "MICRO_GRAVITY_ANOMALIES", "description": "Unpredictable gravity fields, making navigation and construction particularly challenging. These anomalies may also yield unique scientific research opportunities."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 744, "y": 187, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.742Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-J77", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -382, "y": -9, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.742Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B23", "traits": [{"name": "Mineral Deposits", "symbol": "MINERAL_DEPOSITS", "description": "Abundant mineral resources, attracting mining operations and providing valuable materials such as silicon crystals and quartz sand for various industries."}, {"name": "Explosive Gases", "symbol": "EXPLOSIVE_GASES", "description": "A volatile environment filled with highly reactive gases, posing a constant risk to those who venture too close and offering opportunities for harvesting valuable materials such as hydrocarbons."}, {"name": "Deep Craters", "symbol": "DEEP_CRATERS", "description": "Marked by deep, expansive craters, potentially formed by ancient meteor impacts. These formations may offer hidden resources but also pose challenges for mobility and construction."}, {"name": "Shallow Craters", "symbol": "SHALLOW_CRATERS", "description": "Numerous shallow craters, offering easier access to sub-surface resources but also creating an uneven terrain that can complicate land-based activities."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 167, "y": -343, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.743Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B31", "traits": [{"name": "Mineral Deposits", "symbol": "MINERAL_DEPOSITS", "description": "Abundant mineral resources, attracting mining operations and providing valuable materials such as silicon crystals and quartz sand for various industries."}, {"name": "Radioactive", "symbol": "RADIOACTIVE", "description": "A hazardous location with elevated levels of radiation, requiring specialized equipment and shielding for safe habitation and exploration."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -200, "y": -319, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.742Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B29", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Shallow Craters", "symbol": "SHALLOW_CRATERS", "description": "Numerous shallow craters, offering easier access to sub-surface resources but also creating an uneven terrain that can complicate land-based activities."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 84, "y": -352, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.743Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B32", "traits": [{"name": "Mineral Deposits", "symbol": "MINERAL_DEPOSITS", "description": "Abundant mineral resources, attracting mining operations and providing valuable materials such as silicon crystals and quartz sand for various industries."}, {"name": "Radioactive", "symbol": "RADIOACTIVE", "description": "A hazardous location with elevated levels of radiation, requiring specialized equipment and shielding for safe habitation and exploration."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -115, "y": 297, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.740Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B16", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Radioactive", "symbol": "RADIOACTIVE", "description": "A hazardous location with elevated levels of radiation, requiring specialized equipment and shielding for safe habitation and exploration."}, {"name": "Explosive Gases", "symbol": "EXPLOSIVE_GASES", "description": "A volatile environment filled with highly reactive gases, posing a constant risk to those who venture too close and offering opportunities for harvesting valuable materials such as hydrocarbons."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -737, "y": 52, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.743Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-J66", "traits": [{"name": "Rare Metal Deposits", "symbol": "RARE_METAL_DEPOSITS", "description": "A treasure trove of scarce metal ores such as uranite and meritium, highly sought after for their unique properties and uses."}, {"name": "Hollowed Interior", "symbol": "HOLLOWED_INTERIOR", "description": "A location with large hollow spaces beneath its surface, providing unique opportunities for subterranean construction and resource extraction, but also posing risks of structural instability."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 25, "y": 7, "type": "PLANET", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.733Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-A1", "traits": [{"name": "Volcanic", "symbol": "VOLCANIC", "description": "A volatile world marked by intense volcanic activity, creating a hazardous environment with the potential for valuable resource extraction, such as rare metals and geothermal energy."}, {"name": "Outpost", "symbol": "OUTPOST", "description": "A small, remote settlement providing essential services and a safe haven for travelers passing through."}, {"name": "Perpetual Overcast", "symbol": "PERPETUAL_OVERCAST", "description": "A location with a constant cloud cover, resulting in a perpetually dim and shadowy environment, often impacting local weather and ecosystems."}, {"name": "Dry Seabeds", "symbol": "DRY_SEABEDS", "description": "Vast, desolate landscapes that once held oceans, now exposing the remnants of ancient marine life and providing opportunities for the discovery of valuable resources."}, {"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [{"symbol": "X1-BA38-A4"}, {"symbol": "X1-BA38-A3"}, {"symbol": "X1-BA38-A2"}], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -153, "y": 347, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.740Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B15", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Deep Craters", "symbol": "DEEP_CRATERS", "description": "Marked by deep, expansive craters, potentially formed by ancient meteor impacts. These formations may offer hidden resources but also pose challenges for mobility and construction."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 319, "y": 643, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.742Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-J79", "traits": [{"name": "Rare Metal Deposits", "symbol": "RARE_METAL_DEPOSITS", "description": "A treasure trove of scarce metal ores such as uranite and meritium, highly sought after for their unique properties and uses."}, {"name": "Explosive Gases", "symbol": "EXPLOSIVE_GASES", "description": "A volatile environment filled with highly reactive gases, posing a constant risk to those who venture too close and offering opportunities for harvesting valuable materials such as hydrocarbons."}, {"name": "Radioactive", "symbol": "RADIOACTIVE", "description": "A hazardous location with elevated levels of radiation, requiring specialized equipment and shielding for safe habitation and exploration."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -344, "y": -89, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.742Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B24", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Shallow Craters", "symbol": "SHALLOW_CRATERS", "description": "Numerous shallow craters, offering easier access to sub-surface resources but also creating an uneven terrain that can complicate land-based activities."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -710, "y": 207, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.743Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-J65", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Radioactive", "symbol": "RADIOACTIVE", "description": "A hazardous location with elevated levels of radiation, requiring specialized equipment and shielding for safe habitation and exploration."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -302, "y": -700, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.744Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-J72", "traits": [{"name": "Mineral Deposits", "symbol": "MINERAL_DEPOSITS", "description": "Abundant mineral resources, attracting mining operations and providing valuable materials such as silicon crystals and quartz sand for various industries."}, {"name": "Shallow Craters", "symbol": "SHALLOW_CRATERS", "description": "Numerous shallow craters, offering easier access to sub-surface resources but also creating an uneven terrain that can complicate land-based activities."}, {"name": "Deep Craters", "symbol": "DEEP_CRATERS", "description": "Marked by deep, expansive craters, potentially formed by ancient meteor impacts. These formations may offer hidden resources but also pose challenges for mobility and construction."}, {"name": "Debris Cluster", "symbol": "DEBRIS_CLUSTER", "description": "A region filled with hazardous debris and remnants of celestial bodies or man-made objects, requiring advanced navigational capabilities for ships passing through."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 318, "y": -117, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.744Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B42", "traits": [{"name": "Mineral Deposits", "symbol": "MINERAL_DEPOSITS", "description": "Abundant mineral resources, attracting mining operations and providing valuable materials such as silicon crystals and quartz sand for various industries."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -151, "y": -31, "type": "GAS_GIANT", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.744Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-C45", "traits": [{"name": "Vibrant Auroras", "symbol": "VIBRANT_AURORAS", "description": "A celestial light show caused by the interaction of charged particles with the waypoint's atmosphere, creating a mesmerizing spectacle and attracting tourists from across the galaxy."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [{"symbol": "X1-BA38-C46"}], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 25, "y": 7, "type": "ORBITAL_STATION", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.853Z", "waypointSymbol": null}, "orbits": "X1-BA38-A1", "symbol": "X1-BA38-A4", "traits": [{"name": "Research Facility", "symbol": "RESEARCH_FACILITY", "description": "A state-of-the-art institution dedicated to scientific research and development, often focusing on specific areas of expertise."}, {"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 281, "y": 262, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.734Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B10", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Shallow Craters", "symbol": "SHALLOW_CRATERS", "description": "Numerous shallow craters, offering easier access to sub-surface resources but also creating an uneven terrain that can complicate land-based activities."}, {"name": "Hollowed Interior", "symbol": "HOLLOWED_INTERIOR", "description": "A location with large hollow spaces beneath its surface, providing unique opportunities for subterranean construction and resource extraction, but also posing risks of structural instability."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 247, "y": 236, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.740Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B8", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Explosive Gases", "symbol": "EXPLOSIVE_GASES", "description": "A volatile environment filled with highly reactive gases, posing a constant risk to those who venture too close and offering opportunities for harvesting valuable materials such as hydrocarbons."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -63, "y": 311, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.740Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B17", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Explosive Gases", "symbol": "EXPLOSIVE_GASES", "description": "A volatile environment filled with highly reactive gases, posing a constant risk to those who venture too close and offering opportunities for harvesting valuable materials such as hydrocarbons."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -132, "y": 726, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.742Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-J63", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Shallow Craters", "symbol": "SHALLOW_CRATERS", "description": "Numerous shallow craters, offering easier access to sub-surface resources but also creating an uneven terrain that can complicate land-based activities."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 153, "y": 717, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.742Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-J80", "traits": [{"name": "Mineral Deposits", "symbol": "MINERAL_DEPOSITS", "description": "Abundant mineral resources, attracting mining operations and providing valuable materials such as silicon crystals and quartz sand for various industries."}, {"name": "Debris Cluster", "symbol": "DEBRIS_CLUSTER", "description": "A region filled with hazardous debris and remnants of celestial bodies or man-made objects, requiring advanced navigational capabilities for ships passing through."}, {"name": "Radioactive", "symbol": "RADIOACTIVE", "description": "A hazardous location with elevated levels of radiation, requiring specialized equipment and shielding for safe habitation and exploration."}, {"name": "Deep Craters", "symbol": "DEEP_CRATERS", "description": "Marked by deep, expansive craters, potentially formed by ancient meteor impacts. These formations may offer hidden resources but also pose challenges for mobility and construction."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 562, "y": 500, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.742Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-J81", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Radioactive", "symbol": "RADIOACTIVE", "description": "A hazardous location with elevated levels of radiation, requiring specialized equipment and shielding for safe habitation and exploration."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 25, "y": 7, "type": "MOON", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.849Z", "waypointSymbol": null}, "orbits": "X1-BA38-A1", "symbol": "X1-BA38-A3", "traits": [{"name": "Rocky", "symbol": "ROCKY", "description": "A world with a rugged, rocky landscape, rich in minerals and other resources, providing a variety of opportunities for mining, research, and exploration."}, {"name": "Outpost", "symbol": "OUTPOST", "description": "A small, remote settlement providing essential services and a safe haven for travelers passing through."}, {"name": "Vibrant Auroras", "symbol": "VIBRANT_AURORAS", "description": "A celestial light show caused by the interaction of charged particles with the waypoint's atmosphere, creating a mesmerizing spectacle and attracting tourists from across the galaxy."}, {"name": "Extreme Pressure", "symbol": "EXTREME_PRESSURE", "description": "A location characterized by immense atmospheric pressure, demanding robust engineering solutions and innovative approaches for exploration and resource extraction."}, {"name": "Terraformed", "symbol": "TERRAFORMED", "description": "A waypoint that has been artificially transformed to support life, showcasing the engineering prowess of its inhabitants and providing a hospitable environment for colonization."}, {"name": "Thin Atmosphere", "symbol": "THIN_ATMOSPHERE", "description": "A location with a sparse atmosphere, making it difficult to support life without specialized life-support systems."}, {"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -47, "y": 69, "type": "PLANET", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.739Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-D48", "traits": [{"name": "Rocky", "symbol": "ROCKY", "description": "A world with a rugged, rocky landscape, rich in minerals and other resources, providing a variety of opportunities for mining, research, and exploration."}, {"name": "Outpost", "symbol": "OUTPOST", "description": "A small, remote settlement providing essential services and a safe haven for travelers passing through."}, {"name": "Scarce Life", "symbol": "SCARCE_LIFE", "description": "A waypoint with sparse signs of life, often presenting unique challenges for survival and adaptation in a harsh environment."}, {"name": "Magma Seas", "symbol": "MAGMA_SEAS", "description": "A waypoint dominated by molten rock and intense heat, creating inhospitable conditions and requiring specialized technology to navigate and harvest resources."}, {"name": "Thin Atmosphere", "symbol": "THIN_ATMOSPHERE", "description": "A location with a sparse atmosphere, making it difficult to support life without specialized life-support systems."}, {"name": "Industrial", "symbol": "INDUSTRIAL", "description": "A waypoint dominated by factories, refineries, and other heavy industries, often accompanied by pollution and a bustling workforce."}, {"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [{"symbol": "X1-BA38-D49"}], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 218, "y": -242, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.743Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B35", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Radioactive", "symbol": "RADIOACTIVE", "description": "A hazardous location with elevated levels of radiation, requiring specialized equipment and shielding for safe habitation and exploration."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -663, "y": -290, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.743Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-J69", "traits": [{"name": "Mineral Deposits", "symbol": "MINERAL_DEPOSITS", "description": "Abundant mineral resources, attracting mining operations and providing valuable materials such as silicon crystals and quartz sand for various industries."}, {"name": "Shallow Craters", "symbol": "SHALLOW_CRATERS", "description": "Numerous shallow craters, offering easier access to sub-surface resources but also creating an uneven terrain that can complicate land-based activities."}, {"name": "Explosive Gases", "symbol": "EXPLOSIVE_GASES", "description": "A volatile environment filled with highly reactive gases, posing a constant risk to those who venture too close and offering opportunities for harvesting valuable materials such as hydrocarbons."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -3, "y": 45, "type": "MOON", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.854Z", "waypointSymbol": null}, "orbits": "X1-BA38-H55", "symbol": "X1-BA38-H57", "traits": [{"name": "Barren", "symbol": "BARREN", "description": "A desolate world with little to no vegetation or water, presenting unique challenges for habitation and resource extraction."}, {"name": "Outpost", "symbol": "OUTPOST", "description": "A small, remote settlement providing essential services and a safe haven for travelers passing through."}, {"name": "Vibrant Auroras", "symbol": "VIBRANT_AURORAS", "description": "A celestial light show caused by the interaction of charged particles with the waypoint's atmosphere, creating a mesmerizing spectacle and attracting tourists from across the galaxy."}, {"name": "Extreme Temperatures", "symbol": "EXTREME_TEMPERATURES", "description": "A waypoint with scorching heat or freezing cold, requiring specialized equipment and technology to survive and thrive in these harsh environments."}, {"name": "Industrial", "symbol": "INDUSTRIAL", "description": "A waypoint dominated by factories, refineries, and other heavy industries, often accompanied by pollution and a bustling workforce."}, {"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 23, "y": -48, "type": "PLANET", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.740Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-E50", "traits": [{"name": "Barren", "symbol": "BARREN", "description": "A desolate world with little to no vegetation or water, presenting unique challenges for habitation and resource extraction."}, {"name": "Outpost", "symbol": "OUTPOST", "description": "A small, remote settlement providing essential services and a safe haven for travelers passing through."}, {"name": "Salt Flats", "symbol": "SALT_FLATS", "description": "Expansive, barren plains covered in a thick layer of salt, offering unique opportunities for resource extraction, scientific research, and other activities."}, {"name": "Fossils", "symbol": "FOSSILS", "description": "A waypoint rich in the remains of ancient life, offering a valuable window into the past and the potential for scientific discovery."}, {"name": "Dry Seabeds", "symbol": "DRY_SEABEDS", "description": "Vast, desolate landscapes that once held oceans, now exposing the remnants of ancient marine life and providing opportunities for the discovery of valuable resources."}, {"name": "Industrial", "symbol": "INDUSTRIAL", "description": "A waypoint dominated by factories, refineries, and other heavy industries, often accompanied by pollution and a bustling workforce."}, {"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [{"symbol": "X1-BA38-E51"}], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -292, "y": 210, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.741Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B20", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Shallow Craters", "symbol": "SHALLOW_CRATERS", "description": "Numerous shallow craters, offering easier access to sub-surface resources but also creating an uneven terrain that can complicate land-based activities."}, {"name": "Debris Cluster", "symbol": "DEBRIS_CLUSTER", "description": "A region filled with hazardous debris and remnants of celestial bodies or man-made objects, requiring advanced navigational capabilities for ships passing through."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 153, "y": -340, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.743Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B36", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Deep Craters", "symbol": "DEEP_CRATERS", "description": "Marked by deep, expansive craters, potentially formed by ancient meteor impacts. These formations may offer hidden resources but also pose challenges for mobility and construction."}, {"name": "Radioactive", "symbol": "RADIOACTIVE", "description": "A hazardous location with elevated levels of radiation, requiring specialized equipment and shielding for safe habitation and exploration."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -3, "y": 45, "type": "MOON", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.855Z", "waypointSymbol": null}, "orbits": "X1-BA38-H55", "symbol": "X1-BA38-H58", "traits": [{"name": "Barren", "symbol": "BARREN", "description": "A desolate world with little to no vegetation or water, presenting unique challenges for habitation and resource extraction."}, {"name": "Sprawling Cities", "symbol": "SPRAWLING_CITIES", "description": "Expansive urban centers that stretch across the landscape, boasting advanced infrastructure and diverse populations."}, {"name": "Terraformed", "symbol": "TERRAFORMED", "description": "A waypoint that has been artificially transformed to support life, showcasing the engineering prowess of its inhabitants and providing a hospitable environment for colonization."}, {"name": "Toxic Atmosphere", "symbol": "TOXIC_ATMOSPHERE", "description": "A waypoint with a poisonous atmosphere, necessitating the use of specialized equipment and technology to protect inhabitants and visitors from harmful substances."}, {"name": "Industrial", "symbol": "INDUSTRIAL", "description": "A waypoint dominated by factories, refineries, and other heavy industries, often accompanied by pollution and a bustling workforce."}, {"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 104, "y": 300, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.736Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-B11", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Explosive Gases", "symbol": "EXPLOSIVE_GASES", "description": "A volatile environment filled with highly reactive gases, posing a constant risk to those who venture too close and offering opportunities for harvesting valuable materials such as hydrocarbons."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 600, "y": -501, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.744Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-J73", "traits": [{"name": "Mineral Deposits", "symbol": "MINERAL_DEPOSITS", "description": "Abundant mineral resources, attracting mining operations and providing valuable materials such as silicon crystals and quartz sand for various industries."}, {"name": "Unstable Composition", "symbol": "UNSTABLE_COMPOSITION", "description": "A location with volatile geological composition, making it prone to frequent seismic activities and necessitating specialized construction techniques for long-term habitation or activity."}, {"name": "Radioactive", "symbol": "RADIOACTIVE", "description": "A hazardous location with elevated levels of radiation, requiring specialized equipment and shielding for safe habitation and exploration."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 25, "y": 7, "type": "MOON", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.850Z", "waypointSymbol": null}, "orbits": "X1-BA38-A1", "symbol": "X1-BA38-A2", "traits": [{"name": "Barren", "symbol": "BARREN", "description": "A desolate world with little to no vegetation or water, presenting unique challenges for habitation and resource extraction."}, {"name": "Outpost", "symbol": "OUTPOST", "description": "A small, remote settlement providing essential services and a safe haven for travelers passing through."}, {"name": "Toxic Atmosphere", "symbol": "TOXIC_ATMOSPHERE", "description": "A waypoint with a poisonous atmosphere, necessitating the use of specialized equipment and technology to protect inhabitants and visitors from harmful substances."}, {"name": "Extreme Pressure", "symbol": "EXTREME_PRESSURE", "description": "A location characterized by immense atmospheric pressure, demanding robust engineering solutions and innovative approaches for exploration and resource extraction."}, {"name": "Thin Atmosphere", "symbol": "THIN_ATMOSPHERE", "description": "A location with a sparse atmosphere, making it difficult to support life without specialized life-support systems."}, {"name": "Corrosive Atmosphere", "symbol": "CORROSIVE_ATMOSPHERE", "description": "A hostile environment with an atmosphere that can rapidly degrade materials and equipment, requiring advanced engineering solutions to ensure the safety and longevity of structures and vehicles."}, {"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}, {"name": "Shipyard", "symbol": "SHIPYARD", "description": "A bustling hub for the construction, repair, and sale of various spacecraft, from humble shuttles to mighty warships."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 450, "y": 620, "type": "ASTEROID", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.742Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-J78", "traits": [{"name": "Common Metal Deposits", "symbol": "COMMON_METAL_DEPOSITS", "description": "A waypoint rich in common metal ores like iron, copper, and aluminum, essential for construction and manufacturing."}, {"name": "Deep Craters", "symbol": "DEEP_CRATERS", "description": "Marked by deep, expansive craters, potentially formed by ancient meteor impacts. These formations may offer hidden resources but also pose challenges for mobility and construction."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 71, "y": 29, "type": "PLANET", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.740Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-F52", "traits": [{"name": "Volcanic", "symbol": "VOLCANIC", "description": "A volatile world marked by intense volcanic activity, creating a hazardous environment with the potential for valuable resource extraction, such as rare metals and geothermal energy."}, {"name": "Scattered Settlements", "symbol": "SCATTERED_SETTLEMENTS", "description": "A collection of dispersed communities, each independent yet connected through trade and communication networks."}, {"name": "Scarce Life", "symbol": "SCARCE_LIFE", "description": "A waypoint with sparse signs of life, often presenting unique challenges for survival and adaptation in a harsh environment."}, {"name": "Diverse Life", "symbol": "DIVERSE_LIFE", "description": "A waypoint teeming with a wide variety of life forms, providing ample opportunities for scientific research, trade, and even tourism."}, {"name": "Strong Gravity", "symbol": "STRONG_GRAVITY", "description": "A waypoint with a powerful gravitational force, requiring specialized technology and infrastructure to support habitation and resource extraction."}, {"name": "Toxic Atmosphere", "symbol": "TOXIC_ATMOSPHERE", "description": "A waypoint with a poisonous atmosphere, necessitating the use of specialized equipment and technology to protect inhabitants and visitors from harmful substances."}, {"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [{"symbol": "X1-BA38-F53"}], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -3, "y": 45, "type": "PLANET", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.741Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-H55", "traits": [{"name": "Frozen", "symbol": "FROZEN", "description": "An ice-covered world with frigid temperatures, providing unique research opportunities and resources such as ice water, ammonia ice, and other frozen compounds."}, {"name": "Outpost", "symbol": "OUTPOST", "description": "A small, remote settlement providing essential services and a safe haven for travelers passing through."}, {"name": "Canyons", "symbol": "CANYONS", "description": "Deep, winding ravines carved into the landscape by natural forces, providing shelter for settlements and hosting diverse ecosystems."}, {"name": "Corrosive Atmosphere", "symbol": "CORROSIVE_ATMOSPHERE", "description": "A hostile environment with an atmosphere that can rapidly degrade materials and equipment, requiring advanced engineering solutions to ensure the safety and longevity of structures and vehicles."}, {"name": "Toxic Atmosphere", "symbol": "TOXIC_ATMOSPHERE", "description": "A waypoint with a poisonous atmosphere, necessitating the use of specialized equipment and technology to protect inhabitants and visitors from harmful substances."}, {"name": "Industrial", "symbol": "INDUSTRIAL", "description": "A waypoint dominated by factories, refineries, and other heavy industries, often accompanied by pollution and a bustling workforce."}, {"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [{"symbol": "X1-BA38-H58"}, {"symbol": "X1-BA38-H57"}, {"symbol": "X1-BA38-H56"}], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 71, "y": 29, "type": "ORBITAL_STATION", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.854Z", "waypointSymbol": null}, "orbits": "X1-BA38-F52", "symbol": "X1-BA38-F53", "traits": [{"name": "Frozen", "symbol": "FROZEN", "description": "An ice-covered world with frigid temperatures, providing unique research opportunities and resources such as ice water, ammonia ice, and other frozen compounds."}, {"name": "Scattered Settlements", "symbol": "SCATTERED_SETTLEMENTS", "description": "A collection of dispersed communities, each independent yet connected through trade and communication networks."}, {"name": "Thin Atmosphere", "symbol": "THIN_ATMOSPHERE", "description": "A location with a sparse atmosphere, making it difficult to support life without specialized life-support systems."}, {"name": "Corrosive Atmosphere", "symbol": "CORROSIVE_ATMOSPHERE", "description": "A hostile environment with an atmosphere that can rapidly degrade materials and equipment, requiring advanced engineering solutions to ensure the safety and longevity of structures and vehicles."}, {"name": "High-Tech", "symbol": "HIGH_TECH", "description": "A center of innovation and cutting-edge technology, driving progress and attracting skilled individuals from around the galaxy."}, {"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": -62, "y": -23, "type": "PLANET", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.740Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-G54", "traits": [{"name": "Rocky", "symbol": "ROCKY", "description": "A world with a rugged, rocky landscape, rich in minerals and other resources, providing a variety of opportunities for mining, research, and exploration."}, {"name": "Scattered Settlements", "symbol": "SCATTERED_SETTLEMENTS", "description": "A collection of dispersed communities, each independent yet connected through trade and communication networks."}, {"name": "Toxic Atmosphere", "symbol": "TOXIC_ATMOSPHERE", "description": "A waypoint with a poisonous atmosphere, necessitating the use of specialized equipment and technology to protect inhabitants and visitors from harmful substances."}, {"name": "Magma Seas", "symbol": "MAGMA_SEAS", "description": "A waypoint dominated by molten rock and intense heat, creating inhospitable conditions and requiring specialized technology to navigate and harvest resources."}, {"name": "Perpetual Overcast", "symbol": "PERPETUAL_OVERCAST", "description": "A location with a constant cloud cover, resulting in a perpetually dim and shadowy environment, often impacting local weather and ecosystems."}, {"name": "Salt Flats", "symbol": "SALT_FLATS", "description": "Expansive, barren plains covered in a thick layer of salt, offering unique opportunities for resource extraction, scientific research, and other activities."}, {"name": "Industrial", "symbol": "INDUSTRIAL", "description": "A waypoint dominated by factories, refineries, and other heavy industries, often accompanied by pollution and a bustling workforce."}, {"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}, {"x": 57, "y": -86, "type": "PLANET", "chart": {"submittedBy": "GALACTIC", "submittedOn": "2024-09-01T16:00:06.743Z", "waypointSymbol": null}, "orbits": null, "symbol": "X1-BA38-K83", "traits": [{"name": "Barren", "symbol": "BARREN", "description": "A desolate world with little to no vegetation or water, presenting unique challenges for habitation and resource extraction."}, {"name": "Outpost", "symbol": "OUTPOST", "description": "A small, remote settlement providing essential services and a safe haven for travelers passing through."}, {"name": "Dry Seabeds", "symbol": "DRY_SEABEDS", "description": "Vast, desolate landscapes that once held oceans, now exposing the remnants of ancient marine life and providing opportunities for the discovery of valuable resources."}, {"name": "Thin Atmosphere", "symbol": "THIN_ATMOSPHERE", "description": "A location with a sparse atmosphere, making it difficult to support life without specialized life-support systems."}, {"name": "Mutated Flora", "symbol": "MUTATED_FLORA", "description": "A waypoint teeming with plant life that has undergone dramatic genetic changes, creating a unique and diverse ecosystem with potential for scientific research and discovery."}, {"name": "Industrial", "symbol": "INDUSTRIAL", "description": "A waypoint dominated by factories, refineries, and other heavy industries, often accompanied by pollution and a bustling workforce."}, {"name": "Marketplace", "symbol": "MARKETPLACE", "description": "A thriving center of commerce where traders from across the galaxy gather to buy, sell, and exchange goods."}], "faction": {"symbol": "GALACTIC"}, "orbitals": [], "modifiers": [], "systemSymbol": "X1-BA38", "isUnderConstruction": false}]
        "#;

    let waypoints: Vec<Waypoint> = serde_json::from_str(&test_data_json)?;

    Ok(waypoints)
}

fn to_sc_trade_node(me: &MarketEntry, waypoint_symbol: &WaypointSymbol) -> ApiSupplyChainNode {
    ApiSupplyChainNode {
        id: node_id_of_market_entry(me, waypoint_symbol),
        trade_good_symbol: me.trade_good_symbol.clone(),
        api_supply_chain_trade_node: Some(ApiSupplyChainTradeNode {
            trade_good_supply: me.trade_good_supply.clone(),
            trade_good_activity: me.trade_good_activity.clone(),
            trade_good_type: me.trade_good_type.clone(),
            trade_volume: me.trade_good_trade_volume,
            price: me.trade_good_purchase_price,
        }),
    }
}

fn routes_to_api_supply_chain(routes: &[TradeGoodRoute]) -> ApiSupplyChain {
    let delivery_waypoints = routes
        .into_iter()
        .chunk_by(|route| {
            (
                route.delivery_waypoint.clone(),
                route.trade_good_symbol.clone(),
            )
        })
        .into_iter()
        .filter_map(
            |((delivery_waypoint, trade_good_symbol), delivery_routes)| {
                let delivery_routes = delivery_routes.collect_vec();
                if let Some(first_route) = delivery_routes.clone().get(0) {
                    let import_nodes = delivery_routes
                        .clone()
                        .into_iter()
                        .map(|route| {
                            to_sc_trade_node(
                                &route.delivery_market_entry,
                                &delivery_waypoint.symbol,
                            )
                        })
                        .collect_vec();

                    // we can pick any of the matching delivery_routes, since they should go to the same

                    let export_node = to_sc_trade_node(
                        &first_route.destination_market_entry,
                        &first_route.delivery_waypoint.symbol,
                    );

                    let destination_waypoint = ApiSupplyChainWaypoint {
                        waypoint_symbol: delivery_waypoint.symbol.clone(),
                        location: ApiSupplyChainLocation {
                            x: delivery_waypoint.x as i32,
                            y: delivery_waypoint.y as i32,
                        },
                        export_node,
                        import_nodes,
                    };

                    Some(destination_waypoint)
                } else {
                    None
                }
            },
        )
        .collect_vec();

    let raw_producer_waypoints = routes
        .into_iter()
        .filter_map(|route| {
            route
                .maybe_raw_material_source_type
                .clone()
                .map(|raw_material_source_type| ApiSupplyChainWaypoint {
                    waypoint_symbol: route.source_waypoint.symbol.clone(),
                    location: ApiSupplyChainLocation {
                        x: route.source_waypoint.x as i32,
                        y: route.source_waypoint.y as i32,
                    },
                    export_node: ApiSupplyChainNode {
                        id: node_id_of_raw_material_origin(
                            &raw_material_source_type,
                            &route.trade_good_symbol,
                            &route.source_waypoint.symbol,
                        ),
                        api_supply_chain_trade_node: None,
                        trade_good_symbol: route.trade_good_symbol.clone(),
                    },
                    import_nodes: vec![],
                })
        })
        .collect_vec();

    let waypoints = delivery_waypoints
        .into_iter()
        .chain(raw_producer_waypoints.into_iter())
        .collect_vec();

    let edges = routes
        .iter()
        .flat_map(|route| {
            let source_market_to_destination_edge = match &route.maybe_source_market_entry {
                None => {
                    vec![]
                }
                Some(source_me) => {
                    let from_id =
                        node_id_of_market_entry(&source_me, &route.source_waypoint.symbol);

                    let to_id = node_id_of_market_entry(
                        &route.delivery_market_entry,
                        &route.delivery_waypoint.symbol,
                    );
                    let distance = route.source_waypoint.distance_to(&route.delivery_waypoint);

                    if distance == 0 {
                        println!("HELLO");
                    }

                    vec![ApiSupplyChainEdge {
                        source: from_id,
                        target: to_id,
                        distance,
                    }]
                }
            };

            let raw_source_to_destination_edge = match &route.maybe_raw_material_source_type {
                None => {
                    vec![]
                }
                Some(raw_material_source_type) => {
                    let from_id = node_id_of_raw_material_origin(
                        &raw_material_source_type,
                        &route.trade_good_symbol,
                        &route.source_waypoint.symbol,
                    );
                    let to_id = node_id_of_market_entry(
                        &route.delivery_market_entry,
                        &route.delivery_waypoint.symbol,
                    );
                    let distance = route.source_waypoint.distance_to(&route.delivery_waypoint);

                    if from_id == to_id {
                        println!("HELLO");
                    }

                    vec![ApiSupplyChainEdge {
                        source: from_id,
                        target: to_id,
                        distance,
                    }]
                }
            };

            source_market_to_destination_edge
                .into_iter()
                .chain(raw_source_to_destination_edge.into_iter())
        })
        .collect_vec();

    ApiSupplyChain { waypoints, edges }
}

fn validate_routes(trade_good_routes: &[TradeGoodRoute]) {
    // for some reason we have an import market as a source market entry

    trade_good_routes
        .iter()
        .for_each(|route| match route.maybe_source_market_entry.clone() {
            None => {}
            Some(source_me) => {
                if source_me.trade_good_type == TradeGoodType::Import {
                    println!(
                        "ERROR: found route where source market entry is IMPORT: tradeGood: {}; delivery_waypoint: {}, delivery_me.waypoint: {}, destination_me.waypoint: {}, source_waypoint: {}, source_me.waypoint: {}, source_me.trade_symbol: {}",
                        route.trade_good_symbol,
                        route.delivery_waypoint.symbol.0,
                        route.delivery_market_entry.waypoint_symbol.0,
                        route.destination_market_entry.waypoint_symbol.0,
                        route.source_waypoint.symbol.0,
                        source_me.waypoint_symbol.0,
                        source_me.trade_good_symbol,

                    )
                }
            }
        });
}

fn validate_supply_chain(api_supply_chain: &ApiSupplyChain) {
    /*
    found broken entry - source market entry is IMPORT
    ERROR - edge_node_id Import of ALUMINUM_ORE at X1-BA38-DE5F not found in any of the waypoints
    ERROR - edge_node_id Import of ALUMINUM_ORE at X1-BA38-DE5F not found in any of the waypoints
    ERROR - edge_node_id Import of COPPER_ORE at X1-BA38-DE5F not found in any of the waypoints
    ERROR - edge_node_id Import of FERTILIZERS at X1-BA38-C46 not found in any of the waypoints
    ERROR - edge_node_id Import of IRON_ORE at X1-BA38-DE5F not found in any of the waypoints
    ERROR - edge_node_id Import of IRON_ORE at X1-BA38-DE5F not found in any of the waypoints
    ERROR - edge_node_id Import of LIQUID_NITROGEN at X1-BA38-C46 not found in any of the waypoints
    ERROR - edge_node_id Import of LIQUID_NITROGEN at X1-BA38-C46 not found in any of the waypoints
    ERROR - edge_node_id Import of QUARTZ_SAND at X1-BA38-H57 not found in any of the waypoints
    ERROR - edge_node_id Import of SILICON_CRYSTALS at X1-BA38-H57 not found in any of the waypoints
    ERROR - edge_node_id Import of SILICON_CRYSTALS at X1-BA38-H57 not found in any of the waypoints
    ERROR - edge_node_id Import of SILICON_CRYSTALS at X1-BA38-H57 not found in any of the waypoints
    ERROR - edge_node_id Import of SILICON_CRYSTALS at X1-BA38-H57 not found in any of the waypoints
     */

    // all the source- and target-ids of edges should exist
    api_supply_chain
        .edges
        .iter()
        .flat_map(|edge| vec![edge.source.clone(), edge.target.clone()])
        .for_each(|edge_node_id| {
            let matching_waypoints = api_supply_chain
                .waypoints
                .iter()
                .filter(|wp| {
                    wp.export_node.id == edge_node_id
                        || wp
                            .import_nodes
                            .iter()
                            .any(|import_node| import_node.id == edge_node_id)
                })
                .collect_vec();

            if matching_waypoints.is_empty() {
                println!(
                    "ERROR - edge_node_id {} not found in any of the waypoints",
                    edge_node_id
                )
            }
        })
}

fn node_id_of_raw_material_origin(
    raw_material_source_type: &RawMaterialSourceType,
    trade_good_symbol: &TradeGoodSymbol,
    waypoint_symbol: &WaypointSymbol,
) -> String {
    format!("{} of {}", raw_material_source_type, trade_good_symbol)
}

fn node_id_of_market_entry(me: &MarketEntry, waypoint_symbol: &WaypointSymbol) -> String {
    format!("{} of {}", me.trade_good_type, me.trade_good_symbol)
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ApiSupplyChain {
    waypoints: Vec<ApiSupplyChainWaypoint>,
    edges: Vec<ApiSupplyChainEdge>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ApiSupplyChainWaypoint {
    waypoint_symbol: WaypointSymbol,
    location: ApiSupplyChainLocation,
    export_node: ApiSupplyChainNode,
    import_nodes: Vec<ApiSupplyChainNode>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ApiSupplyChainLocation {
    x: i32,
    y: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ApiSupplyChainNode {
    id: String,
    trade_good_symbol: TradeGoodSymbol,
    api_supply_chain_trade_node: Option<ApiSupplyChainTradeNode>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ApiSupplyChainTradeNode {
    trade_good_supply: SupplyLevel,
    trade_good_type: TradeGoodType,
    trade_good_activity: Option<ActivityLevel>,
    trade_volume: u32,
    price: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ApiSupplyChainEdge {
    source: String,
    target: String,
    distance: u32,
}

#[cfg(test)]
mod tests {
    use crate::{
        get_market_entries, get_waypoint_entries, materialize_supply_chain, rank_supply_chain,
        validate_routes, validate_supply_chain,
    };
    use anyhow::Result;
    use flwi_spacetraders_agent::st_model::TradeGoodSymbol;
    use flwi_spacetraders_agent::supply_chain::*;
    use itertools::Itertools;

    #[tokio::test]
    async fn test_supply_chain_materialization() -> Result<()> {
        let supply_chain = read_supply_chain().await?;

        let trade_map = supply_chain.trade_map();

        let goods_of_interest = [
            TradeGoodSymbol::ADVANCED_CIRCUITRY,
            TradeGoodSymbol::FAB_MATS,
            TradeGoodSymbol::SHIP_PLATING,
            TradeGoodSymbol::SHIP_PARTS,
            TradeGoodSymbol::MICROPROCESSORS,
            TradeGoodSymbol::ELECTRONICS,
            TradeGoodSymbol::CLOTHING,
        ];
        for trade_good in goods_of_interest.clone() {
            let chain = find_complete_supply_chain(Vec::from([trade_good.clone()]), &trade_map);
            println!("\n\n## {} Supply Chain", trade_good);
            println!("{}", chain.to_mermaid());
        }

        let complete_chain = find_complete_supply_chain(Vec::from(&goods_of_interest), &trade_map);
        println!("\n\n## Complete Supply Chain");
        println!("{}", complete_chain.to_mermaid());

        let market_entries = get_market_entries()?;
        let waypoints = get_waypoint_entries()?;

        let ranked = rank_supply_chain(&complete_chain);

        let ordered: Vec<(TradeGoodSymbol, u32)> =
            ranked.into_iter().sorted_by_key(|kv| kv.1).collect();

        println!(
            "ranked supply chain sorted: \n```text\n{}\n```",
            &ordered
                .into_iter()
                .map(|(tg, rank)| format!("#{}: {}", rank, tg.to_string()))
                .join("\n")
        );

        let (all_routes, api_supply_chain) =
            materialize_supply_chain(&complete_chain, &market_entries, &waypoints);

        validate_routes(all_routes.as_slice());
        validate_supply_chain(&api_supply_chain);

        println!(
            "\n\n## all_routes: \n\n```json\n\n{}\n ```",
            serde_json::to_string_pretty(&all_routes).unwrap()
        );

        println!(
            "\n\n## api_supply_chain: \n\n```json\n\n{}\n ```",
            serde_json::to_string_pretty(&api_supply_chain).unwrap()
        );

        Ok(())
    }
}
