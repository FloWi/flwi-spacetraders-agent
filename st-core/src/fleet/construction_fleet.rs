use crate::fleet::fleet::FleetAdmiral;
use anyhow::*;
use itertools::Itertools;
use st_domain::{trading, ConstructJumpGateFleetConfig, EvaluatedTradingOpportunity, Fleet, FleetDecisionFacts, Ship, ShipSymbol, ShipTask};
use st_store::{Ctx, DbModelManager, MarketBmc, SystemBmc};
use std::collections::{HashMap, HashSet};
use std::ops::Not;

pub struct ConstructJumpGateFleet;

impl ConstructJumpGateFleet {
    pub async fn compute_ship_tasks(
        admiral: &mut FleetAdmiral,
        cfg: &ConstructJumpGateFleetConfig,
        fleet: &Fleet,
        facts: &FleetDecisionFacts,
        mm: &DbModelManager,
    ) -> Result<HashMap<ShipSymbol, ShipTask>> {
        let ships: Vec<&Ship> = admiral.get_ships_of_fleet(fleet);

        let budget: u64 = admiral.get_total_budget_for_fleet(fleet);
        let my_ships = admiral.get_ships_of_fleet(fleet);
        let ship_tasks: Vec<(ShipSymbol, ShipTask)> = admiral.get_ship_tasks_of_fleet(fleet);

        let allocated_budget: u64 = admiral.get_allocated_budget_of_fleet(fleet);

        let ships_with_tasks = ship_tasks.iter().map(|(ss, _)| ss.clone()).collect::<HashSet<_>>();
        let unassigned_ships = ships.into_iter().filter(|s| ships_with_tasks.contains(&s.symbol).not()).collect_vec();

        // all ships are traders (command frigate + 4 haulers --> 200k each => 1M total)
        let reserved_for_trading = (200_000 * my_ships.len()) as i64;
        let rest_budget = (budget as i64) - reserved_for_trading;

        // if we have enough budget for purchasing construction material, we do so

        let waypoints = SystemBmc::get_waypoints_of_system(&Ctx::Anonymous, mm, &cfg.system_symbol).await?;
        let waypoint_map = waypoints.iter().map(|wp| (wp.symbol.clone(), wp)).collect::<HashMap<_, _>>();
        let latest_market_data = MarketBmc::get_latest_market_data_for_system(&Ctx::Anonymous, mm, &cfg.system_symbol).await?;

        let market_data = st_domain::trading::to_trade_goods_with_locations(&latest_market_data);
        let trading_opportunities = st_domain::trading::find_trading_opportunities(&market_data, &waypoint_map);
        let evaluated_trading_opportunities: Vec<EvaluatedTradingOpportunity> =
            st_domain::trading::evaluate_trading_opportunities(&unassigned_ships, &waypoint_map, trading_opportunities);

        // FIXME: get currently active trades
        let active_trades = Vec::new();
        let trades_for_ships = trading::find_optimal_trading_routes_exhaustive(&evaluated_trading_opportunities, &active_trades);

        dbg!(budget);
        dbg!(ship_tasks);
        dbg!(allocated_budget);
        dbg!(ships_with_tasks);
        dbg!(unassigned_ships);
        dbg!(reserved_for_trading);
        dbg!(rest_budget);
        dbg!(evaluated_trading_opportunities);
        dbg!(trades_for_ships);

        // TradingManager::acquire_trading_tickets(trading_tickets, admiral);

        Ok(Default::default())
    }
}

struct TradingManager;
