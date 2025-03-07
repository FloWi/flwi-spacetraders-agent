use std::any::Any;

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use axum::routing::get;
    use axum::Router;
    use leptos::logging::log;
    use leptos::prelude::*;
    use leptos_axum::generate_route_list;
    use leptos_axum::LeptosRoutes;
    use st_core::configuration::AgentConfiguration;
    use st_core::reqwest_helpers::create_client;
    use st_core::st_client::{StClient, StClientTrait};
    use st_server::app::{shell, App};
    use st_server::cli_args::AppConfig;
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
    } = AppConfig::from_env().expect("cfg");

    tracing_subscriber::registry()
        .with(fmt::layer().with_span_events(fmt::format::FmtSpan::CLOSE))
        .with(EnvFilter::from_default_env())
        .init();

    let cfg: AgentConfiguration = AgentConfiguration {
        database_url,
        spacetraders_agent_faction,
        spacetraders_agent_symbol,
        spacetraders_registration_email,
        spacetraders_account_token,
    };

    let client_with_account_token =
        StClient::new(create_client(Some(cfg.spacetraders_account_token.clone())));

    let status = client_with_account_token
        .get_status()
        .await
        .expect("get_status should work");

    let pool = db::prepare_database_schema(&status, cfg.pg_connection_string())
        .await
        .expect("should be able to get pool");

    let model_manager = DbModelManager::new(pool);

    // let app_state = app::AppState::new(model_manager.clone());
    let app_state = st_server::app::AppState {
        db_model_manager: model_manager.clone(),
    };
    println!("inside main.rs: AppState type is {:?}", app_state.type_id());

    // Generate the list of routes in your Leptos App
    let routes = generate_route_list(App);

    let app = Router::new()
        .leptos_routes_with_context(
            &leptos_options,
            routes,
            move || provide_context(app_state.clone()),
            {
                let leptos_options = leptos_options.clone();
                move || shell(leptos_options.clone())
            },
        )
        .fallback(leptos_axum::file_and_error_handler(shell))
        .with_state(leptos_options);

    // run our app with hyper
    // `axum::Server` is a re-export of `hyper::Server`
    log!("listening on http://{}", &addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();

    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}

#[cfg(not(feature = "ssr"))]
pub fn main() {
    // no client-side main function
    // unless we want this to work with e.g., Trunk for pure client-side testing
    // see lib.rs for hydration function instead
}
