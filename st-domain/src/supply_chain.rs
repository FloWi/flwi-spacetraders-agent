use crate::{
    ConstructionMaterial, GetConstructionResponse, MarketTradeGood, TradeGoodSymbol, TradeGoodType,
    Waypoint, WaypointSymbol,
};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradeRelation {
    pub export: TradeGoodSymbol,
    pub imports: Vec<TradeGoodSymbol>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SupplyChain {
    pub relations: Vec<TradeRelation>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SupplyChainNode {
    pub good: TradeGoodSymbol,
    pub dependencies: Vec<TradeGoodSymbol>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MaterializedSupplyChain {
    pub explanation: String,
    pub trading_opportunities: Vec<TradingOpportunity>
}

pub fn find_complete_supply_chain(
    products: Vec<TradeGoodSymbol>,
    trade_relations: &HashMap<TradeGoodSymbol, Vec<TradeGoodSymbol>>,
) -> Vec<SupplyChainNode> {
    fn recursive_search(
        product: &TradeGoodSymbol,
        trade_relations: &HashMap<TradeGoodSymbol, Vec<TradeGoodSymbol>>,
        visited: &mut HashSet<TradeGoodSymbol>,
        result: &mut Vec<SupplyChainNode>,
    ) {
        if visited.insert(product.clone()) {
            let dependencies = trade_relations.get(product).cloned().unwrap_or_default();
            result.push(SupplyChainNode {
                good: product.clone(),
                dependencies: dependencies.clone(),
            });

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
    waypoints: &[Waypoint],
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
            format!(
                "{}: {:} of {:} delivered ({:.1}%)",
                cm.trade_symbol, cm.fulfilled, cm.required, percent_done
            )
        })
        .join("\n");



    MaterializedSupplyChain {
        explanation: format!(
            r#"Completion Overview:
{completion_explanation}
"#,
        ),
        trading_opportunities: crate::trading::find_trading_opportunities(&market_data, &waypoints)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TradingOpportunity {
    pub purchase_waypoint_symbol: WaypointSymbol,
    pub purchase_market_trade_good_entry: MarketTradeGood,
    pub sell_waypoint_symbol: WaypointSymbol,
    pub sell_market_trade_good_entry: MarketTradeGood,
    pub profit: u64,
}
