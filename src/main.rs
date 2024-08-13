use std::time::Duration;

use anyhow::{Context, Error, Result};
use clap::{command, Parser};
use futures::join;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::sqlx_macros::migrate;
use sqlx::{ConnectOptions, Executor, Pool, Postgres};
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::log::LevelFilter;
use tracing::{event, Level};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use utoipa::OpenApi;

use crate::cli_args::{Cli, Commands};
use crate::configuration::AgentConfiguration;
use crate::reqwest_helpers::create_client;
use crate::st_client::StClient;

mod cli_args;
mod configuration;
mod db;
mod pagination;
mod reqwest_helpers;
mod st_client;
mod st_model;

#[tokio::main]
async fn main() -> Result<()> {
    let args = cli_args::Cli::parse();

    match args {
        Cli { command } => match command {
            Commands::RunAgent { .. } => {
                tracing_subscriber::registry()
                    .with(fmt::layer())
                    .with(EnvFilter::from_default_env())
                    .init();

                let cfg = AgentConfiguration::new(command);

                let reqwest_client_with_middleware = create_client();

                let unauthenticated_client = StClient::new(reqwest_client_with_middleware);

                let status = unauthenticated_client.get_status().await?;

                db::prepare_database_schema(status, cfg).await?;

                Ok(())
            }
        },
    }
}

/*
DATABASE_USER=postgres
DATABASE_PASSWORD=spacetraders-password
DATABASE_PORT=25432
DATABASE_HOST=localhost

DATABASE_HOST=localhost
DATABASE_PASSWORD=spacetraders-password
DATABASE_PORT=25432
DATABASE_USER=postgres
DATABASE_NAME=spacetraders
 */
