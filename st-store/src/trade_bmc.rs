use crate::ctx::Ctx;
use crate::{DbModelManager, DbShipTaskEntry};
use anyhow::*;
use chrono::Utc;
use sqlx::types::Json;
use st_domain::{ShipSymbol, TicketId, TradeTicket, TransactionSummary, TransactionTicketId};
use std::collections::HashMap;

pub struct TradeBmc;

impl TradeBmc {
    pub async fn upsert_ticket(
        _ctx: &Ctx,
        mm: &DbModelManager,
        ship_symbol: &ShipSymbol,
        ticket_id: &TicketId,
        trade_ticket: &TradeTicket,
        is_complete: bool,
    ) -> Result<()> {
        let now = Utc::now();
        sqlx::query!(
            r#"
insert into trade_tickets (ticket_id, ship_symbol, entry, created_at, updated_at, completed_at)
values ($1, $2, $3, $4, $5, $6)
on conflict (ticket_id) do update set entry = excluded.entry
                                    , updated_at = excluded.updated_at
                                    , completed_at = excluded.completed_at
        "#,
            ticket_id.0,
            ship_symbol.0,
            Json(trade_ticket.clone()) as _,
            now,
            now,
            is_complete.then_some(now),
        )
        .execute(mm.pool())
        .await?;

        Ok(())
    }

    pub async fn load_uncompleted_tickets(_ctx: &Ctx, mm: &DbModelManager) -> Result<HashMap<ShipSymbol, TradeTicket>> {
        let entries: Vec<DbTradeTicket> = sqlx::query_as!(
            DbTradeTicket,
            r#"
select ship_symbol
     , entry as "entry: Json<TradeTicket>"
  from trade_tickets
 where completed_at is null
        "#,
        )
        .fetch_all(mm.pool())
        .await?;

        Ok(entries.into_iter().map(|db_entry| (ShipSymbol(db_entry.ship_symbol), db_entry.entry.0)).collect())
    }

    pub async fn save_transaction_completed(_ctx: Ctx, mm: &DbModelManager, tx_summary: &TransactionSummary) -> Result<()> {
        let now = Utc::now();
        let transaction_ticket_id: TransactionTicketId = tx_summary.transaction_ticket_id.clone();
        let ticket_id = tx_summary.trade_ticket.ticket_id();
        let ship_symbol = tx_summary.ship_symbol.clone();

        let is_complete = tx_summary.trade_ticket.is_complete();

        if is_complete {
            sqlx::query!(
                r#"
update trade_tickets
set updated_at = $1
  , completed_at = $2
where ticket_id = $3
    "#,
                now,
                now,
                ticket_id.0,
            )
            .execute(mm.pool())
            .await?;
        }

        sqlx::query!(
            r#"
insert into transactions (ticket_id,
                          transaction_ticket_id,
                          total_price,
                          ship_symbol,
                          tx_summary,
                          completed_at)
values ($1, $2, $3, $4, $5, $6)
        "#,
            ticket_id.0,
            transaction_ticket_id.0,
            tx_summary.total_price,
            ship_symbol.0,
            Json(tx_summary.clone()) as _,
            now,
        )
        .execute(mm.pool())
        .await?;

        Ok(())
    }
}

struct DbTradeTicket {
    ship_symbol: String,
    entry: Json<TradeTicket>,
}
