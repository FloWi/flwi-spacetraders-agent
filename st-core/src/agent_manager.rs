use crate::agent::run_agent;
use crate::configuration::AgentConfiguration;
use crate::reqwest_helpers::{create_client, ResetSignal};
use crate::st_client::{StClient, StClientTrait};
use crate::transfer_cargo_manager::TransferCargoManager;
use crate::universe_server::universe_server::{InMemoryUniverse, InMemoryUniverseClient};
use anyhow::Result;
use futures::StreamExt;
use sqlx::{Pool, Postgres};
use st_domain::{FactionSymbol, RegistrationRequest};
use st_store::bmc::jump_gate_bmc::InMemoryJumpGateBmc;
use st_store::bmc::ship_bmc::{InMemoryShips, InMemoryShipsBmc};
use st_store::bmc::{Bmc, DbBmc, InMemoryBmc};
use st_store::shipyard_bmc::InMemoryShipyardBmc;
use st_store::survey_bmc::InMemorySurveyBmc;
use st_store::trade_bmc::InMemoryTradeBmc;
use st_store::{
    db, DbModelManager, InMemoryAgentBmc, InMemoryConstructionBmc, InMemoryFleetBmc, InMemoryMarketBmc, InMemoryStatusBmc, InMemorySupplyChainBmc,
    InMemorySystemsBmc,
};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch::Receiver;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tracing::{event, Level};
use tracing_subscriber::prelude::*;

pub struct AgentManager {
    reset_rx: mpsc::Receiver<ResetSignal>,
    cfg: AgentConfiguration,
    current_agent_handle: Option<JoinHandle<()>>,
    bmc: Option<Arc<dyn Bmc>>,
}

impl AgentManager {
    pub fn new(cfg: AgentConfiguration) -> (Self, mpsc::Sender<ResetSignal>) {
        let (reset_tx, reset_rx) = mpsc::channel::<ResetSignal>(8);

        (
            Self {
                reset_rx,
                cfg,
                current_agent_handle: None,
                bmc: None,
            },
            reset_tx,
        )
    }

    pub async fn run(&mut self) -> Result<()> {
        loop {
            // Create a shutdown channel for this agent instance
            let (shutdown_tx, shutdown_rx) = watch::channel(false);

            let either_handle = if self.cfg.use_in_memory_agent {
                self.initialize_and_start_in_memory_agent(shutdown_rx.clone())
                    .await
            } else {
                self.initialize_and_start_db_agent(shutdown_rx.clone())
                    .await
            };
            // Initialize the environment and start a new agent
            match either_handle {
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

    async fn initialize_and_start_in_memory_agent(&mut self, shutdown_rx: watch::Receiver<bool>) -> Result<JoinHandle<()>> {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");

        let json_path = std::path::Path::new(manifest_dir)
            .parent()
            .unwrap()
            .join("resources")
            .join("universe_snapshot.json");

        let in_memory_universe = InMemoryUniverse::from_snapshot(json_path).expect("InMemoryUniverse::from_snapshot");
        let in_memory_client = InMemoryUniverseClient::new(in_memory_universe);

        let client = Arc::new(in_memory_client) as Arc<dyn StClientTrait>;

        let agent = client.get_agent().await?.data;

        let ship_bmc = InMemoryShipsBmc::new(InMemoryShips::new());
        let agent_bmc = InMemoryAgentBmc::new(agent);
        let trade_bmc = InMemoryTradeBmc::new();
        let fleet_bmc = InMemoryFleetBmc::new();
        let system_bmc = InMemorySystemsBmc::new();
        let construction_bmc = InMemoryConstructionBmc::new();
        let survey_bmc = InMemorySurveyBmc::new();

        //insert some data
        //construction_bmc.save_construction_site(&Ctx::Anonymous, in_memory_client.get_construction_site().unwrap())

        let market_bmc = InMemoryMarketBmc::new();
        let shipyard_bmc = InMemoryShipyardBmc::new();
        let jump_gate_bmc = InMemoryJumpGateBmc::new();
        let supply_chain_bmc = InMemorySupplyChainBmc::new();
        let status_bmc = InMemoryStatusBmc::new();

        let trade_bmc = Arc::new(trade_bmc);
        let market_bmc = Arc::new(market_bmc);
        let bmc = InMemoryBmc {
            in_mem_ship_bmc: Arc::new(ship_bmc),
            in_mem_fleet_bmc: Arc::new(fleet_bmc),
            in_mem_trade_bmc: Arc::clone(&trade_bmc),
            in_mem_system_bmc: Arc::new(system_bmc),
            in_mem_agent_bmc: Arc::new(agent_bmc),
            in_mem_construction_bmc: Arc::new(construction_bmc),
            in_mem_survey_bmc: Arc::new(survey_bmc),
            in_mem_market_bmc: Arc::clone(&market_bmc),
            in_mem_jump_gate_bmc: Arc::new(jump_gate_bmc),
            in_mem_shipyard_bmc: Arc::new(shipyard_bmc),
            in_mem_supply_chain_bmc: Arc::new(supply_chain_bmc),
            in_mem_status_bmc: Arc::new(status_bmc),
            in_mem_ledger_bmc: Arc::new(Default::default()),
            in_mem_contract_bmc: Arc::new(Default::default()),
        };

        let bmc = Arc::new(bmc) as Arc<dyn Bmc>;
        let transfer_cargo_manager = Arc::new(TransferCargoManager::new());

        self.bmc = Some(bmc.clone());

        // Spawn the agent task
        let handle = Self::spawn_and_get_handle(shutdown_rx, client, bmc, transfer_cargo_manager);

        Ok(handle)
    }

    fn spawn_and_get_handle(
        shutdown_rx: Receiver<bool>,
        client: Arc<dyn StClientTrait>,
        bmc: Arc<dyn Bmc>,
        transfer_cargo_manager: Arc<TransferCargoManager>,
    ) -> JoinHandle<()> {
        
        tokio::spawn(async move {
            // Run agent with the authenticated client
            let agent_task = async {
                match run_agent(client, bmc, transfer_cargo_manager).await {
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
        })
    }

    async fn initialize_and_start_db_agent(&mut self, shutdown_rx: watch::Receiver<bool>) -> Result<JoinHandle<()>> {
        // Create a reset channel for this specific agent instance
        let (agent_reset_tx, _) = mpsc::channel::<ResetSignal>(8);

        // Create the initial client (without token) with reset detection
        let client_with_account_token = create_client(Some(self.cfg.spacetraders_account_token.clone()), Some(agent_reset_tx.clone()));
        let client_with_account_token = StClient::try_with_base_url(client_with_account_token, &self.cfg.spacetraders_base_url)?;

        // Get the status (this will verify the API is responding)
        let status = client_with_account_token.get_status().await?;

        // Initialize database pool
        let pool = db::prepare_database_schema(&status, self.cfg.pg_connection_string()).await?;

        // Get the authenticated client
        let authenticated_client = get_authenticated_client(&self.cfg, pool.clone(), client_with_account_token).await?;
        let client = Arc::new(authenticated_client) as Arc<dyn StClientTrait>;

        let db_mm = DbModelManager::new(pool.clone());
        let db_bmc = DbBmc::new(db_mm);
        let bmc = Arc::new(db_bmc) as Arc<dyn Bmc>;

        self.bmc = Some(bmc.clone());
        let transfer_cargo_manager = Arc::new(TransferCargoManager::new());

        let handle = Self::spawn_and_get_handle(shutdown_rx, client, bmc, transfer_cargo_manager);

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
