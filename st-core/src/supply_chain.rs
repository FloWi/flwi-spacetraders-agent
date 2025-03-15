use std::fs;
use std::path::Path;

use anyhow::Result;
use itertools::Itertools;
use st_domain::{MarketData, SupplyChain, TradeRelation, Waypoint};

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

pub fn materialize_supply_chain(supply_chain: SupplyChain, market_data: Vec<MarketData>, waypoints: Vec<Waypoint>) {

}
