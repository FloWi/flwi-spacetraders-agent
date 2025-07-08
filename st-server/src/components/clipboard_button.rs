use leptos::leptos_dom::error;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos::{component, view, IntoView};
use phosphor_leptos::{Icon, COPY_SIMPLE};

async fn write_to_clipboard(text: String) {
    use wasm_bindgen_futures::JsFuture;

    let maybe_clipboard = leptos::web_sys::window().map(|w| w.navigator().clipboard());
    match maybe_clipboard {
        Some(cp) => match JsFuture::from(cp.write_text(text.as_str()))
            .await
            .map_err(|err| format!("Error writing to clipboard: {:?}", err))
        {
            Ok(_) => {}
            Err(_) => {
                error!("Can't write to clipboard")
            }
        },
        None => error!("Can't write to clipboard"),
    }
}

#[component]
pub fn ClipboardButton(clipboard_text: String, label: String) -> impl IntoView {
    view! {
        <button
            class="p-0.5 w-fit border border-rounded border-solid"
            on:click=move |_| spawn_local(write_to_clipboard(clipboard_text.clone()))
        >
            <Icon icon=COPY_SIMPLE size="2em" />
            <p>{label}</p>
        </button>
    }
}
