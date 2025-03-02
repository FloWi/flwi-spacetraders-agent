use crate::TradeGoodSymbol;
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
}
