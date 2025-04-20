use std::sync::Arc;

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use axum::Router;
    use leptos::logging::log;
    use leptos::prelude::*;
    use leptos_axum::generate_route_list;
    use leptos_axum::LeptosRoutes;
    use st_core::agent_manager::AgentManager;
    use st_core::configuration::AgentConfiguration;
    use st_server::app::{shell, App};
    use st_server::cli_args::AppConfig;
    use st_store::bmc::{Bmc, DbBmc};
    use st_store::{db, DbModelManager};
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    let conf = get_configuration(None).unwrap();
    let addr = conf.leptos_options.site_addr;
    let leptos_options = conf.leptos_options;

    let AppConfig {
        database_url,
        spacetraders_agent_faction,
        spacetraders_agent_symbol,
        spacetraders_registration_email,
        spacetraders_account_token,
        spacetraders_base_url,
    } = AppConfig::from_env().expect("cfg");

    tracing_subscriber::registry().with(fmt::layer().with_span_events(fmt::format::FmtSpan::CLOSE)).with(EnvFilter::from_default_env()).init();

    let cfg: AgentConfiguration = AgentConfiguration {
        database_url,
        spacetraders_agent_faction,
        spacetraders_agent_symbol,
        spacetraders_registration_email,
        spacetraders_account_token,
        spacetraders_base_url,
    };

    // Create the agent manager and get the reset channel
    let (mut agent_manager, _reset_tx) = AgentManager::new(cfg.clone());

    let pool = db::get_pg_connection_pool(cfg.pg_connection_string()).await.expect("should be able to get pool");

    let model_manager = DbModelManager::new(pool);
    let db_bmc = Arc::new(DbBmc::new(model_manager));

    let app_state = st_server::app::AppState {
        bmc: Arc::clone(&db_bmc) as Arc<dyn Bmc>,
    };

    // Generate the list of routes in your Leptos App
    let routes = generate_route_list(App);

    let app = Router::new()
        .leptos_routes_with_context(&leptos_options, routes, move || provide_context(app_state.clone()), {
            let leptos_options = leptos_options.clone();
            move || shell(leptos_options.clone())
        })
        .fallback(leptos_axum::file_and_error_handler(shell))
        .with_state(leptos_options);

    // run our app with hyper
    // `axum::Server` is a re-export of `hyper::Server`
    log!("listening on http://{}", &addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();

    // Run the agent manager in the background
    let agent_runner_handle = tokio::spawn(async move {
        if let Err(e) = agent_manager.run().await {
            eprintln!("Agent manager error: {}", e);
        }
    });

    axum::serve(listener, app.into_make_service()).await.unwrap();
    agent_runner_handle.await.unwrap();
}

#[cfg(not(feature = "ssr"))]
pub fn main() {
    // no client-side main function
    // unless we want this to work with e.g., Trunk for pure client-side testing
    // see lib.rs for hydration function instead
}
