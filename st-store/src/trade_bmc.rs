use crate::ctx::Ctx;
use crate::{DbModelManager, DbShipTaskEntry};
use anyhow::*;
use async_trait::async_trait;
use chrono::Utc;
use mockall::automock;
use sqlx::types::Json;
use st_domain::{ShipSymbol, TicketId, TradeTicket, TransactionSummary, TransactionTicketId};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::RwLock;

#[automock]
#[async_trait]
pub trait TradeBmcTrait: Send + Sync + Debug {
    async fn get_ticket_by_id(&self, _ctx: &Ctx, ticket_id: TicketId) -> Result<TradeTicket>;
    async fn upsert_ticket(&self, _ctx: &Ctx, ship_symbol: &ShipSymbol, ticket_id: &TicketId, trade_ticket: &TradeTicket, is_complete: bool) -> Result<()>;
    async fn load_uncompleted_tickets(&self, _ctx: &Ctx) -> Result<HashMap<ShipSymbol, TradeTicket>>;
    async fn save_transaction_completed(&self, _ctx: Ctx, tx_summary: &TransactionSummary) -> Result<()>;
}

#[derive(Debug)]
pub struct DbTradeBmc {
    pub mm: DbModelManager,
}

#[async_trait]
impl TradeBmcTrait for DbTradeBmc {
    async fn get_ticket_by_id(&self, _ctx: &Ctx, ticket_id: TicketId) -> Result<TradeTicket> {
        let db_entry: DbTradeTicket = sqlx::query_as!(
            DbTradeTicket,
            r#"
select ship_symbol
     , entry as "entry: Json<TradeTicket>"
  from trade_tickets
 where ticket_id = $1
        "#,
            ticket_id.0,
        )
        .fetch_one(self.mm.pool())
        .await?;

        Ok(db_entry.entry.0)
    }

    async fn upsert_ticket(&self, _ctx: &Ctx, ship_symbol: &ShipSymbol, ticket_id: &TicketId, trade_ticket: &TradeTicket, is_complete: bool) -> Result<()> {
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
        .execute(self.mm.pool())
        .await?;

        Ok(())
    }

    async fn load_uncompleted_tickets(&self, _ctx: &Ctx) -> Result<HashMap<ShipSymbol, TradeTicket>> {
        let entries: Vec<DbTradeTicket> = sqlx::query_as!(
            DbTradeTicket,
            r#"
select ship_symbol
     , entry as "entry: Json<TradeTicket>"
  from trade_tickets
 where completed_at is null
        "#,
        )
        .fetch_all(self.mm.pool())
        .await?;

        Ok(entries.into_iter().map(|db_entry| (ShipSymbol(db_entry.ship_symbol), db_entry.entry.0)).collect())
    }

    async fn save_transaction_completed(&self, _ctx: Ctx, tx_summary: &TransactionSummary) -> Result<()> {
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
  , entry = $3
where ticket_id = $4
    "#,
                now,
                now,
                Json(tx_summary.trade_ticket.clone()) as _,
                ticket_id.0,
            )
            .execute(self.mm.pool())
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
        .execute(self.mm.pool())
        .await?;

        Ok(())
    }
}

struct DbTradeTicket {
    ship_symbol: String,
    entry: Json<TradeTicket>,
}

#[derive(Debug)]
pub struct InMemoryTrades {
    active_trades: HashMap<ShipSymbol, TradeTicket>,
}

#[derive(Debug)]
pub struct InMemoryTradeBmc {
    in_memory_trades: Arc<RwLock<InMemoryTrades>>,
}

impl InMemoryTradeBmc {
    pub fn new() -> Self {
        Self {
            in_memory_trades: Arc::new(RwLock::new(InMemoryTrades {
                active_trades: Default::default(),
            })),
        }
    }
}

#[async_trait]
impl TradeBmcTrait for InMemoryTradeBmc {
    async fn get_ticket_by_id(&self, _ctx: &Ctx, ticket_id: TicketId) -> Result<TradeTicket> {
        todo!()
    }

    async fn upsert_ticket(&self, _ctx: &Ctx, ship_symbol: &ShipSymbol, ticket_id: &TicketId, trade_ticket: &TradeTicket, is_complete: bool) -> Result<()> {
        todo!()
    }

    async fn load_uncompleted_tickets(&self, _ctx: &Ctx) -> Result<HashMap<ShipSymbol, TradeTicket>> {
        Ok(self.in_memory_trades.read().await.active_trades.clone())
    }

    async fn save_transaction_completed(&self, _ctx: Ctx, tx_summary: &TransactionSummary) -> Result<()> {
        todo!()
    }
}
