use anyhow::Result;
use clap;
use clap::{Parser, Subcommand};
use itertools::Itertools;
use lazy_static::lazy_static;
use st_core::behavior_tree::behavior_tree::Actionable;
use st_core::configuration::AgentConfiguration;
use st_server::cli_args::AppConfig;
use time::format_description;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, registry::Registry, EnvFilter};

use st_core::agent_manager::AgentManager;
use tracing_subscriber::fmt::time::UtcTime;

/// SpaceTraders CLI utility
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Subcommand to run
    #[command(subcommand)]
    command: MyCommand,
}

/// Available commands
#[derive(Subcommand, Debug, Clone)]
enum MyCommand {
    RunServer,
}

#[tokio::main]
async fn main() -> Result<()> {
    setup_tracing();

    let AppConfig {
        database_url,
        spacetraders_agent_faction,
        spacetraders_agent_symbol,
        spacetraders_registration_email,
        spacetraders_account_token,
        spacetraders_base_url,
        use_in_memory_agent,
    } = AppConfig::from_env().expect("cfg");

    //tracing_subscriber::registry().with(fmt::layer().with_span_events(fmt::format::FmtSpan::CLOSE)).with(EnvFilter::from_default_env()).init();

    let cfg: AgentConfiguration = AgentConfiguration {
        database_url,
        spacetraders_agent_faction,
        spacetraders_agent_symbol,
        spacetraders_registration_email,
        spacetraders_account_token,
        spacetraders_base_url,
        use_in_memory_agent,
    };

    let args = Args::parse();

    match args.command {
        MyCommand::RunServer => {
            // Create the agent manager and get the reset channel
            let (mut agent_manager, _reset_tx) = AgentManager::new(cfg.clone());
            agent_manager.run().await?
        }
    }

    Ok(())
}

lazy_static! {
    static ref GUARD: std::sync::Mutex<Option<tracing_appender::non_blocking::WorkerGuard>> = std::sync::Mutex::new(None);
}

fn setup_tracing() {
    // Create a file appender with daily rotation
    let file_appender = RollingFileAppender::new(Rotation::DAILY, "./logs/cli", "spaceTraders-cli.log.ndjson");

    // Create a non-blocking writer for the file appender
    let (non_blocking_appender, guard) = tracing_appender::non_blocking(file_appender);

    // Format for timestamps
    let time_format = format_description::parse("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond]Z").expect("Invalid time format");

    let timer = UtcTime::new(time_format);

    // Create the console layer with colored output
    let console_layer = fmt::layer()
        .with_timer(timer.clone())
        .with_ansi(true)
        .with_target(true);
    //.pretty();

    // Create the JSON file layer
    let file_layer = fmt::layer()
        .with_span_events(fmt::format::FmtSpan::CLOSE) // Only log spans when they close
        .with_timer(timer)
        .with_ansi(false)
        .json()
        .with_current_span(true) // Keep just one current span
        .with_span_list(false) // Don't include the full span list
        .with_writer(non_blocking_appender);

    // Create the filter
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // Register all layers
    Registry::default()
        .with(filter)
        .with(console_layer)
        .with(file_layer)
        .init();

    // Store guard in a static to keep it alive for the program duration
    if let Ok(mut g) = GUARD.lock() {
        *g = Some(guard);
    }
}
