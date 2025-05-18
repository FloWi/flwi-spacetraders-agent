use std::str::FromStr;

#[derive(Debug)]
pub struct AppConfig {
    pub database_url: String,
    pub spacetraders_agent_faction: String,
    pub spacetraders_agent_symbol: String,
    pub spacetraders_registration_email: String,
    pub spacetraders_account_token: String,
    pub spacetraders_base_url: String,
    pub use_in_memory_agent: bool,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, String> {
        // Helper function to get a variable with a custom error message
        fn get_env_var(name: &str) -> Result<String, String> {
            std::env::var(name).map_err(|_| format!("Environment variable '{}' is not set", name))
        }

        Ok(Self {
            database_url: get_env_var("DATABASE_URL")?,
            spacetraders_agent_faction: get_env_var("SPACETRADERS_AGENT_FACTION")?,
            spacetraders_agent_symbol: get_env_var("SPACETRADERS_AGENT_SYMBOL")?,
            spacetraders_registration_email: get_env_var("SPACETRADERS_REGISTRATION_EMAIL")?,
            spacetraders_account_token: get_env_var("SPACETRADERS_ACCOUNT_TOKEN")?,
            spacetraders_base_url: get_env_var("SPACETRADERS_BASE_URL")?,
            use_in_memory_agent: bool::from_str(
                get_env_var("SPACETRADERS_USE_IN_MEMORY_AGENT")
                    .unwrap_or("false".to_string())
                    .as_str(),
            )
            .unwrap_or(false),
        })
    }
}
