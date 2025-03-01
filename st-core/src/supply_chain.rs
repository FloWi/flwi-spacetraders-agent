use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use anyhow::Result;
use itertools::Itertools;
use st_domain::{SupplyChain, SupplyChainNode, TradeGoodSymbol, TradeRelation};

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
            r###"```mermaid
%%{{init: {{"#flowchart": {{"htmlLabels": false}}}} }}%%
graph LR
{}
```
"###,
            connections.iter().join("\n")
        )
    }
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
pub async fn read_supply_chain() -> Result<SupplyChain> {
    // Construct the path to the JSON file
    let file_path = Path::new("assets").join("production-chain.json");
    println!("Reading supply-chain from {:?}", file_path);

    // Read the contents of the file
    let json_data = fs::read_to_string(file_path)?;

    // Parse the JSON data
    let trade_relations: Vec<TradeRelation> = serde_json::from_str(&json_data)?;

    Ok(SupplyChain {
        relations: trade_relations,
    })
}
