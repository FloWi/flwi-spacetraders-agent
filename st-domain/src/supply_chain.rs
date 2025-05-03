use crate::supply_chain::RawMaterialSourceType::{Extraction, Siphoning};
use crate::{
    ConstructionMaterial, GetConstructionResponse, GetSupplyChainResponse, LabelledCoordinate, MarketTradeGood, ShipSymbol, SupplyChainMap, TradeGoodSymbol,
    TradeGoodType, Waypoint, WaypointSymbol, WaypointType,
};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::ops::Not;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradeRelation {
    pub export: TradeGoodSymbol,
    pub imports: Vec<TradeGoodSymbol>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SupplyChain {
    pub relations: Vec<TradeRelation>,
}

// Function to convert from server format to your model
impl From<GetSupplyChainResponse> for SupplyChain {
    fn from(response: GetSupplyChainResponse) -> Self {
        let relations = response.data.export_to_import_map.into_iter().map(|(export, imports)| TradeRelation { export, imports }).collect();

        SupplyChain { relations }
    }
}

// reverse function to convert from model to server format
impl From<SupplyChain> for GetSupplyChainResponse {
    fn from(supply_chain: SupplyChain) -> Self {
        let export_to_import_map = supply_chain.relations.into_iter().map(|trade_relation| (trade_relation.export, trade_relation.imports)).collect();

        GetSupplyChainResponse {
            data: SupplyChainMap { export_to_import_map },
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SupplyChainNode {
    pub good: TradeGoodSymbol,
    pub dependencies: Vec<TradeGoodSymbol>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct MaterializedSupplyChain {
    pub explanation: String,
    pub trading_opportunities: Vec<TradingOpportunity>,
    pub raw_delivery_routes: Vec<RawDeliveryRoute>,
}

pub fn find_complete_supply_chain(products: Vec<TradeGoodSymbol>, trade_relations: &HashMap<TradeGoodSymbol, Vec<TradeGoodSymbol>>) -> Vec<SupplyChainNode> {
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
        recursive_search(&product, trade_relations, &mut visited, &mut result);
    }
    result
}

pub fn trade_map(supply_chain: &SupplyChain) -> HashMap<TradeGoodSymbol, Vec<TradeGoodSymbol>> {
    supply_chain
        .relations
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
    supply_chain: &SupplyChain,
    market_data: &[(WaypointSymbol, Vec<MarketTradeGood>)],
    waypoint_map: &HashMap<WaypointSymbol, &Waypoint>,
    maybe_construction_site: &Option<GetConstructionResponse>,
    goods_of_interest: &[TradeGoodSymbol],
) -> MaterializedSupplyChain {
    let missing_construction_materials: Vec<&ConstructionMaterial> = match maybe_construction_site {
        None => {
            vec![]
        }
        Some(construction_site) => construction_site.data.materials.iter().filter(|cm| cm.fulfilled < cm.required).collect_vec(),
    };

    let completion_explanation = missing_construction_materials
        .iter()
        .map(|cm| {
            let percent_done = cm.fulfilled as f64 / cm.required as f64 * 100.0;
            format!("{}: {:} of {:} delivered ({:.1}%)", cm.trade_symbol, cm.fulfilled, cm.required, percent_done)
        })
        .join("\n");

    let raw_delivery_routes = compute_raw_delivery_routes(market_data, waypoint_map, missing_construction_materials, goods_of_interest, supply_chain);

    MaterializedSupplyChain {
        explanation: format!(
            r#"Completion Overview:
{completion_explanation}
"#,
        ),
        trading_opportunities: crate::trading::find_trading_opportunities(market_data, waypoint_map),
        raw_delivery_routes,
    }
}

pub fn compute_raw_delivery_routes(
    market_data: &[(WaypointSymbol, Vec<MarketTradeGood>)],
    waypoint_map: &HashMap<WaypointSymbol, &Waypoint>,
    missing_construction_materials: Vec<&ConstructionMaterial>,
    goods_of_interest: &[TradeGoodSymbol],
    supply_chain: &SupplyChain,
) -> Vec<RawDeliveryRoute> {
    let trade_map = trade_map(supply_chain);
    let complete_supply_chain = find_complete_supply_chain(goods_of_interest.iter().cloned().collect_vec(), &trade_map);

    let inputs: HashSet<TradeGoodSymbol> = complete_supply_chain.iter().flat_map(|scn| scn.dependencies.iter()).unique().cloned().collect::<HashSet<_>>();
    let outputs: HashSet<TradeGoodSymbol> = complete_supply_chain.iter().map(|scn| scn.good.clone()).unique().collect::<HashSet<_>>();
    let intermediates: HashSet<TradeGoodSymbol> = inputs.intersection(&outputs).cloned().collect::<HashSet<_>>();

    /*
    SupplyChain::materialize:
    17 inputs: QUARTZ_SAND, SILICON_CRYSTALS, LIQUID_HYDROGEN, LIQUID_NITROGEN, IRON, IRON_ORE, COPPER, COPPER_ORE, ALUMINUM, ALUMINUM_ORE, FERTILIZERS, FABRICS, MACHINERY, ELECTRONICS, EQUIPMENT, MICROPROCESSORS, PLASTICS
    22 outputs: QUARTZ_SAND, SILICON_CRYSTALS, LIQUID_HYDROGEN, LIQUID_NITROGEN, ADVANCED_CIRCUITRY, IRON, IRON_ORE, COPPER, COPPER_ORE, ALUMINUM, ALUMINUM_ORE, FAB_MATS, FERTILIZERS, FABRICS, MACHINERY, ELECTRONICS, SHIP_PLATING, SHIP_PARTS, EQUIPMENT, CLOTHING, MICROPROCESSORS, PLASTICS
    17 intermediates: QUARTZ_SAND, SILICON_CRYSTALS, LIQUID_HYDROGEN, LIQUID_NITROGEN, IRON, IRON_ORE, COPPER, COPPER_ORE, ALUMINUM, ALUMINUM_ORE, FERTILIZERS, FABRICS, MACHINERY, ELECTRONICS, EQUIPMENT, MICROPROCESSORS, PLASTICS
    0 raw_materials:
    5 end_products: ADVANCED_CIRCUITRY, FAB_MATS, SHIP_PLATING, SHIP_PARTS, CLOTHING
             */

    let raw_materials = inputs.iter().filter(|t| intermediates.contains(t).not() && outputs.contains(t).not()).cloned().collect::<HashSet<_>>();
    let end_products = outputs.iter().filter(|t| intermediates.contains(t).not() && inputs.contains(t).not()).cloned().collect::<HashSet<_>>();

    let source_type_map: HashMap<TradeGoodSymbol, RawMaterialSourceType> = get_raw_material_source();
    let source_waypoints: HashMap<RawMaterialSourceType, Vec<Waypoint>> = get_sourcing_waypoints(waypoint_map);

    let raw_material_sources: Vec<RawMaterialSource> = raw_materials
        .iter()
        .map(|raw_tgs| {
            let source_type = source_type_map.get(raw_tgs).expect("source_type must be known");
            let source_waypoints = source_waypoints.get(source_type).expect("source_waypoint must be known");
            let source_waypoint_symbols = source_waypoints.iter().map(|wp| wp.symbol.clone()).collect_vec();
            RawMaterialSource {
                trade_good: raw_tgs.clone(),
                source_type: source_type.clone(),
                source_waypoint: source_waypoint_symbols.first().expect("At least one waypoint").clone(),
            }

            // raw_tgs.clone(), source_type.clone(), source_waypoint_symbols);
        })
        .collect_vec();

    let flattened_market_data: Vec<(MarketTradeGood, WaypointSymbol)> =
        market_data.iter().flat_map(|(wps, mtg_vec)| mtg_vec.iter().map(|mtg| (mtg.clone(), wps.clone()))).collect_vec();

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

            let export_markets_to_supply =
                delivery_markets_with_distances.clone().filter(|(mtg, _, _)| mtg.trade_good_type == TradeGoodType::Export).collect_vec();
            let exchange_markets = delivery_markets_with_distances.clone().filter(|(mtg, _, _)| mtg.trade_good_type == TradeGoodType::Exchange).collect_vec();
            let closest_one = delivery_markets_with_distances.min_by_key(|t| t.2).expect("at least one");

            /*
             */

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
            let source = raw_material_sources.iter().find(|rms| rms.trade_good == *raw_material).expect("RawMaterialSource").clone();
            maybe_best_one.map(|(mtg, wps, distance)| RawDeliveryRoute {
                source,
                delivery_location: wps,
                distance,
                delivery_market_entry: mtg,
            })
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

    println!("hello, breakpoint");

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
            let combined = map1.get(key).into_iter().flatten().chain(map2.get(key).into_iter().flatten()).cloned().collect();

            (key.clone(), combined)
        })
        .collect()
}
pub fn get_raw_material_source() -> HashMap<TradeGoodSymbol, RawMaterialSourceType> {
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

#[derive(Eq, Clone, PartialEq, Hash, Debug, Serialize, Deserialize)]
enum RawMaterialSourceType {
    Extraction,
    Siphoning,
}

#[derive(Eq, Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct RawDeliveryRoute {
    source: RawMaterialSource,
    delivery_location: WaypointSymbol,
    distance: u32,
    delivery_market_entry: MarketTradeGood,
}

#[derive(Eq, PartialEq, Clone, Hash, Debug, Serialize, Deserialize)]
pub struct RawMaterialSource {
    trade_good: TradeGoodSymbol,
    source_type: RawMaterialSourceType,
    source_waypoint: WaypointSymbol,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct TradingOpportunity {
    pub purchase_waypoint_symbol: WaypointSymbol,
    pub purchase_market_trade_good_entry: MarketTradeGood,
    pub sell_waypoint_symbol: WaypointSymbol,
    pub sell_market_trade_good_entry: MarketTradeGood,
    pub direct_distance: u32,
    pub profit_per_unit: u64,
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
