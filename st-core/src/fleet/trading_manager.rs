use st_domain::{Ship, TradeTicket, TransactionActionEvent, TransactionSummary};
use st_store::trade_bmc::TradeBmc;
use st_store::{Ctx, DbModelManager};

pub struct TradingManager;

impl TradingManager {
    pub async fn log_transaction_completed(
        ctx: Ctx,
        mm: &DbModelManager,
        ship: &Ship,
        transaction_action_event: &TransactionActionEvent,
        trade_ticket: &TradeTicket,
    ) -> anyhow::Result<TransactionSummary> {
        let total_price: i64 = match transaction_action_event.clone() {
            TransactionActionEvent::PurchasedTradeGoods(_, resp) => -resp.data.transaction.total_price as i64,
            TransactionActionEvent::SoldTradeGoods(_, resp) => resp.data.transaction.total_price as i64,
            TransactionActionEvent::SuppliedConstructionSite(_, _) => 0,
            TransactionActionEvent::ShipPurchased(_, resp) => -(resp.data.transaction.price as i64),
        };

        let transaction_ticket_id = transaction_action_event.transaction_ticket_id();
        let tx_summary = TransactionSummary {
            ship_symbol: ship.symbol.clone(),
            transaction_action_event: transaction_action_event.clone(),
            trade_ticket: trade_ticket.clone(),
            total_price,
            transaction_ticket_id,
        };

        TradeBmc::save_transaction_completed(ctx, mm, &tx_summary).await?;
        anyhow::Ok(tx_summary)
    }
}
