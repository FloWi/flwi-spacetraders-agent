use crate::ctx::Ctx;
use crate::{DbModelManager, DbShipTaskEntry};
use anyhow::*;
use chrono::Utc;
use sqlx::types::Json;
use st_domain::{ShipSymbol, TicketId, TradeTicket};
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
        "#,
        )
        .fetch_all(mm.pool())
        .await?;

        Ok(entries.into_iter().map(|db_entry| (ShipSymbol(db_entry.ship_symbol), db_entry.entry.0)).collect())
    }
}

struct DbTradeTicket {
    ship_symbol: String,
    entry: Json<TradeTicket>,
}
