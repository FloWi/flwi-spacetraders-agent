use crate::ctx::Ctx;
use crate::DbModelManager;
use anyhow::*;
use async_trait::async_trait;
use chrono::Utc;
use mockall::automock;
use sqlx::types::Json;
use st_domain::budgeting::treasury_redesign::FinanceTicket;
use st_domain::{ShipSymbol, TicketId};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::RwLock;

#[automock]
#[async_trait]
pub trait TradeBmcTrait: Send + Sync + Debug {
    async fn get_ticket_by_id(&self, _ctx: &Ctx, ticket_id: TicketId) -> Result<FinanceTicket>;
    async fn upsert_ticket(&self, _ctx: &Ctx, ship_symbol: &ShipSymbol, ticket_id: &TicketId, trade_ticket: &FinanceTicket, is_complete: bool) -> Result<()>;
    async fn load_uncompleted_tickets(&self, _ctx: &Ctx) -> Result<HashMap<ShipSymbol, FinanceTicket>>;
}

#[derive(Debug)]
pub struct DbTradeBmc {
    pub mm: DbModelManager,
}

#[async_trait]
impl TradeBmcTrait for DbTradeBmc {
    async fn get_ticket_by_id(&self, _ctx: &Ctx, ticket_id: TicketId) -> Result<FinanceTicket> {
        let db_entry: DbFinanceTicket = sqlx::query_as!(
            DbFinanceTicket,
            r#"
select ship_symbol
     , entry as "entry: Json<FinanceTicket>"
  from trade_tickets
 where ticket_id = $1
        "#,
            ticket_id.0,
        )
        .fetch_one(self.mm.pool())
        .await?;

        Ok(db_entry.entry.0)
    }

    async fn upsert_ticket(&self, _ctx: &Ctx, ship_symbol: &ShipSymbol, ticket_id: &TicketId, trade_ticket: &FinanceTicket, is_complete: bool) -> Result<()> {
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

    async fn load_uncompleted_tickets(&self, _ctx: &Ctx) -> Result<HashMap<ShipSymbol, FinanceTicket>> {
        let entries: Vec<DbFinanceTicket> = sqlx::query_as!(
            DbFinanceTicket,
            r#"
select ship_symbol
     , entry as "entry: Json<FinanceTicket>"
  from trade_tickets
 where completed_at is null
        "#,
        )
        .fetch_all(self.mm.pool())
        .await?;

        Ok(entries
            .into_iter()
            .map(|db_entry| (ShipSymbol(db_entry.ship_symbol), db_entry.entry.0))
            .collect())
    }
}

struct DbFinanceTicket {
    ship_symbol: String,
    entry: Json<FinanceTicket>,
}

#[derive(Debug)]
pub struct InMemoryTrades {
    active_trades: HashMap<ShipSymbol, FinanceTicket>,
}

#[derive(Debug)]
pub struct InMemoryTradeBmc {
    in_memory_trades: Arc<RwLock<InMemoryTrades>>,
}

impl Default for InMemoryTradeBmc {
    fn default() -> Self {
        Self::new()
    }
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
    async fn get_ticket_by_id(&self, _ctx: &Ctx, ticket_id: TicketId) -> Result<FinanceTicket> {
        self.in_memory_trades
            .read()
            .await
            .active_trades
            .iter()
            .find_map(|(_, ticket)| (ticket.ticket_id == ticket_id).then(|| ticket.clone()))
            .ok_or(anyhow!("Ticket not found"))
    }

    async fn upsert_ticket(&self, _ctx: &Ctx, ship_symbol: &ShipSymbol, _ticket_id: &TicketId, trade_ticket: &FinanceTicket, is_complete: bool) -> Result<()> {
        if is_complete {
            self.in_memory_trades
                .write()
                .await
                .active_trades
                .remove(ship_symbol);
        } else {
            self.in_memory_trades
                .write()
                .await
                .active_trades
                .insert(ship_symbol.clone(), trade_ticket.clone());
        }
        Ok(())
    }

    async fn load_uncompleted_tickets(&self, _ctx: &Ctx) -> Result<HashMap<ShipSymbol, FinanceTicket>> {
        Ok(self.in_memory_trades.read().await.active_trades.clone())
    }
}
