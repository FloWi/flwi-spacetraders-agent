use leptos::html::{button, Button, HtmlElement};
use leptos::prelude::ClassAttribute;
use leptos::tachys::html::class::Class;

pub fn styled_button() -> HtmlElement<Button, (Class<&'static str>,), ()> {
    button().class(
        r#"inline-flex
    items-center
    justify-center
    whitespace-nowrap
    text-sm
    font-medium
    ring-offset-background
    transition-colors
    focus-visible:outline-none
    focus-visible:ring-2
    focus-visible:ring-ring
    focus-visible:ring-offset-2
    disabled:pointer-events-none
    disabled:opacity-50
    border
    border-input
    bg-primary
    text-primary-foreground
    hover:bg-accent
    hover:text-accent-foreground
    h-9
    rounded-md
    px-3
    w-fit
    "#,
    )
}
