use anyhow::Result;
use clap;
use clap::{Parser, Subcommand};
use itertools::Itertools;
use lazy_static::lazy_static;
use st_core::agent_manager::AgentManager;
use st_core::behavior_tree::behavior_tree::Actionable;
use st_core::configuration::AgentConfiguration;
use st_server::cli_args::AppConfig;
use time::format_description;
use tracing_appender::non_blocking;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::fmt::time::UtcTime;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, registry::Registry, EnvFilter, Layer};

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
    setup_tracing_with_console();

    // for tokio-console (helps detecting deadlocks)
    // setup_tracing conflicts with this global subscriber, so it needs to be disabled
    //console_subscriber::init();

    let AppConfig {
        database_url,
        spacetraders_agent_faction,
        spacetraders_agent_symbol,
        spacetraders_registration_email,
        spacetraders_account_token,
        spacetraders_base_url,
        use_in_memory_agent,
        no_agent,
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
        no_agent,
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

fn setup_tracing_with_console() {
    // Create a file appender with daily rotation
    let file_appender = RollingFileAppender::new(Rotation::DAILY, "./logs/cli", "spaceTraders-cli.log.ndjson");

    // Create a non-blocking writer for the file appender
    let (non_blocking_appender, guard) = non_blocking(file_appender);

    // Format for timestamps
    let time_format = format_description::parse("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond]Z").expect("Invalid time format");
    let timer = UtcTime::new(time_format);

    let base_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // Create the console layer with colored output (for stdout)
    let console_layer = fmt::layer()
        .with_timer(timer.clone())
        .with_ansi(true)
        .with_target(true)
        .with_filter(base_filter);

    let base_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // Create the JSON file layer
    let file_layer = fmt::layer()
        .with_span_events(fmt::format::FmtSpan::CLOSE)
        .with_timer(timer)
        .with_ansi(false)
        .json()
        .with_current_span(true)
        .with_span_list(false)
        .with_writer(non_blocking_appender)
        .with_filter(base_filter);

    // Create the tokio-console layer
    let tokio_console_layer = console_subscriber::ConsoleLayer::builder()
        .retention(std::time::Duration::from_secs(60))
        .spawn();

    // Create the filter - IMPORTANT: Must include tokio=trace,runtime=trace for console
    let base_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // Append tokio traces to existing filter
    let filter_str = format!("{},tokio=trace,runtime=trace", base_filter);
    let filter = EnvFilter::new(filter_str);

    println!("Tokio console enabled with filter: {}", filter);

    // Combine all layers using Registry - layers first, then filter
    Registry::default()
        .with(tokio_console_layer) // Add console layer first
        .with(filter) // Filter goes last
        .with(console_layer) // Your stdout layer
        .with(file_layer) // Your file layer
        .init();

    // Store guard to keep it alive
    if let Ok(mut g) = GUARD.lock() {
        *g = Some(guard);
    }
}
