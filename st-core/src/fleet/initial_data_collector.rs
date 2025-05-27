use crate::marketplaces::marketplaces::filter_waypoints_with_trait;
use crate::pagination::fetch_all_pages;
use crate::st_client::StClientTrait;
use chrono::Utc;
use itertools::Itertools;
use st_domain::{Ship, WaypointTraitSymbol, WaypointType};
use st_store::bmc::Bmc;
use st_store::Ctx;
use std::ops::Not;
use std::sync::Arc;

pub async fn load_and_store_initial_data_in_bmcs(client: Arc<dyn StClientTrait>, bmc: Arc<dyn Bmc>) -> anyhow::Result<()> {
    let ctx = &Ctx::Anonymous;
    let agent = match bmc.agent_bmc().load_agent(ctx).await {
        Ok(agent) => agent,
        Err(_) => {
            let response = client.get_agent().await?;
            bmc.agent_bmc().store_agent(ctx, &response.data).await?;
            response.data
        }
    };

    let ships = bmc.ship_bmc().get_ships(&Ctx::Anonymous, None).await?;

    if ships.is_empty() {
        let ships: Vec<Ship> = fetch_all_pages(|p| client.list_ships(p)).await?;
        bmc.ship_bmc()
            .upsert_ships(&Ctx::Anonymous, &ships, Utc::now())
            .await?;
    }

    let headquarters_system_symbol = agent.headquarters.system_symbol();

    let waypoint_entries_of_home_system = match bmc
        .system_bmc()
        .get_waypoints_of_system(ctx, &headquarters_system_symbol)
        .await
    {
        Ok(waypoints) if waypoints.is_empty().not() => waypoints,
        _ => {
            let waypoints = fetch_all_pages(|p| client.list_waypoints_of_system_page(&headquarters_system_symbol, p)).await?;
            bmc.system_bmc()
                .save_waypoints_of_system(ctx, &headquarters_system_symbol, waypoints.clone())
                .await?;
            waypoints
        }
    };

    let marketplaces_to_collect_remotely = filter_waypoints_with_trait(&waypoint_entries_of_home_system, WaypointTraitSymbol::MARKETPLACE)
        .map(|wp| wp.symbol.clone())
        .collect_vec();

    let shipyards_to_collect_remotely = filter_waypoints_with_trait(&waypoint_entries_of_home_system, WaypointTraitSymbol::SHIPYARD)
        .map(|wp| wp.symbol.clone())
        .collect_vec();

    for wps in marketplaces_to_collect_remotely {
        let market = client.get_marketplace(wps).await?;
        bmc.market_bmc()
            .save_market_data(ctx, vec![market.data], Utc::now())
            .await?;
    }
    for wps in shipyards_to_collect_remotely {
        let shipyard = client.get_shipyard(wps).await?;
        bmc.shipyard_bmc()
            .save_shipyard_data(ctx, shipyard.data, Utc::now())
            .await?;
    }
    let jump_gate_wp_of_home_system = waypoint_entries_of_home_system
        .iter()
        .find(|wp| wp.r#type == WaypointType::JUMP_GATE)
        .expect("home system should have a jump-gate");

    let construction_site = match bmc
        .construction_bmc()
        .get_construction_site_for_system(ctx, headquarters_system_symbol)
        .await
    {
        Ok(Some(cs)) => cs,
        _ => {
            let cs = client
                .get_construction_site(&jump_gate_wp_of_home_system.symbol)
                .await?;
            bmc.construction_bmc()
                .save_construction_site(ctx, cs.data.clone())
                .await?;
            cs.data
        }
    };

    let supply_chain_data = client.get_supply_chain().await?;
    bmc.supply_chain_bmc()
        .insert_supply_chain(&Ctx::Anonymous, supply_chain_data.into(), Utc::now())
        .await?;

    Ok(())
}
