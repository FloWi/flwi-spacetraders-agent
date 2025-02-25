use clap::{Parser, Subcommand};

#[derive(Clone, Parser)]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Clone, Subcommand)]
pub enum Commands {
    /// runs the agent
    RunAgent {
        #[arg(long, env("DATABASE_URL"))]
        database_url: String,
        #[arg(long, env("SPACETRADERS_AGENT_FACTION"))]
        spacetraders_agent_faction: String,
        #[arg(long, env("SPACETRADERS_AGENT_SYMBOL"))]
        spacetraders_agent_symbol: String,
        #[arg(long, env("SPACETRADERS_REGISTRATION_EMAIL"))]
        spacetraders_registration_email: String,
        #[arg(long, env("SPACETRADERS_ACCOUNT_TOKEN"))]
        spacetraders_account_token: String,
    },
}
