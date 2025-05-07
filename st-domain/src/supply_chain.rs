use crate::supply_chain::RawMaterialSourceType::{Extraction, Siphoning};
use crate::{
    ActivityLevel, ConstructionMaterial, GetConstructionResponse, GetSupplyChainResponse, LabelledCoordinate, MarketTradeGood, ShipSymbol, SupplyChainMap,
    SupplyLevel, SystemSymbol, TradeGoodSymbol, TradeGoodType, Waypoint, WaypointSymbol, WaypointType, MAX_ACTIVITY_LEVEL_SCORE, MAX_SUPPLY_LEVEL_SCORE,
};
use itertools::{all, Itertools};
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;
use std::ops::Not;
use strum::{Display, IntoEnumIterator};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradeRelation {
    pub export: TradeGoodSymbol,
    pub imports: Vec<TradeGoodSymbol>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SupplyChain {
    pub relations: Vec<TradeRelation>,
    pub trade_map: HashMap<TradeGoodSymbol, Vec<TradeGoodSymbol>>,
    pub individual_supply_chains: HashMap<TradeGoodSymbol, (Vec<SupplyChainNode>, HashSet<TradeGoodSymbol>)>,
}

// Function to convert from server format to your model
impl From<GetSupplyChainResponse> for SupplyChain {
    fn from(response: GetSupplyChainResponse) -> Self {
        let relations = response
            .data
            .export_to_import_map
            .into_iter()
            .map(|(export, imports)| TradeRelation { export, imports })
            .collect_vec();
        let trade_map = calc_trade_map(&relations);
        let individual_supply_chains = calc_individual_chains(&trade_map);
        SupplyChain {
            relations,
            trade_map,
            individual_supply_chains,
        }
    }
}

// reverse function to convert from model to server format
impl From<SupplyChain> for GetSupplyChainResponse {
    fn from(supply_chain: SupplyChain) -> Self {
        let export_to_import_map = supply_chain
            .relations
            .into_iter()
            .map(|trade_relation| (trade_relation.export, trade_relation.imports))
            .collect();

        GetSupplyChainResponse {
            data: SupplyChainMap { export_to_import_map },
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct SupplyChainNode {
    pub good: TradeGoodSymbol,
    pub dependencies: Vec<TradeGoodSymbol>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct MaterializedIndividualSupplyChain {
    pub trade_good: TradeGoodSymbol,
    pub total_distance: u32,
    pub all_routes: Vec<DeliveryRoute>,
}

impl MaterializedIndividualSupplyChain {
    pub fn higher_order_routes(&self) -> Vec<HigherDeliveryRoute> {
        self.all_routes
            .iter()
            .filter_map(|r| match r {
                DeliveryRoute::Raw(_) => None,
                DeliveryRoute::Processed { route, rank } => Some(route),
            })
            .cloned()
            .collect_vec()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct MaterializedSupplyChain {
    pub explanation: String,
    pub system_symbol: SystemSymbol,
    pub trading_opportunities: Vec<TradingOpportunity>,
    pub raw_delivery_routes: Vec<RawDeliveryRoute>,
    pub relevant_supply_chain: Vec<SupplyChainNode>,
    pub all_routes: Vec<DeliveryRoute>,
    pub products_for_sale: HashSet<TradeGoodSymbol>,
    pub goods_for_sale_not_conflicting_with_construction: HashSet<TradeGoodSymbol>,
    pub goods_for_sale_conflicting_with_construction: HashSet<TradeGoodSymbol>,
    pub individual_materialized_routes: HashMap<TradeGoodSymbol, MaterializedIndividualSupplyChain>,
}

pub fn get_all_goods_involved(chain: &[SupplyChainNode]) -> HashSet<TradeGoodSymbol> {
    chain
        .iter()
        .flat_map(|scn| {
            scn.dependencies
                .iter()
                .cloned()
                .chain(std::iter::once(scn.good.clone()))
        })
        .collect()
}

pub fn find_complete_supply_chain(products: &[TradeGoodSymbol], trade_map: &HashMap<TradeGoodSymbol, Vec<TradeGoodSymbol>>) -> Vec<SupplyChainNode> {
    fn recursive_search(
        product: &TradeGoodSymbol,
        trade_relations: &HashMap<TradeGoodSymbol, Vec<TradeGoodSymbol>>,
        visited: &mut HashSet<TradeGoodSymbol>,
        result: &mut Vec<SupplyChainNode>,
    ) {
        if visited.insert(product.clone()) {
            let dependencies = trade_relations.get(product).cloned().unwrap_or_default();
            if dependencies.is_empty().not() {
                result.push(SupplyChainNode {
                    good: product.clone(),
                    dependencies: dependencies.clone(),
                });
            }

            for dep in dependencies {
                recursive_search(&dep, trade_relations, visited, result);
            }
        }
    }

    let mut visited = HashSet::new();
    let mut result = Vec::new();
    for product in products {
        recursive_search(&product, trade_map, &mut visited, &mut result);
    }
    result
}

fn calc_trade_map(trade_relations: &[TradeRelation]) -> HashMap<TradeGoodSymbol, Vec<TradeGoodSymbol>> {
    trade_relations
        .iter()
        .map(|relation| (relation.export.clone(), relation.imports.clone()))
        .filter(|(exp, imp)| {
            // if the only import is MACHINERY || EXPLOSIVES, we filter it out
            match imp.as_slice() {
                [TradeGoodSymbol::EXPLOSIVES] | [TradeGoodSymbol::MACHINERY] => false,
                _ => true,
            }
        })
        .collect()
}

fn calc_individual_chains(
    trade_map: &HashMap<TradeGoodSymbol, Vec<TradeGoodSymbol>>,
) -> HashMap<TradeGoodSymbol, (Vec<SupplyChainNode>, HashSet<TradeGoodSymbol>)> {
    let all_individual_trade_good_chains: HashMap<TradeGoodSymbol, (Vec<SupplyChainNode>, HashSet<TradeGoodSymbol>)> = TradeGoodSymbol::iter()
        .map(|trade_good| {
            let chain = find_complete_supply_chain(&[trade_good.clone()], &trade_map);
            let products_involved = get_all_goods_involved(&chain);

            (trade_good.clone(), (chain, products_involved))
        })
        .collect();

    all_individual_trade_good_chains
}

pub trait SupplyChainNodeVecExt {
    fn to_mermaid_md(&self) -> String;
    fn to_mermaid(&self) -> String;
}

impl SupplyChainNodeVecExt for Vec<SupplyChainNode> {
    fn to_mermaid_md(&self) -> String {
        let mermaid_str = self.to_mermaid();
        format!(
            r###"```mermaid
{}
```
"###,
            mermaid_str
        )
    }

    fn to_mermaid(&self) -> String {
        let mut connections = Vec::new();
        for node in self {
            for dependency in &node.dependencies {
                connections.push(format!("{} --> {}", dependency, node.good));
            }
        }

        format!(
            r###"
graph LR
{}
"###,
            connections.iter().join("\n")
        )
    }
}

pub fn materialize_supply_chain(
    system_symbol: SystemSymbol,
    supply_chain: &SupplyChain,
    market_data: &[(WaypointSymbol, Vec<MarketTradeGood>)],
    waypoint_map: &HashMap<WaypointSymbol, &Waypoint>,
    maybe_construction_site: &Option<GetConstructionResponse>,
) -> MaterializedSupplyChain {
    let missing_construction_materials: Vec<&ConstructionMaterial> = match maybe_construction_site {
        None => {
            vec![]
        }
        Some(construction_site) => construction_site
            .data
            .materials
            .iter()
            .filter(|cm| cm.fulfilled < cm.required)
            .collect_vec(),
    };

    let completion_explanation = missing_construction_materials
        .iter()
        .map(|cm| {
            let percent_done = cm.fulfilled as f64 / cm.required as f64 * 100.0;
            format!("{}: {:} of {:} delivered ({:.1}%)", cm.trade_symbol, cm.fulfilled, cm.required, percent_done)
        })
        .join("\n");

    //FIXME: compute myself
    let goods_of_interest = vec![
        TradeGoodSymbol::ADVANCED_CIRCUITRY,
        TradeGoodSymbol::FAB_MATS,
        TradeGoodSymbol::SHIP_PLATING,
        TradeGoodSymbol::SHIP_PARTS,
        TradeGoodSymbol::MICROPROCESSORS,
        TradeGoodSymbol::CLOTHING,
    ];

    let products_for_sale: HashSet<TradeGoodSymbol> = market_data
        .iter()
        .flat_map(|(_, entries)| {
            entries
                .iter()
                .filter_map(|mtg| (mtg.trade_good_type == TradeGoodType::Export).then_some(mtg.symbol.clone()))
        })
        .collect();

    let raw_materials = get_raw_material_source();

    let mut individual_materialized_routes = HashMap::new();

    for p in products_for_sale
        .iter()
        .filter(|tg| raw_materials.contains_key(tg).not())
    {
        let goods_of_interest = vec![p.clone()];
        let raw_delivery_routes = compute_raw_delivery_routes(market_data, waypoint_map, &goods_of_interest, supply_chain);

        let relevant_products = goods_of_interest.iter().cloned().collect_vec();

        let relevant_supply_chain = find_complete_supply_chain(&relevant_products, &supply_chain.trade_map);

        let all_routes = compute_all_routes(&relevant_products, &raw_delivery_routes, &relevant_supply_chain, waypoint_map, market_data);

        let total_distance: u32 = all_routes
            .iter()
            .map(|r| match r {
                DeliveryRoute::Raw(r) => r.distance,
                DeliveryRoute::Processed { route, .. } => route.distance,
            })
            .sum();
        individual_materialized_routes.insert(
            p.clone(),
            MaterializedIndividualSupplyChain {
                trade_good: p.clone(),
                total_distance,
                all_routes,
            },
        );
    }

    println!("Total distances of all {} products for sale\n", products_for_sale.len());

    individual_materialized_routes
        .iter()
        .sorted_by_key(|(_, mat)| mat.total_distance)
        .for_each(|(_, mat)| {
            println!("{}; {}; {}", mat.total_distance, mat.trade_good, mat.all_routes.len());
        });

    let raw_delivery_routes = compute_raw_delivery_routes(market_data, waypoint_map, &goods_of_interest, supply_chain);

    let relevant_products = goods_of_interest
        .iter()
        .cloned()
        .chain(
            missing_construction_materials
                .iter()
                .map(|cm| cm.trade_symbol.clone()),
        )
        .unique()
        .collect_vec();

    let relevant_supply_chain = find_complete_supply_chain(&relevant_products, &supply_chain.trade_map);

    let all_routes = compute_all_routes(&relevant_products, &raw_delivery_routes, &relevant_supply_chain, waypoint_map, market_data);

    let trading_opportunities = crate::trading::find_trading_opportunities_sorted_by_profit_per_distance_unit(market_data, waypoint_map);

    // println!("\n\nTop 10 trading opportunities");
    // trading_opportunities.iter().take(10).for_each(|to| {
    //     println!(
    //         "{}; Profit: {}; Profit per distance: {}",
    //         to.sell_market_trade_good_entry.symbol, to.profit_per_unit, to.profit_per_unit_per_distance
    //     );
    // });

    let missing_construction_material_map = maybe_construction_site
        .clone()
        .map(|cs| cs.data.missing_construction_materials())
        .unwrap_or_default();

    let ConstructionRelatedTradeGoodsOverview {
        goods_for_sale_not_conflicting_with_construction,
        goods_for_sale_conflicting_with_construction,
    } = calc_construction_related_trade_good_overview(supply_chain, missing_construction_material_map, &products_for_sale);

    MaterializedSupplyChain {
        explanation: format!(
            r#"Completion Overview:
{completion_explanation}
"#,
        ),
        system_symbol,
        relevant_supply_chain,
        trading_opportunities,
        raw_delivery_routes,
        all_routes,
        products_for_sale,
        goods_for_sale_not_conflicting_with_construction,
        goods_for_sale_conflicting_with_construction,
        individual_materialized_routes,
    }
}

struct ConstructionRelatedTradeGoodsOverview {
    goods_for_sale_not_conflicting_with_construction: HashSet<TradeGoodSymbol>,
    goods_for_sale_conflicting_with_construction: HashSet<TradeGoodSymbol>,
}

fn calc_construction_related_trade_good_overview(
    supply_chain: &SupplyChain,
    missing_construction_material: HashMap<TradeGoodSymbol, u32>,
    products_for_sale: &HashSet<TradeGoodSymbol>,
) -> ConstructionRelatedTradeGoodsOverview {
    let construction_material_chains: HashMap<TradeGoodSymbol, HashSet<TradeGoodSymbol>> = missing_construction_material
        .keys()
        .filter_map(|construction_material| {
            supply_chain
                .individual_supply_chains
                .get(construction_material)
                .map(|(_, all_goods_involved)| (construction_material.clone(), all_goods_involved.clone()))
        })
        .collect();

    let goods_for_sale_not_conflicting_with_construction: HashSet<TradeGoodSymbol> = products_for_sale
        .iter()
        .filter(|tg| missing_construction_material.contains_key(tg).not())
        .cloned()
        .filter(|trade_symbol| {
            let products_involved = supply_chain
                .individual_supply_chains
                .get(trade_symbol)
                .cloned()
                .unwrap()
                .1;

            let no_conflict_with_construction_chains = construction_material_chains
                .iter()
                .all(|(construction_material, construction_products_involved)| {
                    let intersection = products_involved
                        .intersection(&construction_products_involved)
                        .collect_vec();
                    intersection.is_empty()
                });

            no_conflict_with_construction_chains
        })
        .collect();

    let goods_for_sale_conflicting_with_construction: HashSet<TradeGoodSymbol> = products_for_sale
        .difference(&goods_for_sale_not_conflicting_with_construction)
        .cloned()
        .collect::<HashSet<_>>();

    ConstructionRelatedTradeGoodsOverview {
        goods_for_sale_not_conflicting_with_construction,
        goods_for_sale_conflicting_with_construction,
    }
}

fn group_markets_by_type(
    market_data: &[(WaypointSymbol, Vec<MarketTradeGood>)],
    trade_good_type: TradeGoodType,
) -> HashMap<TradeGoodSymbol, Vec<(WaypointSymbol, MarketTradeGood)>> {
    market_data
        .iter()
        .flat_map(|(wps, entries)| {
            entries
                .iter()
                .filter(|mtg| mtg.trade_good_type == trade_good_type)
                .map(|mtg| (mtg.symbol.clone(), (wps.clone(), mtg.clone())))
        })
        .into_group_map()
}

fn compute_all_routes(
    relevant_products: &[TradeGoodSymbol],
    raw_delivery_routes: &[RawDeliveryRoute],
    relevant_supply_chain: &[SupplyChainNode],
    waypoint_map: &HashMap<WaypointSymbol, &Waypoint>,
    market_data: &[(WaypointSymbol, Vec<MarketTradeGood>)],
) -> Vec<DeliveryRoute> {
    // Note that we deliver some of the ores directly to the smelting location (e.g. COPPER_ORE --> COPPER), which means that we don't have provider-market of COPPER_ORE

    let raw_input_sources: HashMap<TradeGoodSymbol, WaypointSymbol> = raw_delivery_routes
        .iter()
        .map(|raw_route| (raw_route.source.trade_good.clone(), raw_route.source.source_waypoint.clone()))
        .collect::<HashMap<TradeGoodSymbol, WaypointSymbol>>();

    let all_products_involved = relevant_supply_chain
        .iter()
        .flat_map(|scn| {
            // Create an iterator that yields the node's good followed by its dependencies
            std::iter::once(scn.good.clone()).chain(scn.dependencies.clone().into_iter())
        })
        .unique()
        .collect::<HashSet<_>>();

    let relevant_market_data: Vec<(WaypointSymbol, Vec<MarketTradeGood>)> = market_data
        .into_iter()
        .filter_map(|(wps, market_trade_goods)| {
            let relevant_entries = market_trade_goods
                .iter()
                .filter(|mtg| all_products_involved.contains(&mtg.symbol))
                .cloned()
                .collect_vec();
            relevant_entries
                .is_empty()
                .not()
                .then_some((wps.clone(), relevant_entries))
        })
        .collect_vec();

    let export_markets: HashMap<TradeGoodSymbol, Vec<(WaypointSymbol, MarketTradeGood)>> = group_markets_by_type(&relevant_market_data, TradeGoodType::Export);
    let import_markets = group_markets_by_type(&relevant_market_data, TradeGoodType::Import);
    let exchange_markets: HashMap<TradeGoodSymbol, Vec<(WaypointSymbol, MarketTradeGood)>> =
        group_markets_by_type(&relevant_market_data, TradeGoodType::Exchange);

    // Then use it like this:
    let supply_markets = combine_maps(&export_markets, &exchange_markets);
    let consume_markets = combine_maps(&import_markets, &exchange_markets);

    // println!("market_data: {}", serde_json::to_string(&market_data).unwrap());
    // println!("relevant_market_data: {}", serde_json::to_string(&relevant_market_data).unwrap());
    // println!("raw_delivery_routes: {}", serde_json::to_string(&raw_delivery_routes).unwrap());
    // println!("raw_input_sources: {}", serde_json::to_string(&raw_input_sources).unwrap());
    // println!("all_products_involved: {}", serde_json::to_string(&all_products_involved).unwrap());
    // println!("export_markets: {}", serde_json::to_string(&export_markets).unwrap());
    // println!("import_markets: {}", serde_json::to_string(&import_markets).unwrap());
    // println!("exchange_markets: {}", serde_json::to_string(&exchange_markets).unwrap());
    // println!("supply_markets: {}", serde_json::to_string(&supply_markets).unwrap());
    // println!("consume_markets: {}", serde_json::to_string(&consume_markets).unwrap());

    // Note that we deliver some of the ores directly to the smelting location (e.g. COPPER_ORE --> COPPER), which means that we don't have provider-market of COPPER_ORE
    let mut input_sources: HashMap<TradeGoodSymbol, (WaypointSymbol, u32)> = raw_delivery_routes
        .iter()
        .flat_map(|raw_route| {
            if raw_route.export_entry.symbol == raw_route.source.trade_good {
                // we deliver the raw material to an exchange market
                vec![(raw_route.delivery_market_entry.symbol.clone(), (raw_route.delivery_location.clone(), 1))]
            } else {
                // we deliver the raw material directly to a producing market (e.g. smelter of ores)
                // This means we have already have a provider of the processed material
                vec![
                    (raw_route.delivery_market_entry.symbol.clone(), (raw_route.delivery_location.clone(), 1)),
                    (raw_route.export_entry.symbol.clone(), (raw_route.delivery_location.clone(), 2)),
                ]
            }
        })
        .collect();

    let rest: Vec<TradeGoodSymbol> = all_products_involved
        .iter()
        .filter(|tg| raw_input_sources.contains_key(tg.clone()).not() && input_sources.contains_key(tg.clone()).not())
        .cloned()
        .collect();

    //assert_eq!(rest.len() + raw_input_sources.len() + input_sources.len(), all_products_involved.len());

    let mut rest_queue = VecDeque::from_iter(rest.iter().cloned());
    let mut higher_delivery_routes = vec![];

    while let Some(candidate) = rest_queue.pop_front() {
        let node = relevant_supply_chain
            .iter()
            .find(|scn| scn.good == candidate)
            .expect(format!("Unable to find supply_chain node for candidate {}", candidate).as_str());

        let dependency_providers = node
            .dependencies
            .iter()
            .filter_map(|dependency_trade_good| {
                let maybe_market_input_source = input_sources.get(dependency_trade_good).cloned();

                maybe_market_input_source.map(|dep_wps| (dependency_trade_good.clone(), dep_wps))
            })
            .collect_vec();

        // println!(
        //     "{} has {} dependencies: {:?}. {} providers have been found {:?} ",
        //     &node.good,
        //     &node.dependencies.len(),
        //     &node.dependencies,
        //     dependency_providers.len(),
        //     &dependency_providers
        // );

        let are_all_dependencies_fulfilled = node.dependencies.len() == dependency_providers.len();
        if are_all_dependencies_fulfilled {
            let candidate_export_entries = export_markets
                .get(&candidate)
                .expect(format!("Supply market of {} should exist", candidate).as_str());
            assert_eq!(1, candidate_export_entries.len(), "We expect only one producing market of {}", candidate);
            let (candidate_export_wps, candidate_export_mtg) = candidate_export_entries.first().cloned().unwrap();

            for (dep_trade_good, (dep_wps, rank)) in dependency_providers.iter() {
                let import_entry_at_destination = consume_markets
                    .get(dep_trade_good)
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .find_map(|(wps, mtg)| (wps == &candidate_export_wps).then_some(mtg.clone()))
                    .expect(format!("An import market of {} should exist at {}", dep_trade_good, candidate_export_wps).as_str());

                let relevant_supply_markets = supply_markets
                    .get(&dep_trade_good)
                    .cloned()
                    .unwrap_or_default();

                let Some((provider_wps, providing_mtg)) = relevant_supply_markets
                    .iter()
                    .find(|(wps, export_or_exchange_mtg)| dep_wps == wps)
                    .cloned()
                else {
                    eprintln!("An export/exchange market of {} should exist at {}", dep_trade_good, dep_wps);
                    panic!("boom");
                };

                let from_wp = waypoint_map.get(&provider_wps).unwrap();
                let to_wp = waypoint_map.get(&candidate_export_wps).unwrap();

                higher_delivery_routes.push(DeliveryRoute::Processed {
                    route: HigherDeliveryRoute {
                        trade_good: dep_trade_good.clone(),
                        source_location: provider_wps.clone(),
                        source_market_entry: providing_mtg,
                        delivery_location: candidate_export_wps.clone(),
                        distance: from_wp.distance_to(to_wp),
                        delivery_market_entry: import_entry_at_destination,
                        producing_trade_good: candidate.clone(),
                        producing_market_entry: candidate_export_mtg.clone(),
                        rank: rank + 1,
                    },
                    rank: rank + 1,
                })
            }

            let rank = dependency_providers
                .iter()
                .map(|(_, (_, r))| *r)
                .max()
                .unwrap_or_default();

            input_sources.insert(candidate, (candidate_export_wps, rank + 1));

            // println!(
            //     "All {} dependencies ({:?}) of {} fulfilled",
            //     node.dependencies.len(),
            //     node.dependencies,
            //     node.good
            // );
        } else {
            // println!(
            //     "Only {} out of {} dependencies ({:?}) of {} fulfilled.",
            //     dependency_providers.len(),
            //     node.dependencies.len(),
            //     node.dependencies,
            //     node.good
            // );
            rest_queue.push_back(candidate)
        }
    }

    // println!("higher_delivery_routes: {}", serde_json::to_string(&higher_delivery_routes).unwrap());
    // println!("raw_delivery_routes: {}", serde_json::to_string(&raw_delivery_routes).unwrap());

    let all_routes = higher_delivery_routes
        .into_iter()
        .chain(
            raw_delivery_routes
                .into_iter()
                .map(|r| DeliveryRoute::Raw(r.clone())),
        )
        .collect_vec();

    // println!("all_routes: {}", serde_json::to_string(&all_routes).unwrap());

    all_routes
}

fn combine_maps<K, V>(map1: &HashMap<K, Vec<V>>, map2: &HashMap<K, Vec<V>>) -> HashMap<K, Vec<V>>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    map1.iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .chain(map2.iter().map(|(k, v)| (k.clone(), v.clone())))
        .fold(HashMap::new(), |mut acc, (k, vs)| {
            acc.entry(k).or_insert_with(Vec::new).extend(vs);
            acc
        })
}

pub fn compute_raw_delivery_routes(
    market_data: &[(WaypointSymbol, Vec<MarketTradeGood>)],
    waypoint_map: &HashMap<WaypointSymbol, &Waypoint>,
    goods_of_interest: &[TradeGoodSymbol],
    supply_chain: &SupplyChain,
) -> Vec<RawDeliveryRoute> {
    let complete_supply_chain = find_complete_supply_chain(&goods_of_interest.iter().cloned().collect_vec(), &supply_chain.trade_map);

    let inputs: HashSet<TradeGoodSymbol> = complete_supply_chain
        .iter()
        .flat_map(|scn| scn.dependencies.iter())
        .unique()
        .cloned()
        .collect::<HashSet<_>>();
    let outputs: HashSet<TradeGoodSymbol> = complete_supply_chain
        .iter()
        .map(|scn| scn.good.clone())
        .unique()
        .collect::<HashSet<_>>();
    let intermediates: HashSet<TradeGoodSymbol> = inputs
        .intersection(&outputs)
        .cloned()
        .collect::<HashSet<_>>();

    /*
    SupplyChain::materialize:
    17 inputs: QUARTZ_SAND, SILICON_CRYSTALS, LIQUID_HYDROGEN, LIQUID_NITROGEN, IRON, IRON_ORE, COPPER, COPPER_ORE, ALUMINUM, ALUMINUM_ORE, FERTILIZERS, FABRICS, MACHINERY, ELECTRONICS, EQUIPMENT, MICROPROCESSORS, PLASTICS
    22 outputs: QUARTZ_SAND, SILICON_CRYSTALS, LIQUID_HYDROGEN, LIQUID_NITROGEN, ADVANCED_CIRCUITRY, IRON, IRON_ORE, COPPER, COPPER_ORE, ALUMINUM, ALUMINUM_ORE, FAB_MATS, FERTILIZERS, FABRICS, MACHINERY, ELECTRONICS, SHIP_PLATING, SHIP_PARTS, EQUIPMENT, CLOTHING, MICROPROCESSORS, PLASTICS
    17 intermediates: QUARTZ_SAND, SILICON_CRYSTALS, LIQUID_HYDROGEN, LIQUID_NITROGEN, IRON, IRON_ORE, COPPER, COPPER_ORE, ALUMINUM, ALUMINUM_ORE, FERTILIZERS, FABRICS, MACHINERY, ELECTRONICS, EQUIPMENT, MICROPROCESSORS, PLASTICS
    0 raw_materials:
    5 end_products: ADVANCED_CIRCUITRY, FAB_MATS, SHIP_PLATING, SHIP_PARTS, CLOTHING
             */

    let raw_materials = inputs
        .iter()
        .filter(|t| intermediates.contains(t).not() && outputs.contains(t).not())
        .cloned()
        .collect::<HashSet<_>>();
    let end_products = outputs
        .iter()
        .filter(|t| intermediates.contains(t).not() && inputs.contains(t).not())
        .cloned()
        .collect::<HashSet<_>>();

    let source_type_map: HashMap<TradeGoodSymbol, RawMaterialSourceType> = get_raw_material_source();
    let source_waypoints: HashMap<RawMaterialSourceType, Vec<Waypoint>> = get_sourcing_waypoints(waypoint_map);

    let raw_material_sources: Vec<RawMaterialSource> = raw_materials
        .iter()
        .map(|raw_tgs| {
            let source_type = source_type_map
                .get(raw_tgs)
                .expect(format!("source_type of {} should be known", raw_tgs).as_str());
            let source_waypoints = source_waypoints
                .get(source_type)
                .expect("source_waypoint must be known");
            let source_waypoint_symbols = source_waypoints
                .iter()
                .map(|wp| wp.symbol.clone())
                .collect_vec();
            RawMaterialSource {
                trade_good: raw_tgs.clone(),
                source_type: source_type.clone(),
                source_waypoint: source_waypoint_symbols
                    .first()
                    .expect("At least one waypoint")
                    .clone(),
            }

            // raw_tgs.clone(), source_type.clone(), source_waypoint_symbols);
        })
        .collect_vec();

    let flattened_market_data: Vec<(MarketTradeGood, WaypointSymbol)> = market_data
        .iter()
        .flat_map(|(wps, mtg_vec)| mtg_vec.iter().map(|mtg| (mtg.clone(), wps.clone())))
        .collect_vec();

    /*
    val exchangeMarketsOfRawMaterials: Map[TradeSymbol, List[(MarketEntry, Waypoint)]]
    val marketsRequiringRawMaterials: Map[TradeSymbol, List[(MarketEntry, Waypoint)]]
     */
    let exchange_markets_of_raw_materials: HashMap<TradeGoodSymbol, Vec<(MarketTradeGood, WaypointSymbol)>> = raw_material_sources
        .iter()
        .map(|rms| {
            let raw_trade_good = rms.trade_good.clone();
            let markets = flattened_market_data
                .iter()
                .filter(|(mtg, wps)| mtg.symbol == rms.trade_good && mtg.trade_good_type == TradeGoodType::Exchange)
                .cloned()
                .collect_vec();
            (raw_trade_good, markets)
        })
        .collect();

    let markets_requiring_raw_materials: HashMap<TradeGoodSymbol, Vec<(MarketTradeGood, WaypointSymbol)>> = raw_material_sources
        .iter()
        .map(|rms| {
            let raw_trade_good = rms.trade_good.clone();
            let markets = supply_chain
                .relations
                .iter()
                .filter(|tr| tr.imports.contains(&raw_trade_good))
                .flat_map(|tr| {
                    let export_trade_symbol = tr.export.clone();
                    let export_markets = flattened_market_data
                        .iter()
                        .filter(|(mtg, wps)| mtg.symbol == export_trade_symbol && mtg.trade_good_type == TradeGoodType::Export)
                        .cloned()
                        .collect_vec();
                    export_markets
                })
                .collect_vec();

            (raw_trade_good, markets)
        })
        .collect();

    let all_delivery_destinations_of_raw_materials = merge_hashmaps(&exchange_markets_of_raw_materials, &markets_requiring_raw_materials);

    let raw_delivery_routes: Vec<RawDeliveryRoute> = all_delivery_destinations_of_raw_materials
        .iter()
        .filter_map(|(raw_material, delivery_markets)| {
            // if the closest market is an exchange, use that
            // if only one relevant market is importing, use that market
            // if more than one export requires this good, pick the closest exchange market

            let source_waypoint = raw_material_sources
                .iter()
                .find(|rms| rms.trade_good == raw_material.clone())
                .and_then(|rms| waypoint_map.get(&rms.source_waypoint))
                .expect("Should find waypoint");

            let delivery_markets_with_distances = delivery_markets.iter().map(|(mtg, wps)| {
                let waypoint = waypoint_map.get(wps).expect("Should find waypoint");
                let distance = waypoint.distance_to(source_waypoint);
                (mtg.clone(), wps.clone(), distance)
            });

            let export_markets_to_supply = delivery_markets_with_distances
                .clone()
                .filter(|(mtg, _, _)| mtg.trade_good_type == TradeGoodType::Export)
                .collect_vec();
            let exchange_markets = delivery_markets_with_distances
                .clone()
                .filter(|(mtg, _, _)| mtg.trade_good_type == TradeGoodType::Exchange)
                .collect_vec();
            let maybe_closest_one = delivery_markets_with_distances.min_by_key(|t| t.2);

            match maybe_closest_one {
                None => None,
                Some(closest_one) => {
                    let maybe_best_one: Option<(MarketTradeGood, WaypointSymbol, u32)> = if closest_one.0.trade_good_type == TradeGoodType::Exchange {
                        Some(closest_one)
                    } else if export_markets_to_supply.len() == 1 {
                        // only export market importing this good
                        export_markets_to_supply.first().cloned()
                    } else if export_markets_to_supply.len() > 1 && exchange_markets.is_empty().not() {
                        // closest exchange market
                        exchange_markets.iter().min_by_key(|t| t.2).cloned()
                    } else {
                        None
                    };
                    let source = raw_material_sources
                        .iter()
                        .find(|rms| rms.trade_good == *raw_material)
                        .expect("RawMaterialSource")
                        .clone();
                    maybe_best_one.map(|(mtg, best_wps, distance)| {
                        let (delivery_market_entry, export_entry) = match mtg.trade_good_type {
                            TradeGoodType::Export => {
                                let import_mtg_at_destination_waypoint: MarketTradeGood = market_data
                                    .iter()
                                    .find_map(|(wps, market_data_at_destination)| {
                                        if &best_wps == wps {
                                            market_data_at_destination
                                                .iter()
                                                .find(|mtg| &mtg.symbol == raw_material && mtg.trade_good_type == TradeGoodType::Import)
                                                .cloned()
                                        } else {
                                            None
                                        }
                                    })
                                    .expect(format!("Expected to find Import market for {} at {} ({})", raw_material, best_wps, mtg.symbol).as_str());
                                (import_mtg_at_destination_waypoint, mtg.clone())
                            }
                            TradeGoodType::Exchange => (mtg.clone(), mtg.clone()),
                            TradeGoodType::Import => {
                                unreachable!()
                            }
                        };
                        RawDeliveryRoute {
                            source,
                            delivery_location: best_wps,
                            distance,
                            delivery_market_entry,
                            export_entry,
                        }
                    })
                }
            }
        })
        .collect_vec();

    // println!(
    //     "SupplyChain::materialize:
    // {} inputs: {}
    //
    // {} outputs: {}
    //
    // {} intermediates: {}
    //
    // {} raw_materials: {}
    //
    // {} end_products: {}
    //
    // trade_map: {:?}
    //
    // complete_supply_chain: {:?}
    //
    // source_type_map: {:?}
    //
    // raw_material_sources: {:?}
    //
    // exchange_markets_of_raw_materials: {:?}
    //
    // markets_requiring_raw_materials: {:?}
    //
    // raw_delivery_routes: {}
    //
    // ",
    //     inputs.len(),
    //     inputs.iter().sorted().join(", "),
    //     outputs.len(),
    //     outputs.iter().sorted().join(", "),
    //     intermediates.len(),
    //     intermediates.iter().sorted().join(", "),
    //     raw_materials.len(),
    //     raw_materials.iter().sorted().join(", "),
    //     end_products.len(),
    //     end_products.iter().sorted().join(", "),
    //     &trade_map,
    //     &complete_supply_chain,
    //     source_type_map,
    //     raw_material_sources,
    //     exchange_markets_of_raw_materials,
    //     markets_requiring_raw_materials,
    //     serde_json::to_string_pretty(&raw_delivery_routes).unwrap()
    // );

    // println!("hello, breakpoint");

    raw_delivery_routes
}

fn merge_hashmaps<K, V>(map1: &HashMap<K, Vec<V>>, map2: &HashMap<K, Vec<V>>) -> HashMap<K, Vec<V>>
where
    K: Eq + std::hash::Hash + Clone,
    V: Clone,
{
    map1.keys()
        .chain(map2.keys())
        .unique()
        .map(|key| {
            let combined = map1
                .get(key)
                .into_iter()
                .flatten()
                .chain(map2.get(key).into_iter().flatten())
                .cloned()
                .collect();

            (key.clone(), combined)
        })
        .collect()
}
pub fn get_raw_material_source() -> HashMap<TradeGoodSymbol, RawMaterialSourceType> {
    use RawMaterialSourceType::*;
    use TradeGoodSymbol::*;

    HashMap::from([
        (AMMONIA_ICE, Extraction),
        (DIAMONDS, Extraction),
        (IRON_ORE, Extraction),
        (COPPER_ORE, Extraction),
        (SILICON_CRYSTALS, Extraction),
        (QUARTZ_SAND, Extraction),
        (ALUMINUM_ORE, Extraction),
        (PRECIOUS_STONES, Extraction),
        (ICE_WATER, Extraction),
        (SILVER_ORE, Extraction),
        (GOLD_ORE, Extraction),
        (PLATINUM_ORE, Extraction),
        (URANITE_ORE, Extraction),
        (LIQUID_NITROGEN, Siphoning),
        (LIQUID_HYDROGEN, Siphoning),
        (HYDROCARBON, Siphoning),
    ])
}

pub fn get_sourcing_waypoints(waypoint_map: &HashMap<WaypointSymbol, &Waypoint>) -> HashMap<RawMaterialSourceType, Vec<Waypoint>> {
    [Extraction, Siphoning]
        .into_iter()
        .map(|source| {
            let relevant_waypoints = waypoint_map
                .values()
                .filter(|wp| match source {
                    Extraction => wp.r#type == WaypointType::ENGINEERED_ASTEROID,
                    Siphoning => wp.r#type == WaypointType::GAS_GIANT,
                })
                .cloned()
                .cloned()
                .collect_vec();
            (source, relevant_waypoints.to_vec())
        })
        .collect()
}

#[derive(Eq, Clone, PartialEq, Hash, Debug, Display, Serialize, Deserialize)]
pub enum RawMaterialSourceType {
    Extraction,
    Siphoning,
}

#[derive(Eq, Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum DeliveryRoute {
    Raw(RawDeliveryRoute),
    Processed { route: HigherDeliveryRoute, rank: u32 },
}

#[derive(Eq, Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct RawDeliveryRoute {
    pub source: RawMaterialSource,
    pub delivery_location: WaypointSymbol,
    pub distance: u32,
    pub delivery_market_entry: MarketTradeGood,
    pub export_entry: MarketTradeGood,
}

#[derive(Eq, Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct HigherDeliveryRoute {
    pub trade_good: TradeGoodSymbol,
    pub source_location: WaypointSymbol,
    pub source_market_entry: MarketTradeGood,
    pub delivery_location: WaypointSymbol,
    pub distance: u32,
    pub delivery_market_entry: MarketTradeGood,
    pub producing_trade_good: TradeGoodSymbol,
    pub producing_market_entry: MarketTradeGood,
    pub rank: u32,
}

#[derive(Eq, PartialEq, Clone, Hash, Debug, Serialize, Deserialize)]
pub struct RawMaterialSource {
    pub trade_good: TradeGoodSymbol,
    pub source_type: RawMaterialSourceType,
    pub source_waypoint: WaypointSymbol,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct TradingOpportunity {
    pub purchase_waypoint_symbol: WaypointSymbol,
    pub purchase_market_trade_good_entry: MarketTradeGood,
    pub sell_waypoint_symbol: WaypointSymbol,
    pub sell_market_trade_good_entry: MarketTradeGood,
    pub direct_distance: u32,
    pub profit_per_unit: u64,
    pub profit_per_unit_per_distance: OrderedFloat<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EvaluatedTradingOpportunity {
    pub ship_symbol: ShipSymbol,
    pub distance_to_start: u32,
    pub total_distance: u32,
    pub total_profit: u64,
    pub profit_per_distance_unit: u64,
    pub units: u32,
    pub trading_opportunity: TradingOpportunity,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ScoredSupplyChainSupportRoute {
    pub tgr: HigherDeliveryRoute,
    pub priorities_of_chains_containing_this_route: Vec<u32>,
    pub source_market: MarketTradeGood,
    pub delivery_market_export_volume: i32,
    pub delivery_market_import_volume: i32,
    pub is_import_volume_too_low: bool,
    pub supply_level_at_source: SupplyLevel,
    pub activity_level_at_source: Option<ActivityLevel>,
    pub supply_level_of_import_at_destination: SupplyLevel,
    pub activity_level_of_import_at_destination: Option<ActivityLevel>,
    pub supply_level_score: i32,
    pub activity_level_score: i32,
    pub level_score: i32,
    pub max_prio_score: u32,
    pub purchase_price: i32,
    pub sell_price: i32,
    pub spread: i32,
    pub num_parallel_pickups: u32,
    pub score: i32,
}

impl ScoredSupplyChainSupportRoute {
    pub fn calc(
        tgr: &HigherDeliveryRoute,
        max_level: u32,
        individual_materialized_routes: &HashMap<TradeGoodSymbol, MaterializedIndividualSupplyChain>,
        priorities_of_products_to_boost: &HashMap<TradeGoodSymbol, u32>,
    ) -> Self {
        let delivery_market_export_volume: i32 = tgr.producing_market_entry.trade_volume;
        let delivery_market_import_volume: i32 = tgr.delivery_market_entry.trade_volume;
        let is_import_volume_too_low: bool = delivery_market_import_volume <= delivery_market_export_volume;
        let supply_level_of_import_at_destination = tgr.delivery_market_entry.supply.clone();

        let activity_level_of_import_at_destination = tgr.delivery_market_entry.activity.clone();

        let supply_level_score: i32 = calc_supply_level_demand_score(&supply_level_of_import_at_destination);
        let activity_level_score: i32 = calc_activity_level_demand_score(&supply_level_of_import_at_destination, &activity_level_of_import_at_destination);
        let level_score: i32 = max_level as i32 - tgr.rank as i32 + 1;

        let priorities_of_chains_containing_this_route = individual_materialized_routes
            .iter()
            .filter(|(_, chain)| {
                chain
                    .higher_order_routes()
                    .iter()
                    .any(|r| r.source_location == tgr.source_location && r.delivery_location == tgr.delivery_location && r.trade_good == tgr.trade_good)
            })
            .map(|chain| {
                priorities_of_products_to_boost
                    .get(chain.0)
                    .cloned()
                    .unwrap_or_default()
            })
            .unique()
            .collect_vec();

        if priorities_of_chains_containing_this_route.is_empty() {
            println!("couldn't find this chain elsewhere: \n{:?}", tgr);
        }

        let source_market = tgr.source_market_entry.clone();
        let supply_level_at_source = source_market.supply.clone();
        let activity_level_at_source = source_market.activity.clone();

        let purchase_price = source_market.purchase_price;
        let sell_price = tgr.delivery_market_entry.sell_price;

        let spread = sell_price - purchase_price;
        let is_spread_ok = spread >= -25;

        let num_parallel_pickups: u32 = match supply_level_at_source {
            SupplyLevel::Abundant => 3,
            SupplyLevel::High => 2,
            SupplyLevel::Moderate => 1,
            SupplyLevel::Limited => {
                if is_spread_ok {
                    1
                } else {
                    0
                }
            }
            SupplyLevel::Scarce => 0,
        };

        let max_prio_score = *priorities_of_chains_containing_this_route
            .iter()
            .max()
            .unwrap();

        let score = if is_spread_ok && supply_level_at_source != SupplyLevel::Scarce {
            (supply_level_score + activity_level_score) * level_score * max_prio_score as i32
        } else {
            0
        };

        ScoredSupplyChainSupportRoute {
            tgr: tgr.clone(),
            priorities_of_chains_containing_this_route,
            source_market: source_market.clone(),
            delivery_market_export_volume,
            delivery_market_import_volume,
            is_import_volume_too_low,
            supply_level_at_source,
            activity_level_at_source,
            supply_level_of_import_at_destination,
            activity_level_of_import_at_destination,
            supply_level_score,
            activity_level_score,
            level_score,
            max_prio_score,
            purchase_price,
            sell_price,
            spread,
            num_parallel_pickups,
            score,
        }
    }
}

fn calc_supply_level_demand_score(supply_level: &SupplyLevel) -> i32 {
    let supply_level_score = supply_level.clone() as i32;
    *MAX_SUPPLY_LEVEL_SCORE - supply_level_score
}

fn calc_activity_level_demand_score(supply_level_of_export_at_this_producer: &SupplyLevel, maybe_activity_level_of_import: &Option<ActivityLevel>) -> i32 {
    if *supply_level_of_export_at_this_producer == SupplyLevel::Abundant {
        0
    } else {
        maybe_activity_level_of_import
            .clone()
            .map(|level| level as i32)
            .map(|score| *MAX_ACTIVITY_LEVEL_SCORE - score)
            .unwrap_or(0)
    }
}
