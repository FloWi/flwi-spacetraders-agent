use anyhow::{Context, Result};
use clap::Parser;
use sqlx::{ConnectOptions, Executor, Pool, Postgres};
use tracing::{event, Level};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use utoipa::OpenApi;

use flwi_spacetraders_agent::api_client::api_model::RegistrationRequest;
use flwi_spacetraders_agent::cli_args;
use flwi_spacetraders_agent::cli_args::{Cli, Commands};
use flwi_spacetraders_agent::configuration::AgentConfiguration;
use flwi_spacetraders_agent::db;
use flwi_spacetraders_agent::pagination::{collect_results, fetch_all_pages, PaginationInput};
use flwi_spacetraders_agent::reqwest_helpers::create_client;
use flwi_spacetraders_agent::st_client::StClient;
use flwi_spacetraders_agent::st_model::{
    AgentSymbol, FactionSymbol, WaypointSymbol, WaypointTrait, WaypointTraitSymbol,
};

#[tokio::main]
async fn main() -> Result<()> {
    let args = cli_args::Cli::parse();

    match args {
        Cli { command } => match command.clone() {
            Commands::RunAgent {
                database_url,
                spacetraders_agent_faction,
                spacetraders_agent_symbol,
                spacetraders_registration_email,
            } => {
                tracing_subscriber::registry()
                    .with(fmt::layer().with_span_events(fmt::format::FmtSpan::CLOSE))
                    .with(EnvFilter::from_default_env())
                    .init();

                let cfg = AgentConfiguration::new(command.clone());

                let reqwest_client_with_middleware = create_client(None);

                let unauthenticated_client = StClient::new(reqwest_client_with_middleware);

                let status = unauthenticated_client.get_status().await?;

                let pool = db::prepare_database_schema(status, cfg.clone()).await?;

                let authenticated_client = get_authenticated_client(
                    &pool,
                    unauthenticated_client,
                    spacetraders_agent_faction,
                    spacetraders_agent_symbol,
                    spacetraders_registration_email,
                )
                .await?;

                let my_agent = authenticated_client.get_agent().await?;
                dbg!(my_agent.clone());

                let headquarters_waypoint_symbol =
                    WaypointSymbol(my_agent.data.headquarters.clone());
                let headquarters_system_symbol = headquarters_waypoint_symbol.system_symbol();

                let waypoints_of_system = fetch_all_pages(
                    |page| {
                        authenticated_client
                            .list_waypoints_of_system_page(&headquarters_system_symbol, page)
                    },
                    PaginationInput { page: 1, limit: 20 },
                )
                .await?;

                let marketplaces: Vec<_> = waypoints_of_system
                    .iter()
                    .filter(|wp| {
                        wp.traits.iter().any(|wp_trait| {
                            wp_trait.symbol == WaypointTraitSymbol("MARKETPLACE".to_string())
                        })
                    })
                    .map(|wp| wp.symbol.clone())
                    .collect();

                let market_data: Vec<_> =
                    collect_results(marketplaces.clone(), |waypoint_symbol| {
                        authenticated_client.get_marketplace(waypoint_symbol)
                    })
                    .await?
                    .iter()
                    .map(|md| md.data.clone())
                    .collect();

                println!("marketplaces: \n{}", serde_json::to_string(&marketplaces)?);
                println!("market_data: \n{}", serde_json::to_string(&market_data)?);

                println!(
                    "all waypoints of home systme: \n{}",
                    serde_json::to_string(&waypoints_of_system)?
                );

                Ok(())
            }
        },
    }
}

async fn get_authenticated_client(
    pool: &Pool<Postgres>,
    unauthenticated_client: StClient,
    spacetraders_agent_faction: String,
    spacetraders_agent_symbol: String,
    spacetraders_registration_email: String,
) -> Result<StClient> {
    event!(Level::INFO, "Trying to load registration from database",);

    let maybe_existing_reqistration = db::load_registration(pool).await?;

    match maybe_existing_reqistration {
        Some(db_entry) => {
            event!(
                Level::INFO,
                "Found registration infos in database. Creating authenticated client",
            );

            Ok(StClient::new(create_client(Some(db_entry.token))))
        }
        None => {
            event!(
                Level::INFO,
                "No registration infos found in database. Registering new agent",
            );

            let registration_response = unauthenticated_client
                .register(RegistrationRequest {
                    faction: FactionSymbol(spacetraders_agent_faction),
                    symbol: spacetraders_agent_symbol,
                    email: spacetraders_registration_email,
                })
                .await
                .expect("Error during registration");

            event!(
                Level::INFO,
                "Registration complete: {:?}",
                registration_response
            );

            let _ = db::save_registration(pool, registration_response.clone()).await;

            Ok(StClient::new(create_client(Some(
                registration_response.clone().data.token,
            ))))
        }
    }
}
