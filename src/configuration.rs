use crate::cli_args::Commands;
use crate::st_model::StStatusResponse;

#[derive(Debug)]
pub struct AgentConfiguration {
    pub database_url: String,
    pub spacetraders_agent_faction: String,
    pub spacetraders_agent_symbol: String,
    pub spacetraders_registration_email: String,
}

impl AgentConfiguration {
    pub fn pg_connection_string(self: &Self) -> String {
        self.database_url.clone()
    }

    pub fn get_schema_name(self: &Self, st_status_response: StStatusResponse) -> String {
        self.get_schema_name_for_reset_date(st_status_response.reset_date)
    }

    pub fn get_schema_name_for_reset_date(self: &Self, reset_date: String) -> String {
        format!("reset_{}", reset_date.replace("-", "_"))
    }

    pub fn new(commands: Commands) -> Self {
        match commands {
            Commands::RunAgent {
                database_url,
                spacetraders_agent_faction,
                spacetraders_agent_symbol,
                spacetraders_registration_email,
            } => Self {
                database_url,
                spacetraders_agent_faction,
                spacetraders_agent_symbol,
                spacetraders_registration_email,
            },
        }
    }
}
