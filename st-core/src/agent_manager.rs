use crate::agent::run_agent;
use crate::configuration::AgentConfiguration;
use crate::reqwest_helpers::{create_client, ResetSignal};
use crate::st_client::{StClient, StClientTrait};
use anyhow::Result;
use futures::StreamExt;
use reqwest::Url;
use sqlx::{Pool, Postgres};
use st_domain::{FactionSymbol, RegistrationRequest};
use st_store::db;
use std::str::FromStr;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tracing::{event, Level};
use tracing_subscriber::prelude::*;

pub struct AgentManager {
    reset_rx: mpsc::Receiver<ResetSignal>,
    cfg: AgentConfiguration,
    current_agent_handle: Option<JoinHandle<()>>,
    pool: Option<Pool<Postgres>>,
}

impl AgentManager {
    pub fn new(cfg: AgentConfiguration) -> (Self, mpsc::Sender<ResetSignal>) {
        let (reset_tx, reset_rx) = mpsc::channel::<ResetSignal>(8);

        (
            Self {
                reset_rx,
                cfg,
                current_agent_handle: None,
                pool: None,
            },
            reset_tx,
        )
    }

    pub async fn run(&mut self) -> Result<()> {
        loop {
            // Create a shutdown channel for this agent instance
            let (shutdown_tx, shutdown_rx) = watch::channel(false);

            // Initialize the environment and start a new agent
            match self.initialize_and_start_agent(shutdown_rx.clone()).await {
                Ok(agent_handle) => {
                    self.current_agent_handle = Some(agent_handle);
                    event!(Level::INFO, "Agent started successfully");
                }
                Err(e) => {
                    event!(Level::ERROR, "Failed to start agent: {}", e);
                    // Wait a bit before trying again
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            }

            // Wait for a reset signal
            if let Some(signal) = self.reset_rx.recv().await {
                event!(Level::INFO, "Reset signal received: {:?}, restarting agent", signal);

                // Signal the current agent to shut down
                let _ = shutdown_tx.send(true);

                // If we have a handle to the current agent, wait for it to shut down
                if let Some(handle) = self.current_agent_handle.take() {
                    // Give the agent a chance to shut down gracefully
                    tokio::time::sleep(Duration::from_secs(1)).await;

                    // If it's still running, abort it
                    handle.abort();
                }

                // Wait a bit before starting a new agent
                tokio::time::sleep(Duration::from_secs(2)).await;
            } else {
                // Channel closed, exit the loop
                break;
            }
        }

        Ok(())
    }

    async fn initialize_and_start_agent(&mut self, shutdown_rx: watch::Receiver<bool>) -> Result<JoinHandle<()>> {
        // Create a reset channel for this specific agent instance
        let (agent_reset_tx, _) = mpsc::channel::<ResetSignal>(8);

        // Create the initial client (without token) with reset detection
        let client_with_account_token = create_client(Some(self.cfg.spacetraders_account_token.clone()), Some(agent_reset_tx.clone()));
        let client_with_account_token = StClient::try_with_base_url(client_with_account_token, &self.cfg.spacetraders_base_url)?;

        // Get the status (this will verify the API is responding)
        let status = client_with_account_token.get_status().await?;

        // Initialize or get database pool
        if self.pool.is_none() {
            self.pool = Some(db::prepare_database_schema(&status, self.cfg.pg_connection_string()).await?);
        }
        let pool = self.pool.clone().unwrap();

        // Get the authenticated client
        let authenticated_client = get_authenticated_client(&self.cfg, pool.clone(), client_with_account_token).await?;

        // Clone configuration for the new agent
        let cfg = self.cfg.clone();

        // Spawn the agent task
        let handle = tokio::spawn(async move {
            // Run agent with the authenticated client
            let agent_task = async {
                match run_agent(cfg, status, authenticated_client, pool).await {
                    Ok(()) => event!(Level::INFO, "Agent completed successfully"),
                    Err(e) => event!(Level::ERROR, "Agent error: {}", e),
                }
            };

            // Create a task that waits for the shutdown signal
            let shutdown_task = async {
                loop {
                    if *shutdown_rx.borrow() {
                        event!(Level::INFO, "Shutdown signal received, stopping agent");
                        break;
                    }

                    // Check periodically
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            };

            // Use select to race between the agent task and shutdown signal
            tokio::select! {
                _ = agent_task => {
                    // Agent completed on its own
                    event!(Level::INFO, "Agent task completed");
                },
                _ = shutdown_task => {
                    // Shutdown was requested
                    event!(Level::INFO, "Agent shutdown requested");
                }
            }
        });

        Ok(handle)
    }
}

pub async fn get_authenticated_client(cfg: &AgentConfiguration, pool: Pool<Postgres>, client_with_account_token: StClient) -> Result<StClient> {
    event!(Level::INFO, "Trying to load registration from database");

    let maybe_existing_registration = db::load_registration(&pool).await?;

    match maybe_existing_registration {
        Some(db_entry) => {
            event!(Level::INFO, "Found registration infos in database. Creating authenticated client",);

            // Create a reset channel for this specific client
            let (agent_reset_tx, _) = mpsc::channel::<ResetSignal>(8);

            Ok(StClient::try_with_base_url(
                create_client(Some(db_entry.token), Some(agent_reset_tx)),
                &cfg.spacetraders_base_url,
            )?)
        }
        None => {
            event!(Level::INFO, "No registration infos found in database. Registering new agent",);

            let faction = FactionSymbol::from_str(&cfg.spacetraders_agent_faction)?;

            let registration_response = client_with_account_token
                .register(RegistrationRequest {
                    faction,
                    symbol: cfg.spacetraders_agent_symbol.clone(),
                    email: cfg.spacetraders_registration_email.clone(),
                })
                .await
                .expect("Error during registration");

            event!(Level::INFO, "Registration complete: {:?}", registration_response);

            let _ = db::save_registration(&pool, registration_response.clone()).await;

            // Create a reset channel for this specific client
            let (agent_reset_tx, _) = mpsc::channel::<ResetSignal>(8);

            Ok(StClient::try_with_base_url(
                create_client(Some(registration_response.data.token), Some(agent_reset_tx)),
                &cfg.spacetraders_base_url,
            )?)
        }
    }
}
