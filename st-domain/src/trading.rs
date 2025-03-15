use crate::{MarketTradeGood, TradeGoodType, TradingOpportunity, Waypoint, WaypointSymbol};
use itertools::Itertools;
pub fn find_trading_opportunities(market_data: &[(WaypointSymbol, Vec<MarketTradeGood>)],
                                  waypoints: &[Waypoint],
) -> Vec<TradingOpportunity> {

    let denormalized_trade_goods_with_wp_symbols = market_data
        .iter()
        .flat_map(|(wp_symbol, market_trade_goods)| {
            market_trade_goods
                .iter()
                .map(|mtg| (wp_symbol.clone(), mtg.clone()))
        })
        .collect_vec();
    let exports = denormalized_trade_goods_with_wp_symbols
        .iter()
        .filter(|(wp_sym, mtg)| {
            mtg.trade_good_type == TradeGoodType::Export
                || mtg.trade_good_type == TradeGoodType::Exchange
        })
        .collect_vec();
    let imports = denormalized_trade_goods_with_wp_symbols
        .iter()
        .filter(|(wp_sym, mtg)| {
            mtg.trade_good_type == TradeGoodType::Import
                || mtg.trade_good_type == TradeGoodType::Exchange
        })
        .collect_vec();

    let trades_by_profit = exports
        .iter()
        .flat_map(|(export_wp, export_mtg)| {
            imports
                .iter()
                .filter(move |(import_wp, import_mtg)| {
                    export_wp != import_wp
                        && export_mtg.symbol == import_mtg.symbol
                        && export_mtg.purchase_price < import_mtg.sell_price
                })
                .map(|(import_wp, import_mtg)| TradingOpportunity {
                    purchase_waypoint_symbol: export_wp.clone(),
                    purchase_market_trade_good_entry: export_mtg.clone(),
                    sell_waypoint_symbol: import_wp.clone(),
                    sell_market_trade_good_entry: import_mtg.clone(),

                    profit: (import_mtg.sell_price - export_mtg.purchase_price) as u64,
                })
        })
        .sorted_by_key(|trading_opp| trading_opp.profit)
        .rev()
        .collect_vec();

    trades_by_profit

}
