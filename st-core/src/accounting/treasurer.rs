#[cfg(test)]
mod tests {
    use st_domain::budgeting::budgeting::*;
    use st_domain::budgeting::credits::*;

    use crate::fleet::fleet;
    use crate::fleet::fleet::{collect_fleet_decision_facts, FleetAdmiral};
    use crate::fleet::fleet_runner::FleetRunner;
    use crate::in_memory_universe::in_memory_test_universe;
    use crate::st_client::StClientTrait;
    use anyhow::Result;
    use chrono::{Duration, Utc};
    use itertools::{assert_equal, Itertools};
    use st_domain::budgeting::budgeting::{FinanceError, FundingSource, TicketType, TransactionEvent, TransactionGoal};
    use st_domain::budgeting::credits::Credits;
    use st_domain::budgeting::treasurer::{InMemoryTreasurer, Treasurer};
    use st_domain::{Fleet, FleetId, FleetTask, ShipSymbol, ShipType, TradeGoodSymbol, TransactionTicketId, WaypointSymbol};
    use st_store::bmc::Bmc;
    use st_store::Ctx;
    use std::collections::HashSet;
    use std::sync::Arc;
    use test_log::test;

    #[test(tokio::test)]
    //#[tokio::test] // for accessing runtime-infos with tokio-conso&le
    async fn distribute_budget_among_fleets_based_for_initial_exploration_fleet_phase() -> Result<()> {
        let (bmc, client) = in_memory_test_universe::get_test_universe().await;
        let agent = client.get_agent().await?.data;
        let system_symbol = agent.headquarters.system_symbol();

        let mut finance = InMemoryTreasurer::new(Credits::new(agent.credits));

        FleetRunner::load_and_store_initial_data_in_bmcs(Arc::clone(&client), Arc::clone(&bmc))
            .await
            .expect("FleetRunner::load_and_store_initial_data");

        let facts = collect_fleet_decision_facts(Arc::clone(&bmc), &system_symbol).await?;

        let marketplaces_of_interest: HashSet<WaypointSymbol> = HashSet::from_iter(facts.marketplaces_of_interest.iter().cloned());
        let shipyards_of_interest: HashSet<WaypointSymbol> = HashSet::from_iter(facts.shipyards_of_interest.iter().cloned());
        let marketplaces_ex_shipyards: Vec<WaypointSymbol> = marketplaces_of_interest
            .difference(&shipyards_of_interest)
            .cloned()
            .collect_vec();

        let fleet_phase = fleet::create_initial_exploration_fleet_phase(&system_symbol, shipyards_of_interest.len());
        // let fleet_phase = fleet::create_construction_fleet_phase(&system_symbol, facts.shipyards_of_interest.len(), marketplaces_ex_shipyards.len());

        let (fleets, fleet_tasks): (Vec<Fleet>, Vec<(FleetId, FleetTask)>) =
            fleet::compute_fleets_with_tasks(&facts, &Default::default(), &Default::default(), &fleet_phase);

        let ship_map = facts
            .ships
            .iter()
            .map(|s| (s.symbol.clone(), s.clone()))
            .collect();

        let ship_price_info = bmc
            .shipyard_bmc()
            .get_latest_ship_prices(&Ctx::Anonymous, &system_symbol)
            .await?;

        let ship_fleet_assignment = FleetAdmiral::assign_ships(&fleet_tasks, &ship_map, &fleet_phase.shopping_list_in_order);

        let command_ship_fleet_id = fleet_tasks
            .iter()
            .find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::InitialExploration { .. }).then_some(id))
            .unwrap();

        let market_observation_fleet_id = fleet_tasks
            .iter()
            .find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::ObserveAllWaypointsOfSystemWithStationaryProbes { .. }).then_some(id))
            .unwrap();

        let all_next_ship_purchases = fleet::get_all_next_ship_purchases(&ship_map, &fleet_phase);

        finance.redistribute_distribute_fleet_budgets(&fleet_phase, &fleet_tasks, &ship_fleet_assignment, &ship_price_info, &all_next_ship_purchases)?;
        let command_fleet_budget = finance.get_fleet_budget(command_ship_fleet_id)?;
        let market_observation_fleet_budget = finance.get_fleet_budget(market_observation_fleet_id)?;

        assert_eq!(Credits::new(0), command_fleet_budget.total_capital);
        assert_eq!(Credits::new(25_000), command_fleet_budget.operating_reserve);
        assert_eq!(Credits::new(0), market_observation_fleet_budget.total_capital);

        assert_eq!(Credits::new(150_000), finance.treasury);

        Ok(())
    }

    #[test(tokio::test)]
    //#[tokio::test] // for accessing runtime-infos with tokio-conso&le
    async fn distribute_budget_among_fleets_based_for_create_construction_fleet_phase() -> Result<()> {
        let (bmc, client) = in_memory_test_universe::get_test_universe().await;
        let agent = client.get_agent().await?.data;
        let system_symbol = agent.headquarters.system_symbol();

        let mut finance = InMemoryTreasurer::new(Credits::new(agent.credits));

        FleetRunner::load_and_store_initial_data_in_bmcs(Arc::clone(&client), Arc::clone(&bmc))
            .await
            .expect("FleetRunner::load_and_store_initial_data");

        let facts = collect_fleet_decision_facts(Arc::clone(&bmc), &system_symbol).await?;

        let marketplaces_of_interest: HashSet<WaypointSymbol> = HashSet::from_iter(facts.marketplaces_of_interest.iter().cloned());
        let shipyards_of_interest: HashSet<WaypointSymbol> = HashSet::from_iter(facts.shipyards_of_interest.iter().cloned());
        let marketplaces_ex_shipyards: Vec<WaypointSymbol> = marketplaces_of_interest
            .difference(&shipyards_of_interest)
            .cloned()
            .collect_vec();

        let fleet_phase = fleet::create_construction_fleet_phase(&system_symbol, facts.shipyards_of_interest.len(), marketplaces_ex_shipyards.len());

        let (fleets, fleet_tasks): (Vec<Fleet>, Vec<(FleetId, FleetTask)>) =
            fleet::compute_fleets_with_tasks(&facts, &Default::default(), &Default::default(), &fleet_phase);

        let ship_map = facts
            .ships
            .iter()
            .map(|s| (s.symbol.clone(), s.clone()))
            .collect();

        let ship_price_info = bmc
            .shipyard_bmc()
            .get_latest_ship_prices(&Ctx::Anonymous, &system_symbol)
            .await?;

        let ship_fleet_assignment = FleetAdmiral::assign_ships(&fleet_tasks, &ship_map, &fleet_phase.shopping_list_in_order);

        let construction_fleet_id = fleet_tasks
            .iter()
            .find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::ConstructJumpGate { .. }).then_some(id))
            .unwrap();

        let market_observation_fleet_id = fleet_tasks
            .iter()
            .find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::ObserveAllWaypointsOfSystemWithStationaryProbes { .. }).then_some(id))
            .unwrap();

        let mining_fleet_id = fleet_tasks
            .iter()
            .find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::MineOres { .. }).then_some(id))
            .unwrap();

        let siphoning_fleet_id = fleet_tasks
            .iter()
            .find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::SiphonGases { .. }).then_some(id))
            .unwrap();

        let all_next_ship_purchases = fleet::get_all_next_ship_purchases(&ship_map, &fleet_phase);

        finance.redistribute_distribute_fleet_budgets(&fleet_phase, &fleet_tasks, &ship_fleet_assignment, &ship_price_info, &all_next_ship_purchases)?;
        let construction_fleet_budget = finance.get_fleet_budget(construction_fleet_id)?;
        let market_observation_fleet_budget = finance.get_fleet_budget(market_observation_fleet_id)?;
        let mining_fleet_budget = finance.get_fleet_budget(mining_fleet_id)?;
        let siphoning_fleet_budget = finance.get_fleet_budget(siphoning_fleet_id)?;

        assert_eq!(Credits::new(75_000), construction_fleet_budget.total_capital);
        assert_eq!(Credits::new(1_000), construction_fleet_budget.operating_reserve);

        assert_eq!(Credits::new(99_000), finance.treasury);

        assert_eq!(Credits::new(0), market_observation_fleet_budget.total_capital); // 3 probes à 25k each (estimated for now, since we don't have accurate marketdata yet)
        assert_eq!(Credits::new(0), market_observation_fleet_budget.operating_reserve);

        assert_eq!(Credits::new(0), mining_fleet_budget.total_capital); // 3 probes à 25k each (estimated for now, since we don't have accurate marketdata yet)
        assert_eq!(Credits::new(0), mining_fleet_budget.operating_reserve);

        assert_eq!(Credits::new(0), siphoning_fleet_budget.total_capital); // 3 probes à 25k each (estimated for now, since we don't have accurate marketdata yet)
        assert_eq!(Credits::new(0), siphoning_fleet_budget.operating_reserve);

        Ok(())
    }

    #[test(tokio::test)]
    async fn give_excess_capital_to_treasurer_should_work_after_successful_trade() -> Result<(), anyhow::Error> {
        let mut treasury = InMemoryTreasurer::new(175_000.into());
        let fleet_id = FleetId(1);
        treasury.create_fleet_budget(fleet_id.clone(), 76_000.into(), 1_000.into())?;
        let ship = ShipSymbol("FOO-1".to_string());

        let budget_before_trade = treasury.get_fleet_budget(&fleet_id)?;
        assert_eq!(budget_before_trade.available_capital, 75_000.into());
        assert_eq!(budget_before_trade.total_capital, 75_000.into());
        assert_eq!(budget_before_trade.operating_reserve, 1_000.into());

        assert_eq!(treasury.treasury, 99_000.into());

        let result = execute_profitable_trade(
            &mut treasury,
            &ship,
            &fleet_id,
            &WaypointSymbol("FROM".to_string()),
            &WaypointSymbol("TO".to_string()),
            TradeGoodSymbol::ADVANCED_CIRCUITRY,
            37,
            2_000.into(),
            4_000.into(),
        )
        .await?;

        let budget_after_trade = treasury.get_fleet_budget(&fleet_id)?;
        let expected_profit = 74_000;
        assert_eq!(result, expected_profit.into());
        assert_eq!(budget_after_trade.available_capital, (75_000 + expected_profit).into());
        assert_eq!(budget_after_trade.total_capital, 75_000.into());
        assert_eq!(budget_after_trade.operating_reserve, 1_000.into());
        assert_eq!(treasury.treasury, 99_000.into());

        treasury.return_excess_capital_to_treasurer(&fleet_id)?;

        let budget_after_rebalance = treasury.get_fleet_budget(&fleet_id)?;
        assert_eq!(budget_after_rebalance.available_capital, 75_000.into());
        assert_eq!(budget_after_rebalance.total_capital, 75_000.into());
        assert_eq!(budget_after_rebalance.operating_reserve, 1_000.into());
        assert_eq!(treasury.treasury, (99_000 + expected_profit).into());

        Ok(())
    }

    #[test(tokio::test)]
    async fn rebalance_after_unsuccessful_trade() -> Result<(), anyhow::Error> {
        let mut treasury = InMemoryTreasurer::new(175_000.into());
        let fleet_id = FleetId(1);
        treasury.create_fleet_budget(fleet_id.clone(), 76_000.into(), 1_000.into())?;
        let ship = ShipSymbol("FOO-1".to_string());

        let budget_before_trade = treasury.get_fleet_budget(&fleet_id)?;
        assert_eq!(budget_before_trade.available_capital, 75_000.into());
        assert_eq!(budget_before_trade.total_capital, 75_000.into());
        assert_eq!(budget_before_trade.operating_reserve, 1_000.into());

        assert_eq!(treasury.treasury, 99_000.into());

        let result = execute_profitable_trade(
            &mut treasury,
            &ship,
            &fleet_id,
            &WaypointSymbol("FROM".to_string()),
            &WaypointSymbol("TO".to_string()),
            TradeGoodSymbol::ADVANCED_CIRCUITRY,
            37,
            2_000.into(), //purchase price higher than sell price
            1_000.into(),
        )
        .await?;

        let budget_after_trade = treasury.get_fleet_budget(&fleet_id)?;
        let expected_profit: i64 = -37_000;
        assert_eq!(result, expected_profit.into());
        assert_eq!(budget_after_trade.available_capital, (75_000 + expected_profit).into());
        assert_eq!(budget_after_trade.total_capital, 75_000.into());
        assert_eq!(budget_after_trade.operating_reserve, 1_000.into());
        assert_eq!(treasury.treasury, 99_000.into());

        treasury.return_excess_capital_to_treasurer(&fleet_id)?;

        // everything is unchanged
        let budget_after_rebalance = treasury.get_fleet_budget(&fleet_id)?;
        assert_eq!(budget_after_rebalance.available_capital, budget_after_trade.available_capital);
        assert_eq!(budget_after_rebalance.total_capital, budget_after_trade.total_capital);
        assert_eq!(budget_after_rebalance.operating_reserve, budget_after_trade.operating_reserve);
        assert_eq!(treasury.treasury, 99_000.into()); //unchanged balance

        treasury.top_up_available_capital(&fleet_id)?;

        // treasury got reduced and fleets' available capital should be topped up
        let budget_after_top_up = treasury.get_fleet_budget(&fleet_id)?;
        assert_eq!(budget_after_top_up.available_capital, 75_000.into());
        assert_eq!(budget_after_top_up.total_capital, 75_000.into());
        assert_eq!(budget_after_top_up.operating_reserve, 1_000.into());
        assert_eq!(treasury.treasury, 62_000.into());

        Ok(())
    }

    #[test(tokio::test)]
    async fn distribute_budget_and_execute_trades_for_ship_purchase_in_construction_phase() -> Result<(), anyhow::Error> {
        let (bmc, client) = in_memory_test_universe::get_test_universe().await;
        let agent = client.get_agent().await?.data;
        let system_symbol = agent.headquarters.system_symbol();

        // Initialize with 200,000 credits for testing - a reasonable starting amount
        let mut finance = InMemoryTreasurer::new(Credits::new(200_000));

        FleetRunner::load_and_store_initial_data_in_bmcs(Arc::clone(&client), Arc::clone(&bmc))
            .await
            .expect("FleetRunner::load_and_store_initial_data");

        let facts = collect_fleet_decision_facts(Arc::clone(&bmc), &system_symbol).await?;

        let marketplaces_of_interest: HashSet<WaypointSymbol> = HashSet::from_iter(facts.marketplaces_of_interest.iter().cloned());
        let shipyards_of_interest: HashSet<WaypointSymbol> = HashSet::from_iter(facts.shipyards_of_interest.iter().cloned());
        let marketplaces_ex_shipyards: Vec<WaypointSymbol> = marketplaces_of_interest
            .difference(&shipyards_of_interest)
            .cloned()
            .collect_vec();

        // Create a construction fleet phase
        let fleet_phase = fleet::create_construction_fleet_phase(&system_symbol, facts.shipyards_of_interest.len(), marketplaces_ex_shipyards.len());

        let (fleets, fleet_tasks): (Vec<Fleet>, Vec<(FleetId, FleetTask)>) =
            fleet::compute_fleets_with_tasks(&facts, &Default::default(), &Default::default(), &fleet_phase);

        let ship_map = facts
            .ships
            .iter()
            .map(|s| (s.symbol.clone(), s.clone()))
            .collect();

        let ship_price_info = bmc
            .shipyard_bmc()
            .get_latest_ship_prices(&Ctx::Anonymous, &system_symbol)
            .await?;

        let ship_fleet_assignment = FleetAdmiral::assign_ships(&fleet_tasks, &ship_map, &fleet_phase.shopping_list_in_order);

        // Find our fleets
        let construction_fleet_id = fleet_tasks
            .iter()
            .find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::ConstructJumpGate { .. }).then_some(id))
            .unwrap();

        let mining_fleet_id = fleet_tasks
            .iter()
            .find_map(|(id, fleet_task)| matches!(fleet_task, FleetTask::MineOres { .. }).then_some(id))
            .unwrap();

        let all_next_ship_purchases = fleet::get_all_next_ship_purchases(&ship_map, &fleet_phase);

        // Distribute the budgets based on fleet phase
        finance.redistribute_distribute_fleet_budgets(&fleet_phase, &fleet_tasks, &ship_fleet_assignment, &ship_price_info, &all_next_ship_purchases)?;

        // Check the initial budgets
        let construction_budget_before = finance.get_fleet_budget(construction_fleet_id)?;
        let mining_budget_before = finance.get_fleet_budget(mining_fleet_id)?;

        println!(
            "Initial construction fleet budget: available={}, total={}",
            construction_budget_before.available_capital, construction_budget_before.total_capital
        );
        println!(
            "Initial mining fleet budget: available={}, total={}",
            mining_budget_before.available_capital, mining_budget_before.total_capital
        );

        // Get a ship to use for execution - just picking the first available ship
        let executing_ship = facts.ships.first().unwrap().symbol.clone();

        // Generate some waypoints for testing
        let source_waypoint = facts.marketplaces_of_interest.first().unwrap().clone();
        let destination_waypoint = facts.marketplaces_of_interest.last().unwrap().clone();

        let available_capital_before_transaction = finance
            .get_fleet_budget(&construction_fleet_id)?
            .available_capital
            .clone();

        let total_capital_before_transaction = finance
            .get_fleet_budget(&construction_fleet_id)?
            .total_capital
            .clone();
        // Step 1: Execute a profitable trade with the construction fleet
        println!("Executing a profitable trade...");
        let profit = execute_profitable_trade(
            &mut finance,
            &executing_ship,
            construction_fleet_id,
            &source_waypoint,
            &destination_waypoint,
            TradeGoodSymbol::ADVANCED_CIRCUITRY, // High-value good
            50,                                  // Quantity
            Credits::new(500),                   // Buy price
            Credits::new(900),                   // Sell price (80% profit)
        )
        .await?;

        println!("Trade completed with profit: {}", profit);
        let available_capital_after_transaction = finance
            .get_fleet_budget(&construction_fleet_id)?
            .available_capital
            .clone();

        let total_capital_after_transaction = finance
            .get_fleet_budget(&construction_fleet_id)?
            .total_capital
            .clone();
        assert_eq!(available_capital_before_transaction + profit, available_capital_after_transaction);
        assert_eq!(total_capital_before_transaction, total_capital_after_transaction); //total capital doesn't change

        // Check the updated budget after trade
        let construction_budget_after_trade = finance.get_fleet_budget(construction_fleet_id)?;
        println!(
            "Construction fleet budget after trade: available={}, total={}",
            construction_budget_after_trade.available_capital, construction_budget_after_trade.total_capital
        );

        // Step 2: Execute a ship purchase for the mining fleet
        println!("Executing a ship purchase...");
        execute_ship_purchase(
            &mut finance,
            &executing_ship,
            construction_fleet_id, // Construction fleet is buying
            mining_fleet_id,       // For the mining fleet
            &facts.shipyards_of_interest.first().unwrap().clone(),
            ShipType::SHIP_MINING_DRONE,
            Credits::new(25_000),
        )
        .await?;

        // Check the updated budgets after ship purchase
        let construction_budget_after_purchase = finance.get_fleet_budget(construction_fleet_id)?;
        let mining_budget_after_purchase = finance.get_fleet_budget(mining_fleet_id)?;

        println!(
            "Construction fleet budget after ship purchase: available={}, total={}",
            construction_budget_after_purchase.available_capital, construction_budget_after_purchase.total_capital
        );
        println!(
            "Mining fleet budget after ship purchase: available={}, total={}, asset_value={}",
            mining_budget_after_purchase.available_capital, mining_budget_after_purchase.total_capital, mining_budget_after_purchase.asset_value
        );

        // Verify the results
        assert_eq!(
            construction_budget_after_trade.total_capital, construction_budget_before.total_capital,
            "Trading should not increase the fleet's total capital"
        );

        assert!(
            construction_budget_after_purchase.available_capital < construction_budget_after_trade.available_capital,
            "Ship purchase should reduce available capital"
        );

        assert!(
            mining_budget_after_purchase.asset_value > mining_budget_before.asset_value,
            "Ship purchase should increase the receiving fleet's asset value"
        );

        Ok(())
    }

    // Helper function to execute a profitable trade
    async fn execute_profitable_trade(
        treasurer: &mut InMemoryTreasurer,
        executing_ship: &ShipSymbol,
        executing_fleet: &FleetId,
        source_waypoint: &WaypointSymbol,
        destination_waypoint: &WaypointSymbol,
        good: TradeGoodSymbol,
        quantity: u32,
        buy_price: Credits,
        sell_price: Credits,
    ) -> Result<Credits, FinanceError> {
        // Create a ticket for trading
        let ticket_id = treasurer.create_ticket(
            TicketType::Trading,
            executing_ship.clone(),
            executing_fleet,
            executing_fleet, // Initiating fleet is the same as executing
            executing_fleet, // Beneficiary fleet is the same as executing
            vec![
                // Purchase goal
                TransactionGoal::PurchaseTradeGoods(PurchaseTradeGoodsTransactionGoal {
                    id: TransactionTicketId::new(),
                    good: good.clone(),
                    target_quantity: quantity,
                    available_quantity: Some(quantity),
                    acquired_quantity: 0,
                    estimated_price_per_unit: buy_price,
                    max_acceptable_price_per_unit: Some(buy_price * 2),
                    source_waypoint: source_waypoint.clone(),
                }),
                // Sell goal
                TransactionGoal::SellTradeGoods(SellTradeGoodsTransactionGoal {
                    id: TransactionTicketId::new(),
                    good: good.clone(),
                    target_quantity: quantity,
                    sold_quantity: 0,
                    estimated_price_per_unit: sell_price,
                    min_acceptable_price_per_unit: Some(sell_price / 2),
                    destination_waypoint: destination_waypoint.clone(),
                }),
            ],
            Utc::now() + Duration::hours(1),
            10.0, // High priority
        )?;

        // Fund the ticket
        let required_capital = quantity as i64 * buy_price.0;
        treasurer.fund_ticket(
            ticket_id,
            FundingSource {
                source_fleet: executing_fleet.clone(),
                amount: Credits::new(required_capital),
            },
        )?;

        // Start execution
        treasurer.start_ticket_execution(ticket_id)?;

        // Record purchase event
        let purchase_event = TransactionEvent::GoodsPurchased {
            timestamp: Utc::now(),
            waypoint: source_waypoint.clone(),
            good: good.clone(),
            quantity,
            price_per_unit: buy_price,
            total_cost: Credits::new(quantity as i64 * buy_price.0),
        };
        treasurer.record_event(ticket_id, purchase_event)?;

        // Record sell event
        let sell_event = TransactionEvent::GoodsSold {
            timestamp: Utc::now() + Duration::minutes(10),
            waypoint: destination_waypoint.clone(),
            good,
            quantity,
            price_per_unit: sell_price,
            total_revenue: Credits::new(quantity as i64 * sell_price.0),
        };
        treasurer.record_event(ticket_id, sell_event)?;

        // The ticket should be automatically completed after all goals are fulfilled
        // Let's get the ticket to check the final profit
        let ticket = treasurer.get_ticket(ticket_id)?;
        Ok(ticket.financials.current_profit)
    }

    // Helper function to execute a ship purchase
    async fn execute_ship_purchase(
        treasurer: &mut InMemoryTreasurer,
        executing_ship: &ShipSymbol,
        executing_fleet: &FleetId,
        beneficiary_fleet: &FleetId,
        shipyard_waypoint: &WaypointSymbol,
        ship_type: ShipType,
        estimated_cost: Credits,
    ) -> Result<(), FinanceError> {
        // Create a ticket for ship purchase
        let ticket_id = treasurer.create_ticket(
            TicketType::ShipPurchase,
            executing_ship.clone(),
            executing_fleet,
            executing_fleet,   // Initiating fleet is the same as executing
            beneficiary_fleet, // The fleet that will receive the ship
            vec![TransactionGoal::PurchaseShip(PurchaseShipTransactionGoal {
                id: TransactionTicketId::new(),
                ship_type: ship_type.clone(),
                estimated_cost,
                has_been_purchased: false,
                beneficiary_fleet: beneficiary_fleet.clone(),
                shipyard_waypoint: shipyard_waypoint.clone(),
            })],
            Utc::now() + Duration::hours(1),
            5.0, // Medium priority
        )?;

        // Fund the ticket
        treasurer.fund_ticket(
            ticket_id,
            FundingSource {
                source_fleet: executing_fleet.clone(),
                amount: estimated_cost,
            },
        )?;

        // Start execution
        treasurer.start_ticket_execution(ticket_id)?;

        // Record ship purchase event
        let purchase_event = TransactionEvent::ShipPurchased {
            timestamp: Utc::now(),
            waypoint: shipyard_waypoint.clone(),
            ship_type,
            ship_id: ShipSymbol("TEST".to_string()), // Generate a random ship ID
            total_cost: estimated_cost,
            beneficiary_fleet: beneficiary_fleet.clone(),
        };
        treasurer.record_event(ticket_id, purchase_event)?;

        // The ticket should be automatically completed after the goal is fulfilled
        Ok(())
    }
}
