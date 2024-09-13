use anyhow::Result;
use flwi_spacetraders_agent::st_model::TradeGoodSymbol;
pub use flwi_spacetraders_agent::supply_chain::*;

#[tokio::main]
async fn main() -> Result<()> {
    let supply_chain = read_supply_chain().await?;

    //println!("Complete Supply Chain");
    //dbg!(supply_chain.clone());

    let trade_map = supply_chain.trade_map();

    for trade_good in [
        TradeGoodSymbol::ADVANCED_CIRCUITRY,
        TradeGoodSymbol::FAB_MATS,
        TradeGoodSymbol::SHIP_PLATING,
        TradeGoodSymbol::MICROPROCESSORS,
        TradeGoodSymbol::CLOTHING,
    ] {
        let chain = find_complete_supply_chain(&trade_good, &trade_map);
        println!("\n\n## {} Supply Chain", trade_good);
        println!("{}", chain.to_mermaid());
    }

    Ok(())
}
