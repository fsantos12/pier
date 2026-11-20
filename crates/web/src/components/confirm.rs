//! A button that asks for confirmation in a dialog before acting.

use leptos::prelude::*;

/// Shows `label`; clicking opens a dialog with `message`, and `on_confirm` runs only
/// after the user accepts.
#[component]
pub fn ConfirmButton(
    /// Text on the button.
    label: &'static str,
    /// Question shown in the dialog.
    #[prop(into)]
    message: String,
    /// Callback run after the user confirms.
    #[prop(into)]
    on_confirm: Callback<()>,
) -> impl IntoView {
    let open = RwSignal::new(false);
    view! {
        <button class="danger" on:click=move |_| open.set(true)>{label}</button>
        <Show when=move || open.get()>
            <div class="overlay">
                <div class="dialog" role="alertdialog">
                    <p>{message.clone()}</p>
                    <div class="buttons">
                        <button class="secondary" on:click=move |_| open.set(false)>"Cancel"</button>
                        <button
                            class="danger"
                            on:click=move |_| {
                                open.set(false);
                                on_confirm.run(());
                            }
                        >
                            {label}
                        </button>
                    </div>
                </div>
            </div>
        </Show>
    }
}
