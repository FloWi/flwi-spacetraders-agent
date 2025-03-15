use std::fs;
use std::path::Path;

use anyhow::Result;
use itertools::Itertools;
use st_domain::{ConstructionMaterial, GetConstructionResponse, MarketTradeGood, MaterializedSupplyChain, SupplyChain, TradeRelation, Waypoint, WaypointSymbol};

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

    MaterializedSupplyChain { explanation: format!(
        r#"Completion Overview:
{completion_explanation}"#
    )}
}
