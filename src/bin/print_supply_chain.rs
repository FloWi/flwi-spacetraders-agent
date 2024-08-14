use std::collections::HashMap;

use anyhow::Result;

use flwi_spacetraders_agent::supply_chain::{
    find_complete_supply_chain, read_supply_chain, TradeGood,
};

#[tokio::main]
async fn main() -> Result<()> {
    let supply_chain = read_supply_chain().await?;

    println!("Complete Supply Chain");
    dbg!(supply_chain.clone());

    // Create a HashMap for easier lookup
    let trade_map: HashMap<TradeGood, Vec<TradeGood>> = supply_chain
        .relations
        .into_iter()
        .map(|relation| (relation.export, relation.imports))
        .collect();

    let advanced_circuitry_chain =
        find_complete_supply_chain(&TradeGood::new("ADVANCED_CIRCUITRY"), &trade_map);

    println!("ADVANCED_CIRCUITRY Supply Chain");
    dbg!(advanced_circuitry_chain);

    let fab_mats_chain = find_complete_supply_chain(&TradeGood::new("FAB_MATS"), &trade_map);

    println!("FAB_MATS Supply Chain");
    dbg!(fab_mats_chain);

    Ok(())
}
