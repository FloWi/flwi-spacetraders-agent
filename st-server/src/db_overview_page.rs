use leptos::prelude::*;
use leptos::{component, server, view, IntoView};
use serde::{Deserialize, Serialize};
use st_domain::StStatusResponse;

#[derive(Serialize, Deserialize, Clone)]
pub struct DbOverview {
    num_systems: i64,
    num_waypoints: i64,
    status: StStatusResponse,
}

#[server]
async fn get_db_overview() -> Result<DbOverview, ServerFnError> {
    use st_store::Ctx;

    let state = expect_context::<crate::app::AppState>();
    let bmc = state.bmc;

    let num_systems = bmc
        .system_bmc()
        .get_num_systems(&Ctx::Anonymous)
        .await
        .expect("num_systems");

    let num_waypoints = bmc
        .system_bmc()
        .get_num_waypoints(&Ctx::Anonymous)
        .await
        .expect("num_waypoints");

    let status = bmc
        .status_bmc()
        .get_status(&Ctx::Anonymous)
        .await
        .expect("status")
        .unwrap();

    Ok(DbOverview {
        num_systems,
        num_waypoints,
        status,
    })
}

#[component]
pub fn DbOverviewPage() -> impl IntoView {
    view! {
        <Await future=get_db_overview() let:data>
            {match data {
                Ok(data) => {
                    view! {
                        <p>
                            <span>"Number of systems: "</span>
                            <span>{data.num_systems}</span>
                            <span>" of "</span>
                            <span>{data.status.stats.systems}</span>
                        </p>
                        <p>
                            <span>"Number of waypoints: "</span>
                            <span>{data.num_waypoints}</span>
                            <span>" of "</span>
                            <span>{data.status.stats.waypoints}</span>
                        </p>
                    }
                        .into_any()
                }
                Err(err) => view! { <p>"Error: " {err.to_string()}</p> }.into_any(),
            }}
        </Await>
    }
}
