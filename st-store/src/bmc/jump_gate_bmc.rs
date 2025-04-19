use crate::{db, Ctx, DbModelManager};
use anyhow::*;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use st_domain::{JumpGate, JumpGateEntry, SystemSymbol, WaypointSymbol};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::RwLock;

#[async_trait]
pub trait JumpGateBmcTrait: Send + Sync + Debug {
    async fn save_jump_gate_data(&self, ctx: &Ctx, jump_gate: JumpGate, now: DateTime<Utc>) -> Result<()>;
}

#[derive(Debug)]
pub struct DbJumpGateBmc {
    pub(crate) mm: DbModelManager,
}

#[async_trait]
impl JumpGateBmcTrait for DbJumpGateBmc {
    async fn save_jump_gate_data(&self, ctx: &Ctx, jump_gate: JumpGate, now: DateTime<Utc>) -> Result<()> {
        db::insert_jump_gates(self.mm.pool(), vec![jump_gate], now).await
    }
}

#[derive(Debug)]
pub struct InMemoryJumpGates {
    jump_gates: HashMap<SystemSymbol, HashMap<WaypointSymbol, JumpGateEntry>>,
}

impl InMemoryJumpGates {
    fn new() -> Self {
        Self {
            jump_gates: Default::default(),
        }
    }
}

#[derive(Debug)]
pub struct InMemoryJumpGateBmc {
    in_memory_jump_gates: Arc<RwLock<InMemoryJumpGates>>,
}

impl Default for InMemoryJumpGateBmc {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryJumpGateBmc {
    pub fn new() -> Self {
        Self {
            in_memory_jump_gates: Arc::new(RwLock::new(InMemoryJumpGates::new())),
        }
    }
}

#[async_trait]
impl JumpGateBmcTrait for InMemoryJumpGateBmc {
    async fn save_jump_gate_data(&self, ctx: &Ctx, jump_gate: JumpGate, now: DateTime<Utc>) -> Result<()> {
        let mut guard = self.in_memory_jump_gates.write().await;

        guard
            .jump_gates
            .entry(jump_gate.symbol.system_symbol())
            .or_default()
            .entry(jump_gate.symbol.clone())
            .and_modify(|old| {
                old.jump_gate = jump_gate.clone();
                old.updated_at = now;
            })
            .or_insert(JumpGateEntry {
                system_symbol: jump_gate.symbol.system_symbol(),
                waypoint_symbol: jump_gate.symbol.clone(),
                jump_gate,
                created_at: now,
                updated_at: now,
            });

        Ok(())
    }
}
