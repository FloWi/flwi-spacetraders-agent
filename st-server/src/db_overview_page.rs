use leptos::prelude::*;
use leptos::{component, server, view, IntoView};
use serde::{Deserialize, Serialize};
use std::any::Any;

#[derive(Serialize, Deserialize, Clone)]
pub struct DbOverview {
    num_systems: i64,
    num_waypoints: i64,
}

#[server]
async fn get_db_overview() -> Result<DbOverview, ServerFnError> {
    use st_store::{Ctx, StatusBmc};

    let state = expect_context::<crate::app::AppState>();
    let mm = state.db_model_manager;

    let num_systems = StatusBmc::get_num_systems(&Ctx::Anonymous, &mm)
        .await
        .expect("num_systems");

    let num_waypoints = StatusBmc::get_num_waypoints(&Ctx::Anonymous, &mm)
        .await
        .expect("num_waypoints");

    Ok(DbOverview {
        num_systems,
        num_waypoints,
    })

    // use axum::extract::State;
    // use leptos_axum::extract_with_state;
    // pub use st_core::app_state::AppState;
    // use store {
    //     ctx::Ctx,
    //     FleetBmc,
    //     DbModelManager,
    // };
    //
    // let state = expect_context::<AppState>();
    // let State(mm) = extract_with_state::<State<DbModelManager>, _>(&state).await?;
    //
    // let fleet = FleetBmc::get_by_name(&Ctx::Anonymous, &mm, &fleet).await?;
    //
    // Ok(fleet)
}

#[component]
pub fn DbOverviewPage() -> impl IntoView {
    // Use create_resource which is the standard way to handle async data in Leptos
    let db_overview = OnceResource::new(get_db_overview());

    view! {
        <Suspense fallback=|| ()>
            {move || Suspend::new(async move {
                let data = db_overview.await;
                view! {
                    <div class="flex flex-col gap-4">
                        <p>
                            <span>"Number of systems: "</span>
                            <span>{db_overview.get().unwrap().unwrap().num_systems}</span>
                        </p>
                        <p>
                            <span>"Number of waypoints: "</span>
                            <span>{db_overview.get().unwrap().unwrap().num_waypoints}</span>
                        </p>
                    </div>
                }
            })}
        </Suspense>
    }
}
