use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct TradeGood(pub String);

impl TradeGood {
    pub fn new(value: &str) -> TradeGood {
        TradeGood(value.to_string())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradeRelation {
    pub export: TradeGood,
    pub imports: Vec<TradeGood>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SupplyChain {
    pub relations: Vec<TradeRelation>,
}

#[derive(Debug)]
pub struct SupplyChainNode {
    good: TradeGood,
    dependencies: Vec<TradeGood>,
}

pub fn find_complete_supply_chain(
    product: &TradeGood,
    trade_relations: &HashMap<TradeGood, Vec<TradeGood>>,
) -> Vec<SupplyChainNode> {
    fn recursive_search(
        product: &TradeGood,
        trade_relations: &HashMap<TradeGood, Vec<TradeGood>>,
        visited: &mut HashSet<TradeGood>,
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
    recursive_search(product, trade_relations, &mut visited, &mut result);
    result
}
pub async fn read_supply_chain() -> Result<SupplyChain> {
    // Construct the path to the JSON file
    let file_path = Path::new("assets").join("production-chain.json");

    // Read the contents of the file
    let json_data = fs::read_to_string(file_path)?;

    // Parse the JSON data
    let trade_relations: Vec<TradeRelation> = serde_json::from_str(&json_data)?;

    Ok(SupplyChain {
        relations: trade_relations,
    })
}
