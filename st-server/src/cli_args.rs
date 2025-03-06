pub struct AppConfig {
    pub database_url: String,
    pub spacetraders_agent_faction: String,
    pub spacetraders_agent_symbol: String,
    pub spacetraders_registration_email: String,
    pub spacetraders_account_token: String,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, String> {
        // Load from .env file if it exists
        let _ = dotenv::dotenv();

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
        })
    }
}
