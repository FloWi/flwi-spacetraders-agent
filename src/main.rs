use anyhow::{Context, Result};
use clap::Parser;
use sqlx::{ConnectOptions, Executor, Pool, Postgres};
use tracing::{event, Level};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use utoipa::OpenApi;

use crate::api_client::api_model::RegistrationRequest;
use crate::cli_args::{Cli, Commands};
use crate::configuration::AgentConfiguration;
use crate::reqwest_helpers::create_client;
use crate::st_client::StClient;
use crate::st_model::{AgentSymbol, FactionSymbol};

mod cli_args;
mod configuration;
mod db;
mod pagination;
mod reqwest_helpers;
mod st_client;
mod st_model;

mod api_client;

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
                    .with(fmt::layer())
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
                dbg!(my_agent);
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
