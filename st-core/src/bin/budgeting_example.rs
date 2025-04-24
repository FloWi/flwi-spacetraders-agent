use chrono::{Duration, Utc};
use st_core::accounting::budgeting::{FundingSource, TicketType, TransactionEvent, TransactionGoal};
use st_core::accounting::credits::Credits;
use st_core::accounting::treasurer::{InMemoryTreasurer, Treasurer};
use st_domain::{FleetId, ShipSymbol, ShipType, TradeGoodSymbol, WaypointSymbol};
use std::collections::HashMap;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    // Create an in-memory finance system with 1,000,000 credits in treasury
    let mut finance = InMemoryTreasurer::new(Credits::new(1_000_000));

    // Create some fleets
    let trading_fleet_id = FleetId(1);
    let mining_fleet_id = FleetId(2);
    let market_observation_fleet_id = FleetId(3);

    finance.create_fleet_budget(mining_fleet_id.clone(), Credits::new(300_000), Credits::new(0))?;
    finance.create_fleet_budget(trading_fleet_id.clone(), Credits::new(500_000), Credits::new(0))?;
    finance.create_fleet_budget(market_observation_fleet_id.clone(), Credits::new(50_000), Credits::new(0))?;

    println!("Created fleets with initial budgets:");
    println!("  TRADING_FLEET: {}", finance.get_fleet_budget(&trading_fleet_id)?.available_capital);
    println!("  MINING_FLEET: {}", finance.get_fleet_budget(&mining_fleet_id)?.available_capital);
    println!(
        "  MARKET_OBSERVATION_FLEET: {}",
        finance.get_fleet_budget(&market_observation_fleet_id)?.available_capital
    );

    println!("Created fleets with budgets");

    // Create a trading transaction with multiple goals
    let goals = vec![
        // Goal 1: Purchase goods at X1-YZ45
        TransactionGoal::Purchase {
            good: TradeGoodSymbol::PRECIOUS_STONES,
            target_quantity: 100,
            available_quantity: None,
            acquired_quantity: 0,
            estimated_price: Credits::new(2_000),
            max_acceptable_price: Some(Credits::new(2_200)),
            source_waypoint: WaypointSymbol("X1-YZ45".to_string()),
        },
        // Goal 2: Refuel at X1-YZ46 (optional)
        TransactionGoal::Refuel {
            target_fuel_level: 100,
            current_fuel_level: 60,
            estimated_cost_per_unit: Credits::new(250),
            waypoint: WaypointSymbol("X1-YZ46".to_string()),
            is_optional: true,
        },
        // Goal 3: Sell goods at X1-YZ47
        TransactionGoal::Sell {
            good: TradeGoodSymbol::PRECIOUS_STONES,
            target_quantity: 100,
            sold_quantity: 0,
            estimated_price: Credits::new(2_500),
            min_acceptable_price: Some(Credits::new(2_400)),
            destination_waypoint: WaypointSymbol("X1-YZ47".to_string()),
        },
    ];

    // Create a transaction ticket
    let estimated_completion = Utc::now() + Duration::hours(3);
    let ticket_id = finance.create_ticket(
        TicketType::Trading,
        ShipSymbol("SHIP-1".to_string()),
        &trading_fleet_id,
        &trading_fleet_id,
        &trading_fleet_id,
        goals,
        estimated_completion,
        10.0,
    )?;

    println!("\nCreated trading ticket with ID: {}", ticket_id);

    // Fund the ticket
    finance.fund_ticket(
        ticket_id,
        FundingSource {
            source_fleet: trading_fleet_id.clone(),
            amount: Credits::new(215_000), // Purchase cost + refuel cost buffer
        },
    )?;

    println!("Funded the ticket with 215,000 credits from Trading Fleet");

    // Start executing the ticket
    finance.start_ticket_execution(ticket_id)?;
    println!("Started ticket execution");

    // Manually simulate the execution process
    println!("\n--- Manually simulating the transaction execution ---\n");

    // STEP 3: Ship observes the market
    let mut market_prices = HashMap::new();
    market_prices.insert(TradeGoodSymbol::PRECIOUS_STONES, Credits::new(2100));

    let mut market_supply = HashMap::new();
    market_supply.insert(TradeGoodSymbol::PRECIOUS_STONES, Credits::new(200));

    // STEP 4: Ship purchases goods
    // The price is acceptable (2100 <= 2200 max acceptable price)
    let purchase_event = TransactionEvent::GoodsPurchased {
        timestamp: Utc::now(),
        waypoint: WaypointSymbol("X1-YZ45".to_string()),
        good: TradeGoodSymbol::PRECIOUS_STONES,
        quantity: 100,
        price_per_unit: Credits::new(2100),
        total_cost: Credits::new(210_000),
    };
    finance.record_event(ticket_id, purchase_event)?;
    println!("Purchased 100 units of PRECIOUS_METALS for 210,000 credits");

    // Display current ticket state
    let current_ticket = finance.get_ticket(ticket_id)?;
    println!("\nCurrent ticket state after purchase:");
    println!("  Spent capital: {}", current_ticket.financials.spent_capital);
    println!("  Purchase goal completed: {}", current_ticket.goals[0].is_completed());

    // STEP 10: Ship observes the sell market
    let mut sell_prices = HashMap::new();
    sell_prices.insert(TradeGoodSymbol::PRECIOUS_STONES, 2700); // Good price!

    // STEP 11: Ship sells the goods
    // The price is very good (2700 > 2400 min acceptable price)
    let sell_event = TransactionEvent::GoodsSold {
        timestamp: Utc::now(),
        waypoint: WaypointSymbol("X1-YZ47".to_string()),
        good: TradeGoodSymbol::PRECIOUS_STONES,
        quantity: 100,
        price_per_unit: Credits::new(2700),
        total_revenue: Credits::new(270_000),
    };
    finance.record_event(ticket_id, sell_event)?;
    println!("Sold 100 units of PRECIOUS_METALS for 270,000 credits");

    // STEP 12: All goals are now complete, so the ticket should be completed
    // The system should auto-detect this, but we'll explicitly call complete
    finance.complete_ticket(ticket_id)?;
    println!("\nAll goals completed, finalizing transaction");

    // Get the final ticket state
    let final_ticket = finance.get_ticket(ticket_id)?;

    // Show final results
    println!("\nFinal Transaction Summary:");
    println!("  Status: {:?}", final_ticket.status);
    println!("  Total spent: {}", final_ticket.financials.spent_capital);
    println!("  Total earned: {}", final_ticket.financials.earned_revenue);
    println!("  Net profit: {}", final_ticket.financials.current_profit);
    println!("  Operating expenses: {}", final_ticket.financials.operating_expenses);

    println!("{}", final_ticket.generate_event_history());

    // Check updated fleet budget
    let updated_budget = finance.get_fleet_budget(&trading_fleet_id)?;
    println!("\nUpdated Trading Fleet Budget:");
    println!("  Available capital: {}", updated_budget.available_capital);

    println!(
        "  Market Observation Fleet: {}",
        finance.get_fleet_budget(&market_observation_fleet_id)?.available_capital
    );

    // Create a ship purchase ticket
    // Trading fleet will buy a ship for the market observation fleet
    let ship_purchase_goal = TransactionGoal::ShipPurchase {
        ship_type: ShipType::SHIP_LIGHT_HAULER,
        estimated_cost: Credits::new(25_000),
        beneficiary_fleet: market_observation_fleet_id.clone(),
        shipyard_waypoint: WaypointSymbol("X1-SHIPYARD".to_string()),
        has_been_purchased: false,
    };

    let ticket_id = finance.create_ticket(
        TicketType::FleetExpansion,
        ShipSymbol("TRADING-SHIP-1".to_string()), // Ship executing the purchase
        &trading_fleet_id,                        // Fleet owning the ship
        &trading_fleet_id,                        // Fleet initiating the purchase
        &market_observation_fleet_id,             // Fleet benefiting from the purchase
        vec![ship_purchase_goal],
        Utc::now() + Duration::hours(1),
        5.0,
    )?;

    println!("\nCreated ship purchase ticket with ID: {}", ticket_id);

    // Fund the ticket from trading fleet
    finance.fund_ticket(
        ticket_id,
        FundingSource {
            source_fleet: market_observation_fleet_id.clone(),
            amount: Credits::new(25_000),
        },
    )?;

    println!("Funded the ticket with 25,000 credits from Trading Fleet");

    // Start executing the ticket
    finance.start_ticket_execution(ticket_id)?;
    println!("Started ticket execution");

    // --- Manually simulate the execution ---
    println!("\n--- Simulating the ship purchase ---\n");

    // Purchase the new ship
    let purchase_event = TransactionEvent::ShipPurchased {
        timestamp: Utc::now(),
        waypoint: WaypointSymbol("X1-SHIPYARD".to_string()),
        ship_type: ShipType::SHIP_LIGHT_HAULER,
        ship_id: ShipSymbol("OBSERVER-SHIP-1".to_string()),
        total_cost: Credits::new(25_000),
        beneficiary_fleet: market_observation_fleet_id.clone(),
    };
    finance.record_event(ticket_id, purchase_event)?;
    println!("Purchased LIGHT_HAULER ship for 25,000 credits (ID: OBSERVER-SHIP-1)");

    // Transfer the ship to the observation fleet
    let transfer_event = TransactionEvent::ShipTransferred {
        timestamp: Utc::now(),
        ship_id: ShipSymbol("OBSERVER-SHIP-1".to_string()),
        from_fleet: trading_fleet_id.clone(),
        to_fleet: market_observation_fleet_id.clone(),
    };
    finance.record_event(ticket_id, transfer_event)?;
    println!("Transferred ship OBSERVER-SHIP-1 to MARKET_OBSERVATION_FLEET");

    // Complete the ticket
    finance.complete_ticket(ticket_id)?;
    println!("\nCompleted ship purchase transaction");

    let final_ticket = finance.get_ticket(ticket_id)?;
    println!("{}", final_ticket.generate_event_history());

    // Check final fleet budgets
    let trading_budget = finance.get_fleet_budget(&trading_fleet_id)?;
    let observer_budget = finance.get_fleet_budget(&market_observation_fleet_id)?;

    println!("\nFinal fleet budgets:");
    println!("  Trading Fleet: {}", trading_budget.available_capital);
    println!("  Market Observation Fleet: {}", observer_budget.available_capital);
    println!("  Market Observation Fleet assets: {}", observer_budget.asset_value);

    Ok(())
}
