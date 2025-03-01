use st_domain::StStatusResponse;
use st_store::PgConnectionString;

#[derive(Clone, Debug)]
pub struct AgentConfiguration {
    pub database_url: String,
    pub spacetraders_agent_faction: String,
    pub spacetraders_agent_symbol: String,
    pub spacetraders_registration_email: String,
    pub spacetraders_account_token: String,
}

impl AgentConfiguration {
    pub fn pg_connection_string(self: &Self) -> PgConnectionString {
        PgConnectionString(self.database_url.clone())
    }
}
