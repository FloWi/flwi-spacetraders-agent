use anyhow::Result;
use flwi_spacetraders_agent::st_model::{
    TradeGood, TradeGoodSymbol, TradeGoodType, TransactionType, WaypointSymbol,
};
pub use flwi_spacetraders_agent::supply_chain::*;

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

    Ok(())
}

struct SupplyChainProcessingNode {
    trade_good: TradeGood,
    waypoint_symbol: WaypointSymbol,
    trade_good_type: TradeGoodType,
    //import: ImportDetails,
}

