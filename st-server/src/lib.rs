pub mod app;
pub mod db_overview_page;
pub mod supply_chain_page;

#[cfg(feature = "ssr")]
pub mod cli_args;

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    use crate::app::*;
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(App);
}
